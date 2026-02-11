// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use thiserror::Error;

// Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: i64,
    pub updated_at: i64,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
    pub provider: String,
    pub model: String,
}

// Stream event for frontend
#[derive(Clone, Serialize)]
pub struct StreamChunk {
    pub session_id: String,
    pub message_id: String,
    pub content: String,
    pub done: bool,
}

// Errors
#[derive(Error, Debug)]
pub enum LLMError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Invalid provider: {0}")]
    InvalidProvider(String),
    #[error("Missing API key")]
    MissingApiKey,
    #[error("Stream error: {0}")]
    StreamError(String),
}

impl Serialize for LLMError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

// Provider configurations: (id, base_url, auth_header_type)
const PROVIDER_CONFIGS: &[(&str, &str, &str)] = &[
    ("openai", "https://api.openai.com/v1/chat/completions", "bearer"),
    ("anthropic", "https://api.anthropic.com/v1/messages", "x_api_key"),
    ("google", "https://generativelanguage.googleapis.com/v1beta/models/", "bearer"),
    ("azure", "", "bearer"),
    ("mistral", "https://api.mistral.ai/v1/chat/completions", "bearer"),
    ("moonshot", "https://api.moonshot.cn/v1/chat/completions", "bearer"),
    ("zhipu", "https://open.bigmodel.cn/api/paas/v4/chat/completions", "bearer"),
    ("aliyun", "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions", "bearer"),
    ("baidu", "https://qianfan.baidubce.com/v2/chat/completions", "bearer"),
    ("doubao", "https://ark.cn-beijing.volces.com/api/v3/chat/completions", "bearer"),
    ("deepseek", "https://api.deepseek.com/v1/chat/completions", "bearer"),
    ("siliconflow", "https://api.siliconflow.cn/v1/chat/completions", "bearer"),
    ("minimax", "https://api.minimax.chat/v1/text/chatcompletion_v2", "bearer"),
    ("yi", "https://api.lingyiwanwu.com/v1/chat/completions", "bearer"),
    ("custom", "", "bearer"),
];

fn build_url(provider: &str, base_url: &str, model: &str) -> String {
    match provider {
        "google" => {
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
                model
            )
        }
        "azure" => base_url.to_string(),
        "custom" => format!("{}/chat/completions", base_url.trim_end_matches('/')),
        _ => {
            if let Some((url, _)) = PROVIDER_CONFIGS.iter().find(|(p, _, _)| *p == provider) {
                url.to_string()
            } else {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        }
    }
}

fn build_stream_request_body(provider: &str, model: &str, messages: &[ChatMessage]) -> serde_json::Value {
    match provider {
        "anthropic" => {
            let system_msg = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());
            
            let msgs: Vec<_> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "assistant" } else { "user" },
                        "content": m.content
                    })
                })
                .collect();

            let mut body = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": 4096,
                "stream": true,
            });

            if let Some(sys) = system_msg {
                body["system"] = serde_json::json!(sys);
            }

            body
        }
        "google" => {
            let contents: Vec<_> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "model" } else { "user" },
                        "parts": [{ "text": m.content }]
                    })
                })
                .collect();

            serde_json::json!({
                "contents": contents,
                "generationConfig": {
                    "temperature": 0.7,
                    "maxOutputTokens": 4096,
                }
            })
        }
        _ => {
            let msgs: Vec<_> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content": m.content
                    })
                })
                .collect();

            serde_json::json!({
                "model": model,
                "messages": msgs,
                "temperature": 0.7,
                "stream": true,
            })
        }
    }
}

fn build_headers(provider: &str, api_key: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        "text/event-stream".parse().unwrap(),
    );

    if provider == "anthropic" {
        headers.insert("x-api-key", api_key.parse().unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    } else {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key).parse().unwrap(),
        );
    }

    headers
}

// Parse SSE line and extract content
fn parse_sse_line(provider: &str, line: &str) -> Option<String> {
    if !line.starts_with("data: ") {
        return None;
    }

    let data = &line[6..];
    
    if data == "[DONE]" {
        return Some(String::new());
    }

    let json: serde_json::Value = serde_json::from_str(data).ok()?;

    match provider {
        "anthropic" => {
            // Anthropic format: delta.text
            json["delta"]["text"].as_str().map(|s| s.to_string())
        }
        _ => {
            // OpenAI format: choices[0].delta.content
            json["choices"][0]["delta"]["content"]
                .as_str()
                .map(|s| s.to_string())
        }
    }
}

// Stream message command
#[tauri::command]
pub async fn stream_message(
    request: SendMessageRequest,
    app_handle: AppHandle,
) -> Result<(), LLMError> {
    let api_key = get_api_key(&request.provider).await?;
    let message_id = uuid::Uuid::new_v4().to_string();
    
    let url = build_url(&request.provider, "", &request.model);
    let client = reqwest::Client::new();
    let body = build_stream_request_body(&request.provider, &request.model, &request.messages);
    let headers = build_headers(&request.provider, &api_key);

    log::info!("Starting stream to {}", url);

    let response = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        log::error!("API error: {}", error_text);
        return Err(LLMError::ApiError(error_text));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| LLMError::StreamError(e.to_string()))?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        // Process complete lines
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(content) = parse_sse_line(&request.provider, &line) {
                if content.is_empty() {
                    // Stream done
                    let _ = app_handle.emit("stream-chunk", StreamChunk {
                        session_id: request.session_id.clone(),
                        message_id: message_id.clone(),
                        content: String::new(),
                        done: true,
                    });
                } else {
                    let _ = app_handle.emit("stream-chunk", StreamChunk {
                        session_id: request.session_id.clone(),
                        message_id: message_id.clone(),
                        content,
                        done: false,
                    });
                }
            }
        }
    }

    // Emit final done event
    let _ = app_handle.emit("stream-chunk", StreamChunk {
        session_id: request.session_id,
        message_id,
        content: String::new(),
        done: true,
    });

    Ok(())
}

#[tauri::command]
pub async fn get_chat_sessions() -> Result<Vec<ChatSession>, LLMError> {
    Ok(vec![])
}

#[tauri::command]
pub async fn delete_chat_session(session_id: String) -> Result<(), LLMError> {
    log::info!("Deleting session: {}", session_id);
    Ok(())
}

async fn get_api_key(_provider: &str) -> Result<String, LLMError> {
    let env_var = format!("{}_API_KEY", _provider.to_uppercase());
    if let Ok(key) = std::env::var(&env_var) {
        return Ok(key);
    }
    Err(LLMError::MissingApiKey)
}
