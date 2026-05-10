// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 认证模块
 * 
 * 功能说明:
 * - 百度 OAuth 2.0 认证 (获取 access_token)
 * - 处理 API 认证错误
 */

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 认证错误类型
#[derive(Error, Debug)]
pub enum AuthError {
    /// HTTP 请求错误
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    /// API 返回错误
    #[error("API error: {0}")]
    ApiError(String),
    /// 缺少凭证
    #[error("Missing credentials")]
    MissingCredentials,
}

/// 实现 Serialize trait 用于 Tauri 命令返回
impl Serialize for AuthError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// 百度 Token 响应结构
#[derive(Debug, Serialize, Deserialize)]
pub struct BaiduTokenResponse {
    /// 访问令牌
    pub access_token: String,
    /// 过期时间 (秒)
    pub expires_in: i64,
    /// 刷新令牌
    pub refresh_token: Option<String>,
    /// 权限范围
    pub scope: Option<String>,
    /// 会话密钥
    pub session_key: Option<String>,
    /// 会话密钥密码
    pub session_secret: Option<String>,
}

/**
 * 获取百度 OAuth 2.0 Access Token
 * 
 * 使用说明:
 * 1. 在百度 AI Studio 创建应用获取 client_id 和 client_secret
 * 2. 调用此函数获取 access_token
 * 3. access_token 有效期为 30 天
 * 
 * @param client_id: 百度应用 API Key
 * @param client_secret: 百度应用 Secret Key
 * @return BaiduTokenResponse 包含 access_token 等信息
 */
#[tauri::command]
pub async fn get_baidu_access_token(
    client_id: String,
    client_secret: String,
) -> Result<BaiduTokenResponse, AuthError> {
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(AuthError::MissingCredentials);
    }

    let url = "https://aip.baidubce.com/oauth/2.0/token";
    
    let client = reqwest::Client::new();
    
    let params = [
        ("grant_type", "client_credentials"),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
    ];
    
    let response = client
        .post(url)
        .form(&params)
        .send()
        .await
        .map_err(AuthError::RequestError)?;
    
    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(AuthError::ApiError(error_text));
    }
    
    let token_response: BaiduTokenResponse = response
        .json()
        .await
        .map_err(AuthError::RequestError)?;
    
    Ok(token_response)
}
