//! Credential storage.
//!
//! A Discord token is a full-account credential, so it belongs in the OS keychain rather than a
//! config file. This module defines the storage *trait* and an in-memory implementation, keeping
//! the concrete keyring backend a separate concern.

use crate::ids::Snowflake;

/// A stored account credential.
///
/// `Debug` is implemented by hand so the token can never reach a log line, including through a
/// container that derives its own `Debug`. Redaction lives on the credential rather than on a
/// wrapper, because a wrapper is one refactor away from being bypassed.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    /// The account's user id.
    pub user_id: Snowflake,
    /// Username, for display only.
    pub username: String,
    /// The authentication token.
    pub token: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("user_id", &self.user_id)
            .field("username", &self.username)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Why a credential operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// No credential is stored under this key.
    #[error("no credential stored for {0}")]
    NotFound(String),
    /// The underlying store refused.
    #[error("credential store: {0}")]
    Backend(String),
}

impl StoreError {
    /// Builds a `NotFound` for an account.
    #[must_use]
    pub fn missing(user_id: Snowflake) -> Self {
        Self::NotFound(user_id.to_string())
    }
}

/// A place credentials can be kept.
///
/// Implementations must not log tokens, and must not include them in `Debug` output.
pub trait CredentialStore: std::fmt::Debug + Send + Sync {
    /// Reads a credential.
    ///
    /// # Errors
    /// Returns [`StoreError::NotFound`] if the account is not stored.
    fn get(&self, user_id: Snowflake) -> Result<Credential, StoreError>;

    /// Writes a credential, replacing any existing one.
    ///
    /// # Errors
    /// Returns [`StoreError::Backend`] if the store refuses the write.
    fn put(&self, credential: &Credential) -> Result<(), StoreError>;

    /// Removes a credential. Removing an absent credential is not an error.
    ///
    /// # Errors
    /// Returns [`StoreError::Backend`] if the store refuses the removal.
    fn remove(&self, user_id: Snowflake) -> Result<(), StoreError>;

    /// Every stored account id.
    fn list(&self) -> Result<Vec<Snowflake>, StoreError>;
}

/// An in-memory store, used by tests and by the CLI before the keyring is unlocked.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: std::sync::Mutex<Vec<Credential>>,
}

impl MemoryStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored credentials.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().map_or(0, |e| e.len())
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CredentialStore for MemoryStore {
    fn get(&self, user_id: Snowflake) -> Result<Credential, StoreError> {
        let entries = self
            .entries
            .lock()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        entries
            .iter()
            .find(|c| c.user_id == user_id)
            .cloned()
            .ok_or_else(|| StoreError::missing(user_id))
    }

    fn put(&self, credential: &Credential) -> Result<(), StoreError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        match entries.iter_mut().find(|c| c.user_id == credential.user_id) {
            Some(existing) => *existing = credential.clone(),
            None => entries.push(credential.clone()),
        }
        Ok(())
    }

    fn remove(&self, user_id: Snowflake) -> Result<(), StoreError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        entries.retain(|c| c.user_id != user_id);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Snowflake>, StoreError> {
        let entries = self
            .entries
            .lock()
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(entries.iter().map(|c| c.user_id).collect())
    }
}

/// A wrapper that makes redaction explicit at call sites.
///
/// Since [`Credential`] already redacts its own `Debug`, this adds nothing functionally; it exists
/// only where a reviewer wants the intent stated at the call site.
#[derive(Debug)]
pub struct Redacting<S>(S);

impl<S> Redacting<S> {
    /// Wraps a store.
    pub const fn new(inner: S) -> Self {
        Self(inner)
    }

    /// The wrapped store.
    pub fn inner(&self) -> &S {
        &self.0
    }
}

impl<S: CredentialStore> CredentialStore for Redacting<S> {
    fn get(&self, user_id: Snowflake) -> Result<Credential, StoreError> {
        self.0.get(user_id)
    }
    fn put(&self, credential: &Credential) -> Result<(), StoreError> {
        self.0.put(credential)
    }
    fn remove(&self, user_id: Snowflake) -> Result<(), StoreError> {
        self.0.remove(user_id)
    }
    fn list(&self) -> Result<Vec<Snowflake>, StoreError> {
        self.0.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(id: u64) -> Credential {
        Credential {
            user_id: Snowflake::new(id),
            username: format!("user{id}"),
            token: format!("secret-token-{id}"),
        }
    }

    #[test]
    fn stores_and_reads_back() {
        let store = MemoryStore::new();
        store.put(&credential(1)).unwrap();
        assert_eq!(
            store.get(Snowflake::new(1)).unwrap().token,
            "secret-token-1"
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn missing_credentials_report_the_account() {
        let store = MemoryStore::new();
        let err = store.get(Snowflake::new(9)).unwrap_err();
        assert!(matches!(err, StoreError::NotFound(_)));
        assert!(err.to_string().contains('9'));
    }

    #[test]
    fn putting_twice_replaces_rather_than_appends() {
        let store = MemoryStore::new();
        store.put(&credential(1)).unwrap();
        let mut second = credential(1);
        second.token = "rotated".to_owned();
        store.put(&second).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(Snowflake::new(1)).unwrap().token, "rotated");
    }

    #[test]
    fn remove_is_idempotent() {
        let store = MemoryStore::new();
        store.put(&credential(1)).unwrap();
        store.remove(Snowflake::new(1)).unwrap();
        store.remove(Snowflake::new(1)).unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn lists_accounts() {
        let store = MemoryStore::new();
        store.put(&credential(1)).unwrap();
        store.put(&credential(2)).unwrap();
        let mut ids = store.list().unwrap();
        ids.sort();
        assert_eq!(ids, vec![Snowflake::new(1), Snowflake::new(2)]);
    }

    #[test]
    fn debug_output_does_not_leak_tokens() {
        let store = MemoryStore::new();
        store.put(&credential(1)).unwrap();

        // The store derives Debug, so redaction has to live on the credential itself.
        assert!(
            !format!("{store:?}").contains("secret-token-1"),
            "MemoryStore leaked a token"
        );
        assert!(
            !format!("{:?}", credential(1)).contains("secret-token-1"),
            "Credential leaked a token"
        );
        assert!(!format!("{:?}", Redacting::new(&store)).contains("secret-token-1"));
        // The non-secret fields should still be visible, or the Debug impl would be useless.
        let rendered = format!("{:?}", credential(1));
        assert!(
            rendered.contains("user1"),
            "expected the username to survive: {rendered}"
        );
    }

    #[test]
    fn redacting_wrapper_forwards_everything() {
        let store = Redacting::new(MemoryStore::new());
        store.put(&credential(1)).unwrap();
        assert!(store.get(Snowflake::new(1)).is_ok());
        assert_eq!(store.list().unwrap().len(), 1);
        store.remove(Snowflake::new(1)).unwrap();
        assert!(store.get(Snowflake::new(1)).is_err());
    }
}
