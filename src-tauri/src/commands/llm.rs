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

/// 图片附件 (base64 编码, 不含 data URL 前缀)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAttachment {
    /// 原始 base64 数据 (不含 "data:...;base64," 前缀)
    pub data: String,
    /// MIME 类型, 如 "image/jpeg"
    pub media_type: String,
}

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
    /// 图片附件 (仅 user 消息有效)
    #[serde(default)]
    pub images: Vec<ImageAttachment>,
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
    /// 是否启用思考模式 (Extended Thinking)
    #[serde(default)]
    pub enable_thinking: bool,
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

fn build_url(provider: &str, base_url: &str, model: &str, streaming: bool) -> String {
    match provider {
        "google" => {
            // Google picks the endpoint by path, not by a body flag like the
            // other providers' `"stream"` field -- the non-streaming
            // follow-up request after a tool call must hit `generateContent`,
            // not `streamGenerateContent`, or the response won't be a single
            // parseable JSON object.
            let method = if streaming { "streamGenerateContent?alt=sse" } else { "generateContent" };
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
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

fn build_stream_request_body(provider: &str, model: &str, messages: &[ChatMessage], tools: &[MCPTool], enable_thinking: bool) -> serde_json::Value {
    match provider {
        "anthropic" => {
            let system_msg = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());

            let msgs: Vec<_> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    let role = if m.role == "assistant" { "assistant" } else { "user" };
                    if role == "user" && !m.images.is_empty() {
                        // Anthropic image format: separate source.media_type + source.data (NOT data URL)
                        let mut blocks: Vec<serde_json::Value> = m.images.iter().map(|img| serde_json::json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": img.media_type, "data": img.data }
                        })).collect();
                        if !m.content.is_empty() {
                            blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                        }
                        serde_json::json!({"role": "user", "content": blocks})
                    } else {
                        serde_json::json!({"role": role, "content": m.content})
                    }
                })
                .collect();

            // Thinking requires higher token budget
            let max_tokens = if enable_thinking { 16000 } else { 4096 };
            let mut body = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": max_tokens,
                "stream": true,
            });

            if enable_thinking {
                body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 8000});
            }

            if let Some(sys) = system_msg {
                body["system"] = serde_json::json!(sys);
            }

            if !tools.is_empty() {
                let tools_json: Vec<_> = tools
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema,
                        })
                    })
                    .collect();
                body["tools"] = serde_json::json!(tools_json);
            }

            body
        }
        "google" => {
            let system_msg = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());

            let contents: Vec<_> = messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    let role = if m.role == "assistant" { "model" } else { "user" };
                    if role == "user" && !m.images.is_empty() {
                        // Gemini image format: inline_data with mime_type + raw base64 (NOT data URL)
                        let mut parts: Vec<serde_json::Value> = vec![];
                        if !m.content.is_empty() {
                            parts.push(serde_json::json!({"text": m.content}));
                        }
                        for img in &m.images {
                            parts.push(serde_json::json!({
                                "inline_data": {"mime_type": img.media_type, "data": img.data}
                            }));
                        }
                        serde_json::json!({"role": "user", "parts": parts})
                    } else {
                        serde_json::json!({"role": role, "parts": [{"text": m.content}]})
                    }
                })
                .collect();

            let mut generation_config = serde_json::json!({
                "maxOutputTokens": 4096,
            });
            if enable_thinking {
                // Gemini 2.5 series: thinkingBudget; 3.x series uses thinkingLevel
                generation_config["thinkingConfig"] = serde_json::json!({"thinkingBudget": 8000});
            }

            let mut body = serde_json::json!({
                "contents": contents,
                "generationConfig": generation_config,
            });

            // Gemini ignores a system-role entry inside `contents` -- the system
            // prompt must go in the separate top-level `systemInstruction` field.
            if let Some(sys) = system_msg {
                body["systemInstruction"] = serde_json::json!({
                    "parts": [{ "text": sys }]
                });
            }

            // Gemini groups every function declaration under a single
            // `tools[0].functionDeclarations` array, unlike OpenAI/Anthropic
            // which list one tool object per entry.
            if !tools.is_empty() {
                let declarations: Vec<_> = tools
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema,
                        })
                    })
                    .collect();
                body["tools"] = serde_json::json!([{ "functionDeclarations": declarations }]);
            }

            body
        }
        _ => {
            let msgs: Vec<_> = messages
                .iter()
                .map(|m| {
                    if m.role == "user" && !m.images.is_empty() {
                        // OpenAI-compatible image format: image_url with data URL
                        let mut content: Vec<serde_json::Value> = vec![];
                        if !m.content.is_empty() {
                            content.push(serde_json::json!({"type": "text", "text": m.content}));
                        }
                        for img in &m.images {
                            content.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": {"url": format!("data:{};base64,{}", img.media_type, img.data)}
                            }));
                        }
                        serde_json::json!({"role": m.role, "content": content})
                    } else {
                        serde_json::json!({"role": m.role, "content": m.content})
                    }
                })
                .collect();

            let mut body = serde_json::json!({
                "model": model,
                "messages": msgs,
                "stream": true,
            });

            // SiliconFlow thinking: enable_thinking + thinking_budget (Qwen3 series)
            if enable_thinking && provider == "siliconflow" {
                body["enable_thinking"] = serde_json::json!(true);
                body["thinking_budget"] = serde_json::json!(8000);
            }

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
pub async fn build_skill_context(skills: &[Skill], app_handle: &AppHandle) -> String {
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
/// `finalize_turn`), it never calls out to anything external by itself.
///
/// Each provider has its own tool-schema shape, so the entry is built
/// differently per branch, but all three are wired through (see
/// `build_stream_request_body`, which now populates `tools` for every
/// provider, not just the generic OpenAI-compatible one).
fn append_skill_tools(body: &mut serde_json::Value, provider: &str, autonomous_skills: &[Skill]) {
    if autonomous_skills.is_empty() {
        return;
    }

    let describe = |skill: &Skill| -> String {
        format!(
            "调用「{}」技能：{}。如果当前任务和这个技能相关，调用它获取具体操作指南。",
            skill.name, skill.description
        )
    };

    match provider {
        "anthropic" => {
            let skill_tools: Vec<_> = autonomous_skills
                .iter()
                .map(|skill| {
                    serde_json::json!({
                        "name": format!("skill__{}", skill.id),
                        "description": describe(skill),
                        "input_schema": { "type": "object", "properties": {} }
                    })
                })
                .collect();

            match body.get_mut("tools").and_then(|t| t.as_array_mut()) {
                Some(existing) => existing.extend(skill_tools),
                None => body["tools"] = serde_json::json!(skill_tools),
            }
        }
        "google" => {
            let declarations: Vec<_> = autonomous_skills
                .iter()
                .map(|skill| {
                    serde_json::json!({
                        "name": format!("skill__{}", skill.id),
                        "description": describe(skill),
                        "parameters": { "type": "object", "properties": {} }
                    })
                })
                .collect();

            // Gemini keeps every function declaration under a single
            // `tools[0].functionDeclarations` array rather than one tool
            // object per entry, so merge into that nested array instead of
            // pushing new top-level `tools` entries.
            let merged = body
                .get_mut("tools")
                .and_then(|t| t.as_array_mut())
                .and_then(|arr| arr.first_mut())
                .and_then(|first| first.get_mut("functionDeclarations"))
                .and_then(|d| d.as_array_mut());

            match merged {
                Some(existing) => existing.extend(declarations),
                None => body["tools"] = serde_json::json!([{ "functionDeclarations": declarations }]),
            }
        }
        _ => {
            let skill_tools: Vec<_> = autonomous_skills
                .iter()
                .map(|skill| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": format!("skill__{}", skill.id),
                            "description": describe(skill),
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
            // Google Gemini format: candidates[0].content.parts[] -- a part is
            // either {"text": ...} or {"functionCall": {"name", "args"}}.
            // Gemini sends a function call's `args` already fully parsed in a
            // single chunk (no incremental fragments like OpenAI/Anthropic),
            // and never supplies an id, so one is synthesized here purely for
            // internal correlation -- it's never sent back to Google.
            let parts = json
                .get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|p| p.as_array())?;

            let mut tool_deltas = Vec::new();
            let mut text_acc = String::new();
            for (i, part) in parts.iter().enumerate() {
                if let Some(call) = part.get("functionCall") {
                    let name = call.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                    let args = call.get("args").cloned().unwrap_or_else(|| serde_json::json!({}));
                    tool_deltas.push(ToolCallDelta {
                        index: i as u32,
                        id: Some(format!("google_call_{}", Uuid::new_v4())),
                        name: Some(name),
                        arguments_fragment: Some(args.to_string()),
                    });
                } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    text_acc.push_str(text);
                }
            }

            if !tool_deltas.is_empty() {
                Some(StreamContent::ToolCallDeltas(tool_deltas))
            } else if !text_acc.is_empty() {
                Some(StreamContent::Text(text_acc))
            } else {
                None
            }
        }
        "anthropic" => {
            // Anthropic streams text via content_block_delta{delta.type=text_delta},
            // and a tool call via content_block_start{content_block.type=tool_use}
            // (carries id+name, empty input) followed by one or more
            // content_block_delta{delta.type=input_json_delta} events (carry
            // `partial_json` fragments of the input object, to be concatenated
            // by `index` same as OpenAI's argument fragments). The turn's real
            // end-of-stream signal is a `message_stop` event, not a `[DONE]`
            // sentinel.
            match json.get("type").and_then(|t| t.as_str()) {
                Some("content_block_delta") => {
                    let index = json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let delta = json.get("delta")?;
                    match delta.get("type").and_then(|t| t.as_str()) {
                        Some("text_delta") => delta
                            .get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| StreamContent::Text(s.to_string())),
                        Some("input_json_delta") => {
                            let fragment = delta
                                .get("partial_json")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            Some(StreamContent::ToolCallDeltas(vec![ToolCallDelta {
                                index,
                                id: None,
                                name: None,
                                arguments_fragment: Some(fragment),
                            }]))
                        }
                        _ => None,
                    }
                }
                Some("content_block_start") => {
                    let index = json.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let block = json.get("content_block")?;
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let id = block.get("id").and_then(|t| t.as_str()).map(|s| s.to_string());
                        let name = block.get("name").and_then(|t| t.as_str()).map(|s| s.to_string());
                        Some(StreamContent::ToolCallDeltas(vec![ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_fragment: None,
                        }]))
                    } else {
                        None
                    }
                }
                Some("message_stop") => Some(StreamContent::Done),
                _ => None,
            }
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
    log::info!(
        "[LLM] stream_message: session={} provider={} model={} messages={} mcp={}",
        request.session_id, request.provider, request.model,
        request.messages.len(), request.enable_mcp
    );
    
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
                    images: vec![],
                });
            }
        }
    }

    let url = build_url(&request.provider, &request.base_url, &request.model, true);
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
    let mut body = build_stream_request_body(&request.provider, &request.model, &effective_messages, &mcp_tools, request.enable_thinking);
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
                                        return finalize_turn(
                                            &app_handle,
                                            state.clone(),
                                            &request,
                                            &message_id,
                                            &effective_messages,
                                            &mcp_tools,
                                            &all_skills,
                                            std::mem::take(&mut tool_call_acc),
                                        )
                                        .await;
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Err(LLMError::StreamError(e.to_string()));
                    }
                    None => {
                        // Stream ended without an explicit end-of-turn signal
                        // (Google never sends one) -- finalize whatever tool
                        // calls accumulated so far the same way an explicit
                        // `StreamContent::Done` would.
                        return finalize_turn(
                            &app_handle,
                            state.clone(),
                            &request,
                            &message_id,
                            &effective_messages,
                            &mcp_tools,
                            &all_skills,
                            std::mem::take(&mut tool_call_acc),
                        )
                        .await;
                    }
                }
            }
        }
    }
}

