// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * LLM 聊天模块
 * 
 * 功能说明:
 * - 支持多种 LLM 提供商 (OpenAI, Anthropic, Google 等)
 * - 流式响应处理 (Server-Sent Events)
 * - MCP 工具集成
 * - 会话和消息管理
 */

use crate::commands::constants::{LLM_CONNECT_TIMEOUT, LLM_REQUEST_TIMEOUT};
use crate::commands::mcp::{get_all_mcp_tools, call_mcp_tool, MCPTool};
use crate::db::DbState;
use chrono::Utc;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use uuid::Uuid;

// ============ 类型定义 ============

/// 聊天消息结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// 消息 ID
    pub id: String,
    /// 消息角色 (user/assistant/system)
    pub role: String,
    /// 消息内容
    pub content: String,
    /// 时间戳 (毫秒)
    pub timestamp: i64,
    /// 错误信息 (如果有)
    pub error: Option<String>,
}

/// 聊天会话结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    /// 会话 ID
    pub id: String,
    /// 会话标题
    pub title: String,
    /// 消息列表
    pub messages: Vec<ChatMessage>,
    /// 创建时间戳
    pub created_at: i64,
    /// 最后更新时间戳
    pub updated_at: i64,
    /// LLM 提供商
    pub provider: String,
    /// 模型名称
    pub model: String,
    /// API 配置 ID
    pub api_config_id: String,
}

/// 发送消息请求结构
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    /// 会话 ID
    pub session_id: String,
    /// 消息历史
    pub messages: Vec<ChatMessage>,
    /// LLM 提供商
    pub provider: String,
    /// 模型名称
    pub model: String,
    /// API 密钥
    pub api_key: String,
    /// API 基础 URL
    pub base_url: String,
    /// 是否启用 MCP
    pub enable_mcp: bool,
}

/// 流式响应事件结构
#[derive(Clone, Serialize)]
pub struct StreamChunk {
    /// 会话 ID
    pub session_id: String,
    /// 消息 ID
    pub message_id: String,
    /// 增量内容
    pub content: String,
    /// 是否完成
    pub done: bool,
}

// Global storage for active stream cancellation flags (temporarily disabled)
#[allow(dead_code)]
static ACTIVE_STREAMS: Lazy<Arc<Mutex<HashMap<String, bool>>>> = 
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

// Errors
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum LLMError {
    /// HTTP 请求错误
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    /// API 返回错误
    #[error("API error: {0}")]
    ApiError(String),
    /// 无效的提供商
    #[error("Invalid provider: {0}")]
    InvalidProvider(String),
    /// 缺少 API 密钥
    #[error("Missing API key")]
    MissingApiKey,
    /// 流式响应错误
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

/// LLM 提供商配置
/// 格式: (提供商标识符, API 端点, 认证头类型)
/// 
/// - bearer: 使用 Authorization: Bearer token
/// - x_api_key: 使用 x-api-key 头
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
    ("local", "", "none"),
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
        "azure" => {
            let base = base_url.trim_end_matches('/');
            if base.is_empty() {
                "".to_string()
            } else {
                let base = if !model.is_empty() && !base.ends_with(model) {
                    format!("{}/{}", base, model)
                } else {
                    base.to_string()
                };
                format!("{}/chat/completions?api-version=2023-05-15", base)
            }
        }
        "custom" => format!("{}/chat/completions", base_url.trim_end_matches('/')),
        "local" => format!("{}/chat/completions", base_url.trim_end_matches('/')),
        _ => {
            if let Some((_, url, _)) = PROVIDER_CONFIGS.iter().find(|(p, _, _)| *p == provider) {
                url.to_string()
            } else {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        }
    }
}

fn build_stream_request_body(provider: &str, model: &str, messages: &[ChatMessage], tools: &[MCPTool]) -> serde_json::Value {
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

            let mut body = serde_json::json!({
                "model": model,
                "messages": msgs,
                "stream": true,
            });

            // Add tools if available
            if !tools.is_empty() {
                let tools_json: Vec<_> = tools
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema
                            }
                        })
                    })
                    .collect();
                body["tools"] = serde_json::json!(tools_json);
            }

            body
        }
    }
}

fn create_http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(LLM_REQUEST_TIMEOUT)
        .connect_timeout(LLM_CONNECT_TIMEOUT)
        .build()
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

    match provider {
        "google" => {
            headers.insert("x-goog-api-key", api_key.parse().unwrap());
        }
        "azure" => {
            headers.insert("api-key", api_key.parse().unwrap());
        }
        "anthropic" => {
            headers.insert("x-api-key", api_key.parse().unwrap());
            headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        }
        "local" => {
            // Local models (e.g. Ollama) don't require authentication
            // No Authorization header needed
        }
        _ => {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", api_key).parse().unwrap(),
            );
        }
    }

    headers
}

