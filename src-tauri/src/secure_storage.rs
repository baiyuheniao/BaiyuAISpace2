// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const APP_NAME: &str = "BaiyuAISpace";
const SERVICE_NAME: &str = "api_keys";

#[derive(Error, Debug)]
pub enum SecureStorageError {
    #[error("Keyring error: {0}")]
    KeyringError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl Serialize for SecureStorageError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// Save API key to system keyring
#[tauri::command]
pub fn save_api_key(provider: String, api_key: String) -> Result<(), SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    entry.set_password(&api_key)
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    log::info!("API key saved for provider: {}", provider);
    Ok(())
}

/// Get API key from system keyring
#[tauri::command]
pub fn get_api_key(provider: String) -> Result<Option<String>, SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    match entry.get_password() {
        Ok(key) => {
            log::info!("API key retrieved for provider: {}", provider);
            Ok(Some(key))
        }
        Err(keyring::Error::NoEntry) => {
            log::info!("No API key found for provider: {}", provider);
            Ok(None)
        }
        Err(e) => Err(SecureStorageError::KeyringError(e.to_string())),
    }
}

/// Delete API key from system keyring
#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    entry.delete_credential()
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    log::info!("API key deleted for provider: {}", provider);
    Ok(())
}

/// Check if API key exists
#[tauri::command]
pub fn has_api_key(provider: String) -> Result<bool, SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(SecureStorageError::KeyringError(e.to_string())),
    }
}
