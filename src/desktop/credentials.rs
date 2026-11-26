use super::ai::AiProvider;
use crate::core::{AppError, Result};

pub fn get(provider: AiProvider) -> Result<Option<String>> {
    match entry(provider)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => match std::env::var(provider.env_var()) {
            Ok(secret) if !secret.trim().is_empty() => Ok(Some(secret)),
            _ => Ok(None),
        },
        Err(error) => Err(AppError::Other(format!(
            "Cannot read the system keyring: {error}"
        ))),
    }
}

pub fn set(provider: AiProvider, secret: &str) -> Result<()> {
    let secret = secret.trim();
    if secret.is_empty() || secret.len() > 16 * 1024 || secret.contains(['\n', '\r']) {
        return Err(AppError::Other("Invalid API key format".into()));
    }
    entry(provider)?.set_password(secret).map_err(|error| {
        AppError::Other(format!(
            "Cannot store the API key in the system keyring: {error}"
        ))
    })
}

pub fn delete(provider: AiProvider) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(AppError::Other(format!(
            "Cannot delete the API key from the system keyring: {error}"
        ))),
    }
}

fn entry(provider: AiProvider) -> Result<keyring::Entry> {
    keyring::Entry::new("appchecker", provider.env_var())
        .map_err(|error| AppError::Other(format!("Cannot initialize the system keyring: {error}")))
}