/// Finalize whatever tool-call fragments have accumulated by the end of a
/// turn (id/name from the first fragment per index, arguments concatenated
/// across every fragment for that index), execute them if any, ask the model
/// to continue with the results, and emit the terminal `done: true` chunk.
///
/// Shared between the explicit end-of-turn signal (OpenAI's `[DONE]`,
/// Anthropic's `message_stop`) and a stream that simply closes with no such
/// signal (Google) -- both need identical finalize-and-continue handling.
async fn finalize_turn(
    app_handle: &AppHandle,
    state: tauri::State<'_, DbState>,
    request: &SendMessageRequest,
    message_id: &str,
    effective_messages: &[ChatMessage],
    mcp_tools: &[MCPTool],
    all_skills: &[Skill],
    tool_call_acc: std::collections::BTreeMap<u32, PartialToolCall>,
) -> Result<(), LLMError> {
    let tool_calls: Vec<ToolCall> = tool_call_acc
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
                    let content = build_skill_context(std::slice::from_ref(skill), app_handle).await;
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

        // Continue the conversation with a single follow-up request carrying
        // the provider-appropriate "here's what I called and what it
        // returned" shape, instead of one request per call.
        match continue_after_tool_calls(
            &request.provider,
            &request.model,
            &request.api_key,
            &request.base_url,
            effective_messages,
            &tool_calls,
            &tool_results,
        )
        .await
        {
            Ok(live_reply) => {
                let _ = app_handle.emit("stream-chunk", StreamChunk {
                    session_id: request.session_id.clone(),
                    message_id: message_id.to_string(),
                    content: live_reply,
                    done: false,
                });
            }
            Err(err) => {
                log::error!("Failed to continue reasoning after tool calls: {}", err);
            }
        }
    }

    log::info!("[LLM] stream_message 完成: session={}", request.session_id);
    let _ = app_handle.emit("stream-chunk", StreamChunk {
        session_id: request.session_id.clone(),
        message_id: message_id.to_string(),
        content: String::new(),
        done: true,
    });
    Ok(())
}

