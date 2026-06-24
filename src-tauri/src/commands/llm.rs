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
use crate::commands::skills::{read_skill_resource_text, Skill};
use crate::db::DbState;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use thiserror::Error;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
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
    /// 手动激活的 Skill ID 列表
    #[serde(default)]
    pub active_skill_ids: Vec<String>,
    /// 是否允许模型自主判断调用其它已启用的 Skill
    #[serde(default)]
    pub enable_skill_autonomy: bool,
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

// One cancellation token per in-flight stream, keyed by session_id, so
// `cancel_stream` can signal `stream_message`'s read loop to stop early.
static ACTIVE_STREAMS: Lazy<Arc<Mutex<HashMap<String, CancellationToken>>>> =
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
    ("minimax", "https://api.minimax.io/v1/text/chatcompletion_v2", "bearer"),
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
            // Convention used by this app (see settings.ts default placeholder
            // "https://your-resource.openai.azure.com/openai/deployments/"):
            // the user-supplied base_url already includes the
            // `/openai/deployments/` segment, so we only need to append the
            // deployment name (`model`) + `/chat/completions`. This matches
            // the real Azure OpenAI REST path
            // {endpoint}/openai/deployments/{deployment}/chat/completions?api-version=...
            let base = base_url.trim_end_matches('/');
            if base.is_empty() {
                "".to_string()
            } else {
                let base = if !model.is_empty() && !base.ends_with(model) {
                    format!("{}/{}", base, model)
                } else {
                    base.to_string()
                };
                format!("{}/chat/completions?api-version=2024-06-01", base)
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
            let system_msg = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());

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

            let mut body = serde_json::json!({
                "contents": contents,
                "generationConfig": {
                    "maxOutputTokens": 4096,
                }
            });

            // Gemini ignores a system-role entry inside `contents` -- the system
            // prompt must go in the separate top-level `systemInstruction` field.
            if let Some(sys) = system_msg {
                body["systemInstruction"] = serde_json::json!({
                    "parts": [{ "text": sys }]
                });
            }

            body
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

/// Build the combined instructions + readable-resource-file text for one or
/// more activated skills, ready to be merged into a system prompt.
async fn build_skill_context(skills: &[Skill], app_handle: &AppHandle) -> String {
    let mut parts = Vec::new();
    for skill in skills {
        let mut section = format!("# Skill: {}\n{}", skill.name, skill.instructions);
        for filename in &skill.resource_files {
            if let Some(content) = read_skill_resource_text(app_handle, &skill.id, filename).await {
                section.push_str(&format!("\n\n## 附带资源文件: {}\n{}", filename, content));
            }
        }
        parts.push(section);
    }
    parts.join("\n\n---\n\n")
}