// Mask a secret, showing only last N characters
fn mask_secret(s: &str, show_last: usize) -> String {
    if s.len() <= show_last {
        "****".to_string()
    } else {
        let keep = &s[s.len() - show_last..];
        format!("{}{}", "*".repeat(s.len() - show_last), keep)
    }
}

fn mask_auth_header_value(value: &str) -> String {
    if value.starts_with("Bearer ") {
        let key = &value[7..];
        format!("Bearer {}", mask_secret(key, 4))
    } else {
        mask_secret(value, 4)
    }
}

// Parse SSE line and extract content or tool calls
fn parse_sse_line(provider: &str, line: &str) -> Option<StreamContent> {
    if !line.starts_with("data: ") {
        return None;
    }

    let data = &line[6..];
    
    if data == "[DONE]" {
        return Some(StreamContent::Done);
    }

    let json: serde_json::Value = serde_json::from_str(data).ok()?;

    match provider {
        "google" => {
            // Google Gemini format: candidates[0].content.parts[0].text
            json.get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|parts| parts.as_array())
                .and_then(|arr| arr.first())
                .and_then(|part| part.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| StreamContent::Text(s.to_string()))
        }
        "anthropic" => {
            // Anthropic format: delta.text
            json.get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| StreamContent::Text(s.to_string()))
        }
        _ => {
            // OpenAI format
            if let Some(choices) = json["choices"].as_array() {
                if let Some(first_choice) = choices.first() {
                    if let Some(content) = first_choice["delta"]["content"].as_str() {
                        return Some(StreamContent::Text(content.to_string()));
                    } else if let Some(tool_calls) = first_choice["delta"]["tool_calls"].as_array() {
                        // Handle tool calls
                        let calls: Vec<_> = tool_calls.iter().filter_map(|call| {
                            if let (Some(id), Some(func)) = (
                                call["id"].as_str(),
                                call["function"].as_object()
                            ) {
                                Some(ToolCall {
                                    _id: id.to_string(),
                                    function: ToolFunction {
                                        name: func["name"].as_str()?.to_string(),
                                        arguments: func["arguments"].as_str()?.to_string(),
                                    }
                                })
                            } else {
                                None
                            }
                        }).collect();
                        
                        if !calls.is_empty() {
                            return Some(StreamContent::ToolCalls(calls));
                        }
                    }
                }
            }
            None
        }
    }
}

#[derive(Debug)]
enum StreamContent {
    Text(String),
    ToolCalls(Vec<ToolCall>),
    Done,
}

#[derive(Debug)]
struct ToolCall {
    _id: String,
    function: ToolFunction,
}

#[derive(Debug)]
struct ToolFunction {
    name: String,
    arguments: String,
}

