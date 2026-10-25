//! Credential storage.

use secrecy::SecretString;

use crate::PlatformError;

/// Secrets by name (`eileen-shell-win`: Windows Credential Manager through `keyring-core`;
/// Linux later: Secret Service). `eileen-config` receives it from `eileen-cli`.
pub trait SecretStore: Send + Sync {
    /// The secret stored under `name`, if any.
    fn get(&self, name: &str) -> Result<Option<SecretString>, PlatformError>;
    /// Stores `value` under `name`, replacing what was there.
    fn set(&self, name: &str, value: &SecretString) -> Result<(), PlatformError>;
    /// Removes the secret under `name`; removing a missing one is not an error.
    fn delete(&self, name: &str) -> Result<(), PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeSecretStore;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use secrecy::{ExposeSecret, SecretString};

    use super::SecretStore;
    use crate::PlatformError;
    use crate::fake_util::lock;

    /// Secrets in memory.
    #[derive(Debug, Default)]
    pub struct FakeSecretStore {
        secrets: Mutex<BTreeMap<String, SecretString>>,
    }

    impl FakeSecretStore {
        /// The names stored, in order.
        #[must_use]
        pub fn names(&self) -> Vec<String> {
            lock(&self.secrets).keys().cloned().collect()
        }
    }

    impl SecretStore for FakeSecretStore {
        fn get(&self, name: &str) -> Result<Option<SecretString>, PlatformError> {
            Ok(lock(&self.secrets).get(name).cloned())
        }

        fn set(&self, name: &str, value: &SecretString) -> Result<(), PlatformError> {
            let copy = SecretString::from(value.expose_secret());
            lock(&self.secrets).insert(name.to_owned(), copy);
            Ok(())
        }

        fn delete(&self, name: &str) -> Result<(), PlatformError> {
            lock(&self.secrets).remove(name);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::{FakeSecretStore, SecretStore};

    #[test]
    fn secrets_round_trip_and_never_show_in_debug() {
        let store = FakeSecretStore::default();
        store.set("deepseek", &SecretString::from("sk-test-value")).unwrap();
        let got = store.get("deepseek").unwrap().unwrap();
        assert_eq!(got.expose_secret(), "sk-test-value", "round trip");
        assert!(!format!("{store:?}").contains("sk-test-value"), "redacted in Debug");
        store.delete("deepseek").unwrap();
        store.delete("deepseek").unwrap();
        assert!(store.names().is_empty(), "deleted, twice without error");
    }
}