/// Append one synthetic tool definition per skill the model may autonomously
/// invoke. The tool only carries name + description -- invoking it returns
/// the skill's instructions as the result (see the `skill__` handling in
/// `stream_message`'s `StreamContent::Done` branch), it never calls out to
/// anything external by itself.
///
/// Only applies to the generic OpenAI-style `tools` shape -- `anthropic` and
/// `google` use entirely different tool schemas and `build_stream_request_body`
/// never wires `tools` through for them in the first place, so adding an
/// OpenAI-shaped `tools` field directly onto their request body here would
/// just send a field their API doesn't understand.
fn append_skill_tools(body: &mut serde_json::Value, provider: &str, autonomous_skills: &[Skill]) {
    if autonomous_skills.is_empty() || matches!(provider, "anthropic" | "google") {
        return;
    }

    let skill_tools: Vec<_> = autonomous_skills
        .iter()
        .map(|skill| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": format!("skill__{}", skill.id),
                    "description": format!(
                        "调用「{}」技能：{}。如果当前任务和这个技能相关，调用它获取具体操作指南。",
                        skill.name, skill.description
                    ),
                    "parameters": { "type": "object", "properties": {} }
                }
            })
        })
        .collect();

    match body.get_mut("tools").and_then(|t| t.as_array_mut()) {
        Some(existing) => existing.extend(skill_tools),
        None => body["tools"] = serde_json::json!(skill_tools),
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
                        // OpenAI streams tool calls incrementally: the first delta for a
                        // given `index` carries `id` + `function.name`, and every
                        // subsequent delta for that same `index` carries only a fragment
                        // of `function.arguments` (id/name absent). Each delta here is a
                        // partial update, not a complete tool call -- the caller must
                        // accumulate fragments by `index` across the whole stream.
                        let deltas: Vec<_> = tool_calls.iter().filter_map(|call| {
                            let index = call["index"].as_u64()? as u32;
                            let id = call["id"].as_str().map(|s| s.to_string());
                            let name = call["function"]["name"].as_str().map(|s| s.to_string());
                            let arguments_fragment = call["function"]["arguments"].as_str().map(|s| s.to_string());
                            Some(ToolCallDelta { index, id, name, arguments_fragment })
                        }).collect();

                        if !deltas.is_empty() {
                            return Some(StreamContent::ToolCallDeltas(deltas));
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
    ToolCallDeltas(Vec<ToolCallDelta>),
    Done,
}

/// One fragment of a streamed tool call, keyed by `index`. `id`/`name` are
/// only present on the first fragment for a given index; `arguments_fragment`
/// must be concatenated across every fragment sharing that index.
#[derive(Debug)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    name: Option<String>,
    arguments_fragment: Option<String>,
}

/// A fully accumulated tool call, ready to execute.
#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    function: ToolFunction,
}

#[derive(Debug, Clone)]
struct ToolFunction {
    name: String,
    arguments: String,
}

/// Accumulator for a single tool call's fragments while the stream is still
/// in progress. `id`/`name` arrive once; `arguments` is built by
/// concatenating every fragment seen for this index, in order.
#[derive(Debug, Default)]
struct PartialToolCall {
    id: Option<String>,
    name: Option<String>,
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
    let session_id = request.session_id.clone();

    // Create a cancellation token and register it so `cancel_stream` can
    // signal this in-flight request to stop early.
    let cancel_token = CancellationToken::new();
    {
        let mut streams = ACTIVE_STREAMS.lock().await;
        streams.insert(session_id.clone(), cancel_token.clone());
    }

    // Deregister the token when this function returns, by whichever path --
    // spawned because Drop can't run the async lock acquire directly.
    let _cleanup = scopeguard::guard(session_id.clone(), |sid| {
        tauri::async_runtime::spawn(async move {
            let mut streams = ACTIVE_STREAMS.lock().await;
            streams.remove(&sid);
        });
    });
    
    // Fetch every enabled MCP server's tools up front -- needed regardless of
    // `enable_mcp` because a manually-activated Skill can bring its own bound
    // servers' tools into the conversation even when the global MCP toggle is off.
    let all_mcp_tools = match get_all_mcp_tools(state.clone()).await {
        Ok(tools) => tools,
        Err(e) => {
            log::warn!("Failed to get MCP tools: {}", e);
            vec![]
        }
    };

    // Load skills and split them into "manually activated this turn" and
    // "enabled but left for the model to decide whether to invoke".
    let all_skills = {
        let db = state.0.lock().await;
        db.get_skills().unwrap_or_else(|e| {
            log::warn!("Failed to load skills: {}", e);
            vec![]
        })
    };
    let active_skills: Vec<Skill> = all_skills
        .iter()
        .filter(|s| s.enabled && request.active_skill_ids.contains(&s.id))
        .cloned()
        .collect();
    let autonomous_skills: Vec<Skill> = if request.enable_skill_autonomy {
        all_skills
            .iter()
            .filter(|s| s.enabled && !request.active_skill_ids.contains(&s.id))
            .cloned()
            .collect()
    } else {
        vec![]
    };