// Stream message command
#[tauri::command]
pub async fn stream_message(
    request: SendMessageRequest,
    state: tauri::State<'_, DbState>,
    app_handle: AppHandle,
) -> Result<(), LLMError> {
    log::info!("[stream_message] Called - session_id: {}, message_count: {}, enable_mcp: {}", 
        request.session_id, request.messages.len(), request.enable_mcp);
    
    let api_key = get_api_key(&request)?;
    let message_id = Uuid::new_v4().to_string();
    let _session_id = request.session_id.clone();
    
    // TODO: Re-enable stream cancellation with proper implementation
    // Create cancellation token and store it
    // let cancel_token = CancellationToken::new();
    // {
    //     let mut streams = ACTIVE_STREAMS.lock().await;
    //     streams.insert(session_id.clone(), true);
    // }
    
    // Cleanup token when function exits
    // let _cleanup = scopeguard::guard(session_id.clone(), |sid| {
    //     tauri::async_runtime::spawn(async move {
    //         let mut streams = ACTIVE_STREAMS.lock().await;
    //         streams.remove(&sid);
    //     });
    // });
    
    // Get MCP tools if enabled
    let mcp_tools = if request.enable_mcp {
        match get_all_mcp_tools(state.clone()).await {
            Ok(tools) => tools,
            Err(e) => {
                log::warn!("Failed to get MCP tools: {}", e);
                vec![]
            }
        }
    } else {
        vec![]
    };
    
    let url = build_url(&request.provider, &request.base_url, &request.model);
    // Log provider/base/model for debugging (do not log API key)
    log::debug!(
        "LLM request details: provider={} base_url='{}' model='{}'",
        request.provider,
        request.base_url,
        request.model
    );

    if url.trim().is_empty() {
        log::error!(
            "Invalid URL constructed for provider={} base_url='{}' model='{}'",
            request.provider,
            request.base_url,
            request.model
        );
        return Err(LLMError::ApiError("Invalid target URL".to_string()));
    }

    let client = create_http_client()?;
    let body = build_stream_request_body(&request.provider, &request.model, &request.messages, &mcp_tools);
    let headers = build_headers(&request.provider, &api_key);

    log::debug!("Constructed URL for provider {}: {}", request.provider, url);

    let masked_auth = if let Some(h) = headers.get(reqwest::header::AUTHORIZATION) {
        match h.to_str() {
            Ok(s) => mask_auth_header_value(s),
            Err(_) => "<non-utf8>".to_string(),
        }
    } else if let Some(h) = headers.get("x-api-key") {
        match h.to_str() {
            Ok(s) => mask_auth_header_value(s),
            Err(_) => "<non-utf8>".to_string(),
        }
    } else {
        "<none>".to_string()
    };

    log::debug!("Auth header (masked): {}", masked_auth);

    let response = match client
        .post(&url)
        .headers(headers.clone())
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("reqwest send error for url '{}': {:?}", url, e);
            return Err(e.into());
        }
    };

    if !response.status().is_success() {
        let error_text = response.text().await?;
        log::error!("API error: {}", error_text);
        return Err(LLMError::ApiError(error_text));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut accumulated_tool_calls = Vec::new();

    // Main loop (cancellation temporarily disabled)
    loop {
        tokio::select! {
            // Check for cancellation signal
            // _ = cancel_token.cancelled() => {
            //     log::info!("Stream cancelled for session: {}", session_id);
            //     let _ = app_handle.emit("stream-chunk", StreamChunk {
            //         session_id: request.session_id.clone(),
            //         message_id: message_id.clone(),
            //         content: String::new(),
            //         done: true,
            //     });
            //     return Ok(());
            // }
            // Read next chunk from stream
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(chunk)) => {
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
                                match content {
                                    StreamContent::Text(text) => {
                                        let _ = app_handle.emit("stream-chunk", StreamChunk {
                                            session_id: request.session_id.clone(),
                                            message_id: message_id.clone(),
                                            content: text,
                                            done: false,
                                        });
                                    }
                                    StreamContent::ToolCalls(calls) => {
                                        accumulated_tool_calls.extend(calls);
                                    }
                                    StreamContent::Done => {
                                        // Process accumulated tool calls if any
                                        if !accumulated_tool_calls.is_empty() && request.enable_mcp {
                                            let tool_calls = std::mem::take(&mut accumulated_tool_calls);
                                            for tool_call in tool_calls {
                                                // Find the tool and execute it
                                                if let Some(tool) = mcp_tools.iter().find(|t| t.name == tool_call.function.name) {
                                                    log::info!("Executing MCP tool: {}", tool.name);
                                                    
                                                    match call_mcp_tool(
                                                        state.clone(),
                                                        Some(tool.server_id.clone()),
                                                        tool.name.clone(),
                                                        serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null),
                                                    ).await {
                                                        Ok(result) => {
                                                            log::info!("Tool execution result: {:?}", result);

                                                            // Send tool result back to the LLM for continued reasoning
                                                            let tool_result_content = format!(
                                                                "工具 {} 调用结果：{}",
                                                                tool.name,
                                                                serde_json::to_string(&result).unwrap_or_else(|_| "<serialize error>".to_string())
                                                            );

                                                            let mut follow_up_messages = request.messages.clone();
                                                            follow_up_messages.push(ChatMessage {
                                                                id: Uuid::new_v4().to_string(),
                                                                role: "assistant".to_string(),
                                                                content: tool_result_content.clone(),
                                                                timestamp: Utc::now().timestamp_millis(),
                                                                error: None,
                                                            });

                                                            match request_llm_once(
                                                                &request.provider,
                                                                &request.model,
                                                                &request.api_key,
                                                                &request.base_url,
                                                                &follow_up_messages,
                                                                &mcp_tools,
                                                            )
                                                            .await
                                                            {
                                                                Ok(live_reply) => {
                                                                    let _ = app_handle.emit("stream-chunk", StreamChunk {
                                                                        session_id: request.session_id.clone(),
                                                                        message_id: message_id.clone(),
                                                                        content: live_reply,
                                                                        done: false,
                                                                    });
                                                                }
                                                                Err(err) => {
                                                                    log::error!("Failed to continue reasoning after tool call: {}", err);
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            log::error!("Tool execution failed: {}", e);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        
                                        let _ = app_handle.emit("stream-chunk", StreamChunk {
                                            session_id: request.session_id.clone(),
                                            message_id: message_id.clone(),
                                            content: String::new(),
                                            done: true,
                                        });
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Err(LLMError::StreamError(e.to_string()));
                    }
                    None => {
                        // Stream ended naturally
                        let _ = app_handle.emit("stream-chunk", StreamChunk {
                            session_id: request.session_id.clone(),
                            message_id: message_id.clone(),
                            content: String::new(),
                            done: true,
                        });
                        return Ok(());
                    }
                }
            }
        }
    }
}

async fn request_llm_once(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    messages: &[ChatMessage],
    tools: &[MCPTool],
) -> Result<String, LLMError> {
    let url = build_url(provider, base_url, model);
    let client = create_http_client()?;
    let mut body = build_stream_request_body(provider, model, messages, tools);
    body["stream"] = serde_json::json!(false);

    let headers = build_headers(provider, api_key);

    log::debug!("Constructed URL for provider {} (one-shot): {}", provider, url);

    let masked_auth_one_shot = if let Some(h) = headers.get(reqwest::header::AUTHORIZATION) {
        match h.to_str() {
            Ok(s) => mask_auth_header_value(s),
            Err(_) => "<non-utf8>".to_string(),
        }
    } else if let Some(h) = headers.get("x-api-key") {
        match h.to_str() {
            Ok(s) => mask_auth_header_value(s),
            Err(_) => "<non-utf8>".to_string(),
        }
    } else {
        "<none>".to_string()
    };

    log::debug!("One-shot auth header (masked): {}", masked_auth_one_shot);

    let response = match client
        .post(&url)
        .headers(headers.clone())
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("reqwest send error (one-shot) for url '{}': {:?}", url, e);
            return Err(e.into());
        }
    };

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "unknown".to_string());
        return Err(LLMError::ApiError(error_text));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(LLMError::RequestError)?;

    if provider == "google" {
        // Google Gemini format: candidates[0].content.parts[0].text
        if let Some(text) = json
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(|parts| parts.as_array())
            .and_then(|arr| arr.first())
            .and_then(|part| part.get("text"))
            .and_then(|t| t.as_str())
        {
            return Ok(text.to_string());
        }
    } else if provider == "anthropic" {
        if let Some(content_array) = json["content"].as_array() {
            if let Some(first_content) = content_array.first() {
                if let Some(text) = first_content["text"].as_str() {
                    return Ok(text.to_string());
                }
            }
        }
    } else {
        if let Some(choices) = json["choices"].as_array() {
            if let Some(first_choice) = choices.first() {
                if let Some(text) = first_choice["message"]["content"].as_str() {
                    return Ok(text.to_string());
                }
                if let Some(text) = first_choice["text"].as_str() {
                    return Ok(text.to_string());
                }
            }
        }
    }

    Err(LLMError::ApiError("LLM did not return content".to_string()))
}

#[allow(dead_code)]
#[tauri::command]
pub async fn get_chat_sessions() -> Result<Vec<ChatSession>, LLMError> {
    Ok(vec![])
}

#[allow(dead_code)]
#[tauri::command]
pub async fn delete_chat_session(session_id: String) -> Result<(), LLMError> {
    log::info!("Deleting session: {}", session_id);
    Ok(())
}

fn get_api_key(request: &SendMessageRequest) -> Result<String, LLMError> {
    // Local models don't require API keys
    if request.provider == "local" {
        return Ok(String::new());
    }
    if request.api_key.is_empty() {
        return Err(LLMError::MissingApiKey);
    }
    Ok(request.api_key.clone())
}

/// Cancel an active stream for a session
#[tauri::command]
pub async fn cancel_stream(session_id: String) -> Result<(), String> {
    // Temporarily disabled - needs proper implementation
    // let mut streams = ACTIVE_STREAMS.lock().await;
    // if let Some(flag) = streams.get(&session_id) {
    //     *flag = true; // Set cancellation flag
    //     log::info!("Cancelled stream for session: {}", session_id);
    //     Ok(())
    // } else {
    //     Err("No active stream found for session".to_string())
    // }
    log::info!("Stream cancellation requested for session: {} (temporarily disabled)", session_id);
    Ok(())
}
