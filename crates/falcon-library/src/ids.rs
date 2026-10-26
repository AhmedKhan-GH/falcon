//! Item and folder IDs.
//!
//! Eagle builds an ID from the creation time in milliseconds, written in base
//! 36, followed by 5 random base-36 characters, all upper case. Eagle's own
//! random part can come out shorter than 5 characters; falcon always pads it,
//! because Eagle only loads `.info` folders whose name is 13 or 36 long.

use std::time::{SystemTime, UNIX_EPOCH};

const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const SUFFIX_LEN: usize = 5;

/// A new ID for the current time.
pub fn new_id() -> String {
    id_at(now_ms(), &mut fastrand::Rng::new())
}

/// An ID for `ms` (milliseconds since the epoch) using `rng` for the suffix.
pub fn id_at(ms: u64, rng: &mut fastrand::Rng) -> String {
    let mut id = to_base36(ms);
    id.extend((0..SUFFIX_LEN).map(|_| DIGITS[rng.usize(..36)] as char));
    id
}

/// The creation time encoded in an ID's prefix, in milliseconds.
///
/// Returns `None` for legacy 36-character UUIDs and malformed IDs.
pub fn timestamp(id: &str) -> Option<u64> {
    let prefix = id.len().checked_sub(SUFFIX_LEN).filter(|_| id.len() != 36)?;
    u64::from_str_radix(&id[..prefix], 36).ok()
}

/// Whether Eagle would load an `.info` folder with this ID.
pub fn is_valid(id: &str) -> bool {
    matches!(id.len(), 13 | 36)
}

fn to_base36(mut n: u64) -> String {
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).expect("base-36 digits are ASCII")
}

fn now_ms() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    since.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matches_fixture_ids() {
        // Taken from the Assets fixture, where each id's prefix equals the
        // item's modificationTime.
        assert_eq!(timestamp("MVP1BZ832CIJ1"), Some(1793005755411));
        assert_eq!(timestamp("MVP1JTFJI1HWU"), Some(1793006121151));
        assert_eq!(to_base36(1793005755411), "MVP1BZ83");
    }

    #[test]
    fn generated_ids_round_trip() {
        let mut rng = fastrand::Rng::with_seed(7);
        let id = id_at(1793005755411, &mut rng);
        assert_eq!(id.len(), 13);
        assert!(id.starts_with("MVP1BZ83"));
        assert!(is_valid(&id));
        assert_eq!(timestamp(&id), Some(1793005755411));
    }

    #[test]
    fn new_ids_sort_by_time() {
        let mut rng = fastrand::Rng::with_seed(1);
        assert!(id_at(1_000_000_000_000, &mut rng) < id_at(1_000_000_000_001, &mut rng));
    }
}