    // Tools actually exposed this turn: the global MCP set (if enabled) plus
    // whatever the manually-activated skills bind, deduplicated.
    let mut mcp_tools: Vec<MCPTool> = if request.enable_mcp { all_mcp_tools.clone() } else { vec![] };
    for skill in &active_skills {
        for tool in &all_mcp_tools {
            if skill.bound_mcp_server_ids.contains(&tool.server_id)
                && !mcp_tools.iter().any(|t| t.server_id == tool.server_id && t.name == tool.name)
            {
                mcp_tools.push(tool.clone());
            }
        }
    }

    // Inject manually-activated skills' instructions (+ readable resource
    // file contents) as a system-prompt block, merged with any existing
    // system message rather than replacing it.
    let mut effective_messages = request.messages.clone();
    if !active_skills.is_empty() {
        let skill_context = build_skill_context(&active_skills, &app_handle).await;
        if !skill_context.is_empty() {
            if !effective_messages.is_empty() && effective_messages[0].role == "system" {
                effective_messages[0].content =
                    format!("{}\n\n{}", effective_messages[0].content, skill_context);
            } else {
                effective_messages.insert(0, ChatMessage {
                    id: Uuid::new_v4().to_string(),
                    role: "system".to_string(),
                    content: skill_context,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    error: None,
                });
            }
        }
    }

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
    let mut body = build_stream_request_body(&request.provider, &request.model, &effective_messages, &mcp_tools);
    append_skill_tools(&mut body, &request.provider, &autonomous_skills);
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
    let mut tool_call_acc: std::collections::BTreeMap<u32, PartialToolCall> = std::collections::BTreeMap::new();

