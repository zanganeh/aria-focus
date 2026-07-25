use keyring::Entry;

const SERVICE: &str = "com.ariafocus.desktop";
const USERNAME: &str = "openrouter-api-key";

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("the operating-system credential store is unavailable")]
    Unavailable,
    #[error("the operating-system credential store rejected the operation")]
    Operation,
}

fn entry() -> Result<Entry, SecretStoreError> {
    Entry::new(SERVICE, USERNAME).map_err(|_| SecretStoreError::Unavailable)
}

pub fn save(api_key: &str) -> Result<(), SecretStoreError> {
    if api_key.trim().len() < 16 || api_key.contains('\n') || api_key.contains('\r') {
        return Err(SecretStoreError::Operation);
    }
    entry()?
        .set_password(api_key.trim())
        .map_err(|_| SecretStoreError::Operation)
}

pub fn load() -> Result<Option<String>, SecretStoreError> {
    match entry()?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(SecretStoreError::Operation),
    }
}

pub fn remove() -> Result<(), SecretStoreError> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecretStoreError::Operation),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_validation_rejects_short_or_multiline_values() {
        assert!(super::save("short").is_err());
        assert!(super::save("valid-looking-key-123\nleak").is_err());
    }
}
