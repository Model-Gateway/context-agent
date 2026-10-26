//! The OS credential store: the port of `KeychainStore` in `Environment.swift`.
//! API keys live under one service name; the account is the environment
//! variable name (`OPENAI_API_KEY`). The service name is the macOS app's, so
//! both apps share the same Keychain items there; on Windows the entries go
//! to Credential Manager and on Linux to the Secret Service.
//!
//! Every call is blocking (the store may prompt), so the commands run them on
//! a blocking thread. Errors are mapped to readable strings; a missing entry
//! is `Ok(None)` for `get` and not an error for `remove`.

use keyring::{Entry, Error};

/// `KeychainStore.service`.
pub const SERVICE: &str = "dev.contextagent.desktop";

/// `KeychainStore.get`: the stored value, or `None` when there is none.
pub fn get(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(describe(error)),
    }
}

/// `KeychainStore.isSet`.
pub fn is_set(account: &str) -> bool {
    matches!(get(account), Ok(Some(_)))
}

/// `KeychainStore.set`: adds or replaces the value.
pub fn set(account: &str, value: &str) -> Result<(), String> {
    entry(account)?.set_password(value).map_err(describe)
}

/// `KeychainStore.remove`: a missing item is not an error.
pub fn remove(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(describe(error)),
    }
}

fn entry(account: &str) -> Result<Entry, String> {
    let account = account.trim();
    if account.is_empty() {
        return Err("Credential store: the account name is empty.".to_owned());
    }
    Entry::new(SERVICE, account).map_err(describe)
}

/// `KeychainStore.Failure.errorDescription`.
fn describe(error: Error) -> String {
    match error {
        Error::NoStorageAccess(inner) => format!("Credential store unavailable: {inner}"),
        Error::PlatformFailure(inner) => format!("Credential store error: {inner}"),
        Error::BadEncoding(_) => "Credential store: the stored value is not UTF-8 text.".to_owned(),
        Error::TooLong(name, limit) => format!("Credential store: {name} is longer than {limit} bytes."),
        Error::Invalid(name, reason) => format!("Credential store: {name} is invalid ({reason})."),
        Error::Ambiguous(_) => "Credential store: more than one entry matches this key.".to_owned(),
        Error::NoEntry => "Credential store: no such entry.".to_owned(),
        other => format!("Credential store error: {other}"),
    }
}

async fn blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("Credential store task failed: {error}"))?
}

/// `secret_get({ account })` → the value, or `null`.
#[tauri::command]
pub async fn secret_get(account: String) -> Result<Option<String>, String> {
    blocking(move || get(&account)).await
}

/// `secret_set({ account, value })`.
#[tauri::command]
pub async fn secret_set(account: String, value: String) -> Result<(), String> {
    blocking(move || set(&account, &value)).await
}

/// `secret_remove({ account })`.
#[tauri::command]
pub async fn secret_remove(account: String) -> Result<(), String> {
    blocking(move || remove(&account)).await
}
