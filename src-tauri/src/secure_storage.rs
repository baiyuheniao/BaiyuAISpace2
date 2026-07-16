// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 安全存储模块
 * 
 * 功能说明:
 * - 使用系统密钥链 (Keyring) 安全存储 API 密钥
 * - 支持保存、获取、删除 API 密钥
 * - 支持检查密钥是否存在
 * 
 * 使用方式:
 * - Windows: 使用 Windows Credential Manager
 * - macOS: 使用 Keychain
 * - Linux: 使用 libsecret
 */

// 引入依赖
use keyring::Entry;
use serde::Serialize;
use thiserror::Error;

/// 应用名称 (用于密钥链标识)
const APP_NAME: &str = "BaiyuAISpace";
/// 服务名称 (用于密钥链标识)
const SERVICE_NAME: &str = "api_keys";

/// 安全存储错误类型
#[derive(Error, Debug)]
pub enum SecureStorageError {
    /// 密钥链错误
    #[error("Keyring error: {0}")]
    KeyringError(String),
    /// 序列化错误
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// 实现 Serialize trait 用于 Tauri 命令返回
impl Serialize for SecureStorageError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/**
 * 保存 API 密钥到系统密钥链
 * 
 * @param provider: 提供商标识符 (如 openai, anthropic)
 * @param api_key: API 密钥
 */
#[tauri::command]
pub fn save_api_key(provider: String, api_key: String) -> Result<(), SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    entry.set_password(&api_key)
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    log::info!("API key saved for provider: {}", provider);
    Ok(())
}

/// 从系统密钥链获取 API 密钥
/// 
/// # 参数
/// * `provider` - 提供商标识符 (如 openai, anthropic)
/// 
/// # 返回
/// 找到则返回 `Some(api_key)`，未找到则返回 `None`
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

/// 从系统密钥链删除 API 密钥
/// 
/// # 参数
/// * `provider` - 提供商标识符
#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), SecureStorageError> {
    let entry = Entry::new(APP_NAME, &format!("{}_{}", SERVICE_NAME, provider))
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    entry.delete_credential()
        .map_err(|e| SecureStorageError::KeyringError(e.to_string()))?;
    
    log::info!("API key deleted for provider: {}", provider);
    Ok(())
}
