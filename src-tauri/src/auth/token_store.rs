use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

const ENTRY_USERNAME: &str = "tokens";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub scope: Vec<String>,
    pub user_id: String,
}

#[derive(Debug)]
pub enum TokenStoreError {
    Keyring(String),
    Serialization(String),
}

impl From<KeyringError> for TokenStoreError {
    fn from(err: KeyringError) -> Self {
        TokenStoreError::Keyring(err.to_string())
    }
}

impl From<serde_json::Error> for TokenStoreError {
    fn from(err: serde_json::Error) -> Self {
        TokenStoreError::Serialization(err.to_string())
    }
}

pub fn save_tokens(service: &str, tokens: &TokenSet) -> Result<(), TokenStoreError> {
    let entry = Entry::new(service, ENTRY_USERNAME)?;
    let json = serde_json::to_string(tokens)?;
    entry.set_password(&json)?;
    Ok(())
}

pub fn load_tokens(service: &str) -> Result<Option<TokenSet>, TokenStoreError> {
    let entry = Entry::new(service, ENTRY_USERNAME)?;
    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn clear_tokens(service: &str) -> Result<(), TokenStoreError> {
    let entry = Entry::new(service, ENTRY_USERNAME)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_credential_manager() {
        let service = "twitch-clip-hotkeys-test";
        let tokens = TokenSet {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 123,
            scope: vec!["clips:edit".into()],
            user_id: "u1".into(),
        };
        save_tokens(service, &tokens).unwrap();
        let loaded = load_tokens(service).unwrap().unwrap();
        assert_eq!(loaded, tokens);
        clear_tokens(service).unwrap();
        assert!(load_tokens(service).unwrap().is_none());
    }
}