/// Continue a conversation after one or more tool calls have been executed,
/// sending a single non-streaming follow-up request that tells the model
/// what was called and what it returned, then returns its plain-text reply.
/// Each provider has its own shape for "here's what I called" /
/// "here's the result", so the request body and the response parsing both
/// branch by provider.
async fn continue_after_tool_calls(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    original_messages: &[ChatMessage],
    tool_calls: &[ToolCall],
    tool_results: &[serde_json::Value],
) -> Result<String, LLMError> {
    let url = build_url(provider, base_url, model, false);
    let client = create_http_client()?;

    let body = match provider {
        "anthropic" => {
            let system_msg = original_messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());
            let mut msgs: Vec<serde_json::Value> = original_messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "assistant" } else { "user" },
                        "content": m.content
                    })
                })
                .collect();

            // Anthropic requires the tool_use/tool_result blocks to be batched
            // into exactly one assistant message and one user message (it
            // enforces strict user/assistant alternation), unlike OpenAI's
            // one-tool-message-per-result shape below.
            let tool_use_blocks: Vec<_> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({})),
                    })
                })
                .collect();
            msgs.push(serde_json::json!({ "role": "assistant", "content": tool_use_blocks }));

            let tool_result_blocks: Vec<_> = tool_calls
                .iter()
                .zip(tool_results.iter())
                .map(|(tc, result)| {
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tc.id,
                        "content": serde_json::to_string(result).unwrap_or_else(|_| "null".to_string()),
                    })
                })
                .collect();
            msgs.push(serde_json::json!({ "role": "user", "content": tool_result_blocks }));

            let mut b = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": 4096,
                "stream": false,
            });
            if let Some(sys) = system_msg {
                b["system"] = serde_json::json!(sys);
            }
            b
        }
        "google" => {
            let system_msg = original_messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());
            let mut contents: Vec<serde_json::Value> = original_messages
                .iter()
                .filter(|m| m.role != "system")
                .map(|m| {
                    serde_json::json!({
                        "role": if m.role == "assistant" { "model" } else { "user" },
                        "parts": [{ "text": m.content }]
                    })
                })
                .collect();

            let call_parts: Vec<_> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "functionCall": {
                            "name": tc.function.name,
                            "args": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                .unwrap_or_else(|_| serde_json::json!({})),
                        }
                    })
                })
                .collect();
            contents.push(serde_json::json!({ "role": "model", "parts": call_parts }));

            let response_parts: Vec<_> = tool_calls
                .iter()
                .zip(tool_results.iter())
                .map(|(tc, result)| {
                    serde_json::json!({
                        "functionResponse": {
                            "name": tc.function.name,
                            "response": result,
                        }
                    })
                })
                .collect();
            contents.push(serde_json::json!({ "role": "function", "parts": response_parts }));

            let mut b = serde_json::json!({
                "contents": contents,
                "generationConfig": { "maxOutputTokens": 4096 },
            });
            if let Some(sys) = system_msg {
                b["systemInstruction"] = serde_json::json!({ "parts": [{ "text": sys }] });
            }
            b
        }
        _ => {
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

            serde_json::json!({
                "model": model,
                "messages": msgs,
                "stream": false,
            })
        }
    };

    let headers = build_headers(provider, api_key);

    log::debug!("Constructed URL for provider {} (tool-call continuation): {}", provider, url);

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

    match provider {
        "anthropic" => json
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.iter().find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text")))
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| LLMError::ApiError("LLM did not return content".to_string())),
        "google" => json
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|content| content.get("parts"))
            .and_then(|parts| parts.as_array())
            .and_then(|arr| arr.first())
            .and_then(|part| part.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| LLMError::ApiError("LLM did not return content".to_string())),
        _ => {
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
    }
}

