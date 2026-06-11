const KEYRING_SERVICE: &str = "YuriRewrite";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiKeyStorage {
    System,
    DatabaseFallback,
    None,
}

impl ApiKeyStorage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::DatabaseFallback => "database_fallback",
            Self::None => "none",
        }
    }
}

pub(crate) fn classify_api_key_storage(
    system_has_key: bool,
    database_has_key: bool,
) -> ApiKeyStorage {
    if system_has_key {
        ApiKeyStorage::System
    } else if database_has_key {
        ApiKeyStorage::DatabaseFallback
    } else {
        ApiKeyStorage::None
    }
}

pub(crate) fn read_api_key(profile_id: &str) -> Result<String, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, profile_id)?.get_password()
}

pub(crate) fn write_api_key(profile_id: &str, api_key: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, profile_id)?.set_password(api_key)
}

pub(crate) fn delete_api_key_if_present(profile_id: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, profile_id)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_storage_takes_precedence_over_database_fallback() {
        assert_eq!(classify_api_key_storage(true, true), ApiKeyStorage::System);
        assert_eq!(
            classify_api_key_storage(false, true),
            ApiKeyStorage::DatabaseFallback
        );
        assert_eq!(classify_api_key_storage(false, false), ApiKeyStorage::None);
    }
}
