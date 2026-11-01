//! The app lock of Preferences ▸ Password, after Eagle's privacy setting:
//! with a password set, falcon opens to a lock screen and Lock Now (⌥⌘L)
//! hides the window's contents until the password is typed.
//!
//! The password is kept in the app state (never in a library) as a salted,
//! stretched BLAKE3 digest, not in Eagle's reversible base64. It guards the
//! window only: the library files stay readable, as they do with Eagle's.

/// Hash rounds, so guessing a password costs more than one hash each.
const ROUNDS: u32 = 20_000;

/// A stored app password.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Secret {
    salt: String,
    hash: String,
    /// Shown on the lock screen on request, as Eagle's password tip.
    pub hint: String,
}

impl Secret {
    pub fn new(password: &str, hint: &str) -> Secret {
        Secret::with_salt(password, &fresh_salt(), hint)
    }

    fn with_salt(password: &str, salt: &str, hint: &str) -> Secret {
        Secret { salt: salt.to_string(), hash: digest(salt, password), hint: hint.trim().to_string() }
    }

    pub fn matches(&self, password: &str) -> bool {
        let typed = digest(&self.salt, password);
        // Compare in constant time.
        typed.len() == self.hash.len() && typed.bytes().zip(self.hash.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

fn digest(salt: &str, password: &str) -> String {
    let mut h = blake3::Hasher::new_derive_key("falcon app lock v1");
    h.update(salt.as_bytes());
    h.update(&[0]);
    h.update(password.as_bytes());
    let mut out = h.finalize();
    for _ in 1..ROUNDS {
        out = blake3::hash(out.as_bytes());
    }
    out.to_hex().to_string()
}

/// A salt that differs per password set: the time, the process and the
/// standard library's per-process random hash keys.
fn fresh_salt() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = blake3::Hasher::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    h.update(&now.as_nanos().to_le_bytes());
    h.update(&std::process::id().to_le_bytes());
    let mut s = std::collections::hash_map::RandomState::new().build_hasher();
    s.write_u128(now.as_nanos());
    h.update(&s.finish().to_le_bytes());
    h.finalize().to_hex()[..32].to_string()
}

/// Checks a new password typed twice.
pub fn check_new(new: &str, again: &str) -> Result<(), &'static str> {
    if new.is_empty() {
        return Err("Enter a password.");
    }
    if new != again {
        return Err("The passwords don't match.");
    }
    Ok(())
}

/// Whether the window is locked, and the lock screen's field.
#[derive(Default)]
pub struct Lock {
    locked: bool,
    pub typed: String,
    /// The last password typed was wrong.
    pub failed: bool,
    pub show_hint: bool,
    /// A Touch ID request waiting for its answer (`touch_id`).
    pub touch_ticket: Option<u64>,
    /// The last Touch ID attempt failed or was cancelled.
    pub touch_failed: bool,
}

impl Lock {
    /// Locked at launch whenever a password is set.
    pub fn at_launch(secret: Option<&Secret>) -> Lock {
        Lock { locked: secret.is_some(), ..Default::default() }
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Lock Now: locks if a password is set. Returns whether it locked.
    pub fn lock(&mut self, secret: Option<&Secret>) -> bool {
        if secret.is_some() {
            *self = Lock { locked: true, ..Default::default() };
        }
        self.locked
    }

    /// Takes a Touch ID answer: a match unlocks (Touch ID only stands in
    /// for the password when one is set). Returns whether it unlocked.
    pub fn touch_answer(&mut self, matched: bool) -> bool {
        self.touch_ticket = None;
        if matched {
            *self = Lock::default();
        } else {
            self.touch_failed = true;
        }
        matched
    }

    /// Checks the typed password; unlocks on a match (or when the
    /// password has been removed meanwhile). Returns whether it unlocked.
    pub fn try_unlock(&mut self, secret: Option<&Secret>) -> bool {
        if secret.is_none_or(|s| s.matches(&self.typed)) {
            *self = Lock::default();
            true
        } else {
            self.failed = true;
            self.typed.clear();
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_check_passwords() {
        let s = Secret::new("hunter2", " a hint ");
        assert!(s.matches("hunter2"));
        assert!(!s.matches("hunter3") && !s.matches("") && !s.matches("Hunter2"));
        assert_eq!(s.hint, "a hint");
        assert!(!s.hash.contains("hunter2"), "the password itself is not stored");
        let t = Secret::new("hunter2", "");
        assert_ne!(s.salt, t.salt, "each password gets its own salt");
        assert_ne!(s.hash, t.hash);
        assert_eq!(Secret::with_salt("pw", "salt", ""), Secret::with_salt("pw", "salt", ""));
        assert!(Secret::new("密码 café", "").matches("密码 café"), "any characters work");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Secret>(&json).unwrap(), s);
    }

    #[test]
    fn new_passwords() {
        assert_eq!(check_new("a", "a"), Ok(()));
        assert!(check_new("", "").is_err());
        assert!(check_new("a", "b").is_err());
    }

    #[test]
    fn touch_id_answers() {
        let s = Secret::with_salt("pw", "salt", "");
        let mut l = Lock::at_launch(Some(&s));
        l.touch_ticket = Some(3);
        assert!(!l.touch_answer(false));
        assert!(l.is_locked() && l.touch_failed && l.touch_ticket.is_none());
        l.touch_ticket = Some(4);
        assert!(l.touch_answer(true));
        assert!(!l.is_locked() && !l.touch_failed);
    }

    #[test]
    fn lock_states() {
        let s = Secret::with_salt("pw", "salt", "tip");
        assert!(!Lock::at_launch(None).is_locked());
        let mut l = Lock::at_launch(Some(&s));
        assert!(l.is_locked(), "a password locks the app at launch");
        l.typed = "nope".into();
        assert!(!l.try_unlock(Some(&s)));
        assert!(l.is_locked() && l.failed && l.typed.is_empty());
        l.typed = "pw".into();
        assert!(l.try_unlock(Some(&s)));
        assert!(!l.is_locked() && !l.failed);
        assert!(!l.lock(None), "Lock Now needs a password");
        assert!(l.lock(Some(&s)));
        l.typed = "x".into();
        assert!(l.try_unlock(None), "a removed password unlocks");
    }
}