/// A tool call the model wants executed, fully parsed and ready to dispatch.
/// Distinct from the private streaming-only `ToolCall`/`ToolFunction` above:
/// this is the multi-round, non-streaming counterpart used by `run_turn`
/// (Workspace Agent loop), where `arguments` is already a parsed JSON value
/// rather than a string fragment that still needs concatenating.
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of one `run_turn` round-trip.
#[derive(Debug)]
pub enum TurnOutcome {
    Text(String),
    ToolCalls(Vec<PendingToolCall>),
}

/// Build the provider-native "conversation so far" as a JSON array from a
/// flat `ChatMessage` history. This is the multi-round-capable counterpart to
/// `build_stream_request_body`'s inline message mapping: a flat `ChatMessage`
/// list can't represent a tool_use/tool_result round (Anthropic/Google encode
/// those as structured content blocks, not plain text), so callers needing
/// more than one round of tool calling -- the Workspace Agent loop -- build
/// the native array once here, then grow it in place with `append_tool_round`
/// / `append_text_reply` across rounds instead of re-deriving it each time.
pub fn build_native_messages(provider: &str, messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    match provider {
        "anthropic" => messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": if m.role == "assistant" { "assistant" } else { "user" },
                    "content": m.content
                })
            })
            .collect(),
        "google" => messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": if m.role == "assistant" { "model" } else { "user" },
                    "parts": [{ "text": m.content }]
                })
            })
            .collect(),
        _ => messages
            .iter()
            .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
            .collect(),
    }
}

