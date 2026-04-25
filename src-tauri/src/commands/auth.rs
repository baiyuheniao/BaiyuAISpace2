// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Missing credentials")]
    MissingCredentials,
}

impl Serialize for AuthError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BaiduTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub session_key: Option<String>,
    pub session_secret: Option<String>,
}

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
