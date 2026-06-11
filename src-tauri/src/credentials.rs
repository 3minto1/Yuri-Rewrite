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

pub(crate) fn write_api_key(profile_id: &str, api_key: &str) -> Result<(), String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, profile_id).map_err(|error| error.to_string())?;
    entry
        .set_password(api_key)
        .map_err(|error| error.to_string())?;
    let stored = entry.get_password().map_err(|error| error.to_string());
    if let Err(error) = verify_written_api_key(api_key, stored) {
        let _ = entry.delete_credential();
        return Err(error);
    }
    Ok(())
}

fn verify_written_api_key(expected: &str, stored: Result<String, String>) -> Result<(), String> {
    let stored = stored.map_err(|error| format!("系统凭据写入后无法读取：{error}"))?;
    if stored == expected {
        Ok(())
    } else {
        Err("系统凭据写入后校验不一致".to_string())
    }
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

    #[test]
    fn api_key_write_must_be_readable_and_identical() {
        assert!(verify_written_api_key("secret", Ok("secret".to_string())).is_ok());
        assert!(verify_written_api_key("secret", Ok("other".to_string())).is_err());
        assert!(verify_written_api_key("secret", Err("unavailable".to_string())).is_err());
    }
}