/// Append one tool-call round (the model's calls + their executed results)
/// onto a native message array, in the shape each provider expects to see it
/// echoed back as history on the next round. Mirrors the per-provider shapes
/// already established in `continue_after_tool_calls`.
pub fn append_tool_round(
    provider: &str,
    native_messages: &mut Vec<serde_json::Value>,
    calls: &[PendingToolCall],
    results: &[serde_json::Value],
) {
    match provider {
        "anthropic" => {
            let tool_use_blocks: Vec<_> = calls
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments,
                    })
                })
                .collect();
            native_messages.push(serde_json::json!({ "role": "assistant", "content": tool_use_blocks }));

            let tool_result_blocks: Vec<_> = calls
                .iter()
                .zip(results.iter())
                .map(|(c, r)| {
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": c.id,
                        "content": serde_json::to_string(r).unwrap_or_else(|_| "null".to_string()),
                    })
                })
                .collect();
            native_messages.push(serde_json::json!({ "role": "user", "content": tool_result_blocks }));
        }
        "google" => {
            let call_parts: Vec<_> = calls
                .iter()
                .map(|c| serde_json::json!({ "functionCall": { "name": c.name, "args": c.arguments } }))
                .collect();
            native_messages.push(serde_json::json!({ "role": "model", "parts": call_parts }));

            let response_parts: Vec<_> = calls
                .iter()
                .zip(results.iter())
                .map(|(c, r)| serde_json::json!({ "functionResponse": { "name": c.name, "response": r } }))
                .collect();
            native_messages.push(serde_json::json!({ "role": "function", "parts": response_parts }));
        }
        _ => {
            let tool_calls_json: Vec<_> = calls
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "type": "function",
                        "function": {
                            "name": c.name,
                            "arguments": serde_json::to_string(&c.arguments).unwrap_or_else(|_| "{}".to_string()),
                        }
                    })
                })
                .collect();
            native_messages.push(serde_json::json!({
                "role": "assistant", "content": serde_json::Value::Null, "tool_calls": tool_calls_json,
            }));
            for (c, r) in calls.iter().zip(results.iter()) {
                native_messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": c.id,
                    "content": serde_json::to_string(r).unwrap_or_else(|_| "null".to_string()),
                }));
            }
        }
    }
}