    // Main loop
    loop {
        tokio::select! {
            // Check for cancellation signal
            _ = cancel_token.cancelled() => {
                log::info!("Stream cancelled for session: {}", session_id);
                let _ = app_handle.emit("stream-chunk", StreamChunk {
                    session_id: request.session_id.clone(),
                    message_id: message_id.clone(),
                    content: String::new(),
                    done: true,
                });
                return Ok(());
            }
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
                                    StreamContent::ToolCallDeltas(deltas) => {
                                        for delta in deltas {
                                            let entry = tool_call_acc.entry(delta.index).or_default();
                                            if let Some(id) = delta.id {
                                                entry.id = Some(id);
                                            }
                                            if let Some(name) = delta.name {
                                                entry.name = Some(name);
                                            }
                                            if let Some(fragment) = delta.arguments_fragment {
                                                entry.arguments.push_str(&fragment);
                                            }
                                        }
                                    }
                                    StreamContent::Done => {
                                        // Finalize accumulated tool call fragments (id/name from the
                                        // first chunk per index, arguments concatenated across all of
                                        // them) and execute them if any.
                                        let tool_calls: Vec<ToolCall> = std::mem::take(&mut tool_call_acc)
                                            .into_values()
                                            .filter_map(|p| {
                                                Some(ToolCall {
                                                    id: p.id?,
                                                    function: ToolFunction {
                                                        name: p.name?,
                                                        arguments: p.arguments,
                                                    },
                                                })
                                            })
                                            .collect();

                                        if !tool_calls.is_empty() {
                                            let mut tool_results = Vec::with_capacity(tool_calls.len());
                                            for tool_call in &tool_calls {
                                                if let Some(skill_id) = tool_call.function.name.strip_prefix("skill__") {
                                                    // Autonomously-invoked Skill: the "tool result" is the
                                                    // skill's own instructions/resources, not an MCP call.
                                                    if let Some(skill) = all_skills.iter().find(|s| s.id == skill_id) {
                                                        log::info!("Model invoked skill: {}", skill.name);
                                                        let content = build_skill_context(std::slice::from_ref(skill), &app_handle).await;
                                                        tool_results.push(serde_json::json!({ "skill": skill.name, "content": content }));
                                                    } else {
                                                        log::warn!("Skill not found for autonomous call: {}", skill_id);
                                                        tool_results.push(serde_json::json!({ "error": format!("skill '{}' not found", skill_id) }));
                                                    }
                                                } else if let Some(tool) = mcp_tools.iter().find(|t| t.name == tool_call.function.name) {
                                                    log::info!("Executing MCP tool: {}", tool.name);
                                                    let result = match call_mcp_tool(
                                                        state.clone(),
                                                        Some(tool.server_id.clone()),
                                                        tool.name.clone(),
                                                        serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null),
                                                    ).await {
                                                        Ok(result) => {
                                                            log::info!("Tool execution result: {:?}", result);
                                                            result
                                                        }
                                                        Err(e) => {
                                                            log::error!("Tool execution failed: {}", e);
                                                            serde_json::json!({ "error": e.to_string() })
                                                        }
                                                    };
                                                    tool_results.push(result);
                                                } else {
                                                    log::warn!("MCP tool not found: {}", tool_call.function.name);
                                                    tool_results.push(serde_json::json!({ "error": format!("tool '{}' not found", tool_call.function.name) }));
                                                }
                                            }

                                            // Continue the conversation with a single follow-up request
                                            // carrying the proper assistant.tool_calls + tool-role
                                            // messages, instead of one request per call.
                                            match continue_after_tool_calls(
                                                &request.provider,
                                                &request.model,
                                                &request.api_key,
                                                &request.base_url,
                                                &effective_messages,
                                                &tool_calls,
                                                &tool_results,
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
                                                    log::error!("Failed to continue reasoning after tool calls: {}", err);
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

/// Continue a conversation after one or more MCP tool calls have been
/// executed, sending a single follow-up request with the proper OpenAI
/// function-calling message shape: the assistant turn that requested the
/// calls (as a `tool_calls` array, not freeform text), followed by one
/// `role: "tool"` message per result carrying the matching `tool_call_id`.
///
/// This only ever runs for the generic OpenAI-compatible path: `anthropic`
/// and `google` never populate `tool_calls` in the first place (see
/// `build_stream_request_body`, which doesn't forward `tools` for them), so
/// there's no provider-specific branching needed here.
async fn continue_after_tool_calls(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    original_messages: &[ChatMessage],
    tool_calls: &[ToolCall],
    tool_results: &[serde_json::Value],
) -> Result<String, LLMError> {
    let url = build_url(provider, base_url, model);
    let client = create_http_client()?;

    let mut msgs: Vec<serde_json::Value> = original_messages
        .iter()
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();

    let tool_calls_json: Vec<_> = tool_calls
        .iter()
        .map(|tc| {
            serde_json::json!({
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                }
            })
        })
        .collect();

    msgs.push(serde_json::json!({
        "role": "assistant",
        "content": serde_json::Value::Null,
        "tool_calls": tool_calls_json,
    }));

    for (tc, result) in tool_calls.iter().zip(tool_results.iter()) {
        msgs.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tc.id,
            "content": serde_json::to_string(result).unwrap_or_else(|_| "null".to_string()),
        }));
    }

    let body = serde_json::json!({
        "model": model,
        "messages": msgs,
        "stream": false,
    });

    let headers = build_headers(provider, api_key);

    log::debug!("Constructed URL for provider {} (tool-call continuation): {}", provider, url);

    let masked_auth = if let Some(h) = headers.get(reqwest::header::AUTHORIZATION) {
        match h.to_str() {
            Ok(s) => mask_auth_header_value(s),
            Err(_) => "<non-utf8>".to_string(),
        }
    } else {
        "<none>".to_string()
    };
    log::debug!("Tool-call continuation auth header (masked): {}", masked_auth);

    let response = match client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("reqwest send error (tool-call continuation) for url '{}': {:?}", url, e);
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
    let streams = ACTIVE_STREAMS.lock().await;
    if let Some(token) = streams.get(&session_id) {
        token.cancel();
        log::info!("Cancelled stream for session: {}", session_id);
    } else {
        // The stream may have already finished naturally between the user
        // clicking stop and this command running -- not an error condition.
        log::info!("No active stream found for session: {} (already finished?)", session_id);
    }
    Ok(())
}
