//! API keys: in the macOS Keychain, and nowhere else.
//!
//! A [`Secret`] prints as `[key]` in `Debug` and has no `Display`, so a key
//! can't slip into a log line or an error text by formatting. Tests use
//! [`MemoryKeys`], or a [`Keychain`] with a throwaway service name.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use crate::provider::ProviderId;

/// An API key.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(key: impl Into<String>) -> Secret {
        Secret(key.into())
    }

    /// The key itself, for the one header it goes into.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[key]")
    }
}

/// Where keys are kept, one per provider. Errors are plain text without
/// the key.
pub trait KeyStore: Send + Sync {
    fn get(&self, provider: ProviderId) -> Result<Option<Secret>, String>;
    fn set(&self, provider: ProviderId, key: &Secret) -> Result<(), String>;
    /// Removing a key that isn't there is no error.
    fn delete(&self, provider: ProviderId) -> Result<(), String>;
    /// Whether a key is stored, without reading it.
    fn has(&self, provider: ProviderId) -> Result<bool, String>;
}

/// A test double.
#[derive(Default)]
pub struct MemoryKeys(Mutex<HashMap<ProviderId, Secret>>);

impl MemoryKeys {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<ProviderId, Secret>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl KeyStore for MemoryKeys {
    fn get(&self, provider: ProviderId) -> Result<Option<Secret>, String> {
        Ok(self.map().get(&provider).cloned())
    }

    fn set(&self, provider: ProviderId, key: &Secret) -> Result<(), String> {
        self.map().insert(provider, key.clone());
        Ok(())
    }

    fn delete(&self, provider: ProviderId) -> Result<(), String> {
        self.map().remove(&provider);
        Ok(())
    }

    fn has(&self, provider: ProviderId) -> Result<bool, String> {
        Ok(self.map().contains_key(&provider))
    }
}

/// Generic passwords in the login Keychain: the service is falcon's, the
/// account is the provider id.
pub struct Keychain {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    service: String,
}

impl Keychain {
    /// The service name of falcon's own entries.
    pub const SERVICE: &'static str = "Falcon AI Models";

    pub fn new(service: impl Into<String>) -> Keychain {
        Keychain { service: service.into() }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use security_framework::base::Error;
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    use super::*;

    const NOT_FOUND: i32 = -25300;

    fn text(e: &Error) -> String {
        e.message().unwrap_or_else(|| format!("Keychain error {}", e.code()))
    }

    impl KeyStore for Keychain {
        fn get(&self, provider: ProviderId) -> Result<Option<Secret>, String> {
            match get_generic_password(&self.service, provider.id()) {
                Ok(bytes) => String::from_utf8(bytes).map(|k| Some(Secret(k))).map_err(|_| "The stored key is not text".to_string()),
                Err(e) if e.code() == NOT_FOUND => Ok(None),
                Err(e) => Err(text(&e)),
            }
        }

        fn set(&self, provider: ProviderId, key: &Secret) -> Result<(), String> {
            set_generic_password(&self.service, provider.id(), key.expose().as_bytes()).map_err(|e| text(&e))
        }

        fn delete(&self, provider: ProviderId) -> Result<(), String> {
            match delete_generic_password(&self.service, provider.id()) {
                Ok(()) => Ok(()),
                Err(e) if e.code() == NOT_FOUND => Ok(()),
                Err(e) => Err(text(&e)),
            }
        }

        fn has(&self, provider: ProviderId) -> Result<bool, String> {
            let found = ItemSearchOptions::new().class(ItemClass::generic_password()).service(&self.service).account(provider.id()).load_attributes(true).limit(Limit::Max(1)).search();
            match found {
                Ok(items) => Ok(!items.is_empty()),
                Err(e) if e.code() == NOT_FOUND => Ok(false),
                Err(e) => Err(text(&e)),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl KeyStore for Keychain {
    fn get(&self, _provider: ProviderId) -> Result<Option<Secret>, String> {
        Ok(None)
    }

    fn set(&self, _provider: ProviderId, _key: &Secret) -> Result<(), String> {
        Err("Keys are kept in the macOS Keychain, which this system doesn't have".to_string())
    }

    fn delete(&self, _provider: ProviderId) -> Result<(), String> {
        Ok(())
    }

    fn has(&self, _provider: ProviderId) -> Result<bool, String> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_does_not_print() {
        assert_eq!(format!("{:?}", Secret::new("sk-123")), "[key]");
    }

    #[test]
    fn memory_keys_round_trip() {
        let k = MemoryKeys::default();
        assert_eq!(k.get(ProviderId::OpenAi).unwrap(), None);
        assert!(!k.has(ProviderId::OpenAi).unwrap());
        k.set(ProviderId::OpenAi, &Secret::new("a")).unwrap();
        assert!(k.has(ProviderId::OpenAi).unwrap());
        assert_eq!(k.get(ProviderId::OpenAi).unwrap(), Some(Secret::new("a")));
        k.delete(ProviderId::OpenAi).unwrap();
        k.delete(ProviderId::OpenAi).unwrap();
        assert_eq!(k.get(ProviderId::OpenAi).unwrap(), None);
    }

    /// The real Keychain, with a service name used by nothing else. Ignored
    /// so that `cargo test` never touches the login Keychain.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses the login Keychain"]
    fn keychain_round_trip_with_a_throwaway_service() {
        let service = format!("falcon-test-{}-{:x}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let k = Keychain::new(service);
        let p = ProviderId::OpenAi;
        assert_eq!(k.get(p).unwrap(), None);
        assert!(!k.has(p).unwrap());
        k.set(p, &Secret::new("first")).unwrap();
        k.set(p, &Secret::new("second")).unwrap();
        let read = k.get(p);
        let had = k.has(p);
        k.delete(p).unwrap();
        assert_eq!(read.unwrap(), Some(Secret::new("second")));
        assert!(had.unwrap());
        assert_eq!(k.get(p).unwrap(), None);
        k.delete(p).unwrap();
    }
}