/// Append the model's own final plain-text reply onto the native message
/// array, so the next outer call to `run_turn` (e.g. once a new Workspace
/// message arrives) sees it as prior assistant history.
pub fn append_text_reply(provider: &str, native_messages: &mut Vec<serde_json::Value>, text: &str) {
    match provider {
        "google" => native_messages.push(serde_json::json!({ "role": "model", "parts": [{ "text": text }] })),
        _ => native_messages.push(serde_json::json!({ "role": "assistant", "content": text })),
    }
}

/// One non-streaming round-trip: send the conversation-so-far + available
/// tools, return either the model's final text reply or the tool calls it
/// wants executed. Unlike `continue_after_tool_calls` (which sends exactly
/// one follow-up, never re-offers `tools`, and only ever returns text), this
/// always re-offers `tools` and can return `ToolCalls` again -- it's what
/// lets the Workspace Agent loop keep calling tools across multiple rounds
/// instead of just one.
pub async fn run_turn(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    system_prompt: Option<&str>,
    native_messages: &[serde_json::Value],
    tools: &[MCPTool],
) -> Result<TurnOutcome, LLMError> {
    let url = build_url(provider, base_url, model, false);
    let client = create_http_client()?;

    let body = match provider {
        "anthropic" => {
            let mut b = serde_json::json!({
                "model": model, "messages": native_messages, "max_tokens": 4096, "stream": false,
            });
            if let Some(sys) = system_prompt {
                b["system"] = serde_json::json!(sys);
            }
            if !tools.is_empty() {
                let tools_json: Vec<_> = tools
                    .iter()
                    .map(|t| serde_json::json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
                    .collect();
                b["tools"] = serde_json::json!(tools_json);
            }
            b
        }
        "google" => {
            let mut b = serde_json::json!({
                "contents": native_messages, "generationConfig": { "maxOutputTokens": 4096 },
            });
            if let Some(sys) = system_prompt {
                b["systemInstruction"] = serde_json::json!({ "parts": [{ "text": sys }] });
            }
            if !tools.is_empty() {
                let declarations: Vec<_> = tools
                    .iter()
                    .map(|t| serde_json::json!({ "name": t.name, "description": t.description, "parameters": t.input_schema }))
                    .collect();
                b["tools"] = serde_json::json!([{ "functionDeclarations": declarations }]);
            }
            b
        }
        _ => {
            let mut all_messages = Vec::with_capacity(native_messages.len() + 1);
            if let Some(sys) = system_prompt {
                all_messages.push(serde_json::json!({ "role": "system", "content": sys }));
            }
            all_messages.extend_from_slice(native_messages);
            let mut b = serde_json::json!({ "model": model, "messages": all_messages, "stream": false });
            if !tools.is_empty() {
                let tools_json: Vec<_> = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "type": "function",
                            "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
                        })
                    })
                    .collect();
                b["tools"] = serde_json::json!(tools_json);
            }
            b
        }
    };

    let headers = build_headers(provider, api_key);
    let response = client.post(&url).headers(headers).json(&body).send().await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "unknown".to_string());
        return Err(LLMError::ApiError(error_text));
    }

    let json: serde_json::Value = response.json().await.map_err(LLMError::RequestError)?;

    match provider {
        "anthropic" => {
            let blocks = json.get("content").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            let calls: Vec<PendingToolCall> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                .map(|b| PendingToolCall {
                    id: b.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    name: b.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    arguments: b.get("input").cloned().unwrap_or_else(|| serde_json::json!({})),
                })
                .collect();
            if !calls.is_empty() {
                return Ok(TurnOutcome::ToolCalls(calls));
            }
            let text = blocks
                .iter()
                .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            Ok(TurnOutcome::Text(text))
        }
        "google" => {
            let parts = json
                .get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            let calls: Vec<PendingToolCall> = parts
                .iter()
                .filter_map(|p| p.get("functionCall"))
                .map(|call| PendingToolCall {
                    id: format!("google_call_{}", Uuid::new_v4()),
                    name: call.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    arguments: call.get("args").cloned().unwrap_or_else(|| serde_json::json!({})),
                })
                .collect();
            if !calls.is_empty() {
                return Ok(TurnOutcome::ToolCalls(calls));
            }
            let text: String = parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect();
            Ok(TurnOutcome::Text(text))
        }
        _ => {
            let message = json
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("message"))
                .cloned()
                .unwrap_or_default();
            let calls: Vec<PendingToolCall> = message
                .get("tool_calls")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|tc| {
                            let id = tc.get("id").and_then(|v| v.as_str())?.to_string();
                            let name = tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str())?.to_string();
                            let args_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or("{}");
                            let arguments = serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}));
                            Some(PendingToolCall { id, name, arguments })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !calls.is_empty() {
                return Ok(TurnOutcome::ToolCalls(calls));
            }
            let text = message.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(TurnOutcome::Text(text))
        }
    }
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

#[cfg(test)]
mod provider_tool_calling_tests {
    use super::*;

    fn sample_tool() -> MCPTool {
        MCPTool {
            server_id: "srv1".to_string(),
            server_name: "srv".to_string(),
            name: "get_weather".to_string(),
            description: "Get current weather".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "city": { "type": "string" } },
                "required": ["city"]
            }),
        }
    }

    #[test]
    fn anthropic_request_body_carries_tools_in_anthropic_shape() {
        let messages = vec![ChatMessage {
            id: "1".into(), role: "user".into(), content: "hi".into(),
            timestamp: 0, error: None, images: vec![],
        }];
        let body = build_stream_request_body("anthropic", "claude-3-5-sonnet", &messages, &[sample_tool()], false);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert!(tools[0].get("input_schema").is_some(), "anthropic tools use `input_schema`, not `parameters`");
        assert!(tools[0].get("function").is_none(), "anthropic tools must not be nested under `function` like OpenAI");
    }

    #[test]
    fn google_request_body_groups_tools_under_function_declarations() {
        let messages = vec![ChatMessage {
            id: "1".into(), role: "user".into(), content: "hi".into(),
            timestamp: 0, error: None, images: vec![],
        }];
        let body = build_stream_request_body("google", "gemini-1.5-pro", &messages, &[sample_tool()], false);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools.len(), 1, "Gemini nests every declaration under a single tools[0] entry");
        let declarations = tools[0]["functionDeclarations"].as_array().expect("functionDeclarations array");
        assert_eq!(declarations[0]["name"], "get_weather");
        assert!(declarations[0].get("parameters").is_some());
    }

    #[test]
    fn anthropic_tool_use_block_then_input_json_delta_accumulates_into_tool_call_deltas() {
        let start = parse_sse_line(
            "anthropic",
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"get_weather","input":{}}}"#,
        ).expect("should parse content_block_start");
        match start {
            StreamContent::ToolCallDeltas(deltas) => {
                assert_eq!(deltas.len(), 1);
                assert_eq!(deltas[0].index, 1);
                assert_eq!(deltas[0].id.as_deref(), Some("toolu_01"));
                assert_eq!(deltas[0].name.as_deref(), Some("get_weather"));
            }
            other => panic!("expected ToolCallDeltas, got {:?}", other),
        }

        let delta = parse_sse_line(
            "anthropic",
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\": \"SF\"}"}}"#,
        ).expect("should parse content_block_delta");
        match delta {
            StreamContent::ToolCallDeltas(deltas) => {
                assert_eq!(deltas[0].index, 1);
                assert_eq!(deltas[0].arguments_fragment.as_deref(), Some("{\"city\": \"SF\"}"));
            }
            other => panic!("expected ToolCallDeltas, got {:?}", other),
        }

        let stop = parse_sse_line("anthropic", r#"data: {"type":"message_stop"}"#);
        assert!(matches!(stop, Some(StreamContent::Done)));
    }

    #[test]
    fn anthropic_text_delta_still_parses_as_text() {
        let text = parse_sse_line(
            "anthropic",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        );
        assert!(matches!(text, Some(StreamContent::Text(ref s)) if s == "Hello"));
    }

    #[test]
    fn google_function_call_part_parses_as_tool_call_delta_with_full_args() {
        let parsed = parse_sse_line(
            "google",
            r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"city":"SF"}}}]}}]}"#,
        ).expect("should parse functionCall chunk");
        match parsed {
            StreamContent::ToolCallDeltas(deltas) => {
                assert_eq!(deltas.len(), 1);
                assert!(deltas[0].id.is_some(), "google has no native call id, one must be synthesized");
                assert_eq!(deltas[0].name.as_deref(), Some("get_weather"));
                let args: serde_json::Value = serde_json::from_str(deltas[0].arguments_fragment.as_deref().unwrap()).unwrap();
                assert_eq!(args["city"], "SF");
            }
            other => panic!("expected ToolCallDeltas, got {:?}", other),
        }
    }

    #[test]
    fn google_text_part_still_parses_as_text() {
        let parsed = parse_sse_line(
            "google",
            r#"data: {"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#,
        );
        assert!(matches!(parsed, Some(StreamContent::Text(ref s)) if s == "Hello"));
    }

    #[test]
    fn openai_shape_unaffected_by_provider_branching() {
        let messages = vec![ChatMessage {
            id: "1".into(), role: "user".into(), content: "hi".into(),
            timestamp: 0, error: None, images: vec![],
        }];
        let body = build_stream_request_body("openai", "gpt-4o", &messages, &[sample_tool()], false);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
    }

    fn sample_call() -> PendingToolCall {
        PendingToolCall {
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
            arguments: serde_json::json!({ "city": "SF" }),
        }
    }

    #[test]
    fn append_tool_round_anthropic_batches_tool_use_and_tool_result_into_one_pair_of_messages() {
        let mut native = vec![serde_json::json!({ "role": "user", "content": "what's the weather in SF?" })];
        let calls = vec![sample_call()];
        let results = vec![serde_json::json!({ "temp": 70 })];
        append_tool_round("anthropic", &mut native, &calls, &results);

        assert_eq!(native.len(), 3, "original message + 1 assistant tool_use msg + 1 user tool_result msg");
        assert_eq!(native[1]["role"], "assistant");
        assert_eq!(native[1]["content"][0]["type"], "tool_use");
        assert_eq!(native[1]["content"][0]["id"], "call_1");
        assert_eq!(native[2]["role"], "user");
        assert_eq!(native[2]["content"][0]["type"], "tool_result");
        assert_eq!(native[2]["content"][0]["tool_use_id"], "call_1");
    }

    #[test]
    fn append_tool_round_google_appends_model_function_call_then_function_response() {
        let mut native = vec![serde_json::json!({ "role": "user", "parts": [{ "text": "what's the weather in SF?" }] })];
        let calls = vec![sample_call()];
        let results = vec![serde_json::json!({ "temp": 70 })];
        append_tool_round("google", &mut native, &calls, &results);

        assert_eq!(native[1]["role"], "model");
        assert_eq!(native[1]["parts"][0]["functionCall"]["name"], "get_weather");
        assert_eq!(native[2]["role"], "function");
        assert_eq!(native[2]["parts"][0]["functionResponse"]["name"], "get_weather");
    }

    #[test]
    fn append_tool_round_openai_appends_assistant_tool_calls_then_one_tool_message_per_result() {
        let mut native = vec![serde_json::json!({ "role": "user", "content": "what's the weather in SF?" })];
        let calls = vec![sample_call()];
        let results = vec![serde_json::json!({ "temp": 70 })];
        append_tool_round("openai", &mut native, &calls, &results);

        assert_eq!(native[1]["role"], "assistant");
        assert_eq!(native[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(native[2]["role"], "tool");
        assert_eq!(native[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn build_native_messages_matches_provider_shapes() {
        let messages = vec![
            ChatMessage { id: "0".into(), role: "system".into(), content: "be nice".into(), timestamp: 0, error: None, images: vec![] },
            ChatMessage { id: "1".into(), role: "user".into(), content: "hi".into(), timestamp: 0, error: None, images: vec![] },
            ChatMessage { id: "2".into(), role: "assistant".into(), content: "hello".into(), timestamp: 0, error: None, images: vec![] },
        ];

        let anthropic = build_native_messages("anthropic", &messages);
        assert_eq!(anthropic.len(), 2, "system message excluded, carried separately");
        assert_eq!(anthropic[1]["role"], "assistant");
        assert_eq!(anthropic[1]["content"], "hello");

        let google = build_native_messages("google", &messages);
        assert_eq!(google[1]["role"], "model");
        assert_eq!(google[1]["parts"][0]["text"], "hello");

        let openai = build_native_messages("openai", &messages);
        assert_eq!(openai.len(), 3, "openai keeps the system message inline");
    }

    #[test]
    fn append_text_reply_uses_model_role_for_google_and_assistant_for_others() {
        let mut native = vec![];
        append_text_reply("google", &mut native, "done");
        assert_eq!(native[0]["role"], "model");
        assert_eq!(native[0]["parts"][0]["text"], "done");

        let mut native = vec![];
        append_text_reply("anthropic", &mut native, "done");
        assert_eq!(native[0]["role"], "assistant");
        assert_eq!(native[0]["content"], "done");
    }
}
