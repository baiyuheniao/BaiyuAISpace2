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

use crate::commands::constants::{LLM_CONNECT_TIMEOUT, LLM_REQUEST_TIMEOUT, LLM_STREAM_READ_TIMEOUT};
use crate::commands::mcp::{get_all_mcp_tools, call_mcp_tool, MCPTool};
use crate::commands::skills::{read_skill_resource_text, Skill};
use crate::db::DbState;
use keyring::Entry as KeyringEntry;
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

/// 视频附件 (base64 编码, 不含 data URL 前缀, 仅 Gemini provider 支持)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoAttachment {
    pub data: String,
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
    /// 视频附件 (仅 Gemini provider 有效, 其他 provider 忽略)
    #[serde(default)]
    pub videos: Vec<VideoAttachment>,
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
    /// 最大输出 token 数（None 时使用默认值: 普通模式 4096, 思考模式 16000）
    #[serde(default)]
    pub max_tokens: Option<u32>,
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

// 每个正在进行的流对应一个取消令牌，以 session_id 为键，
// 这样 `cancel_stream` 就能通知 `stream_message` 的读取循环提前停止。
static ACTIVE_STREAMS: Lazy<Arc<Mutex<HashMap<String, CancellationToken>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

// 错误类型
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
    // OpenClaw 本地网关默认监听 127.0.0.1:18789，/v1/chat/completions 走
    // OpenAI 兼容格式，但该端点默认是关闭的（需要在 OpenClaw 的
    // gateway.http.endpoints.chatCompletions.enabled 里手动开启），且网关
    // auth 默认必须启用 —— 回环地址并不天然免鉴权，用户需要在 OpenClaw 侧
    // 配置 gateway.auth.token 并在这里填入相同的 Bearer token。
    ("openclaw", "", "bearer"),
];

fn build_url(provider: &str, base_url: &str, model: &str, streaming: bool) -> String {
    match provider {
        "google" => {
            // Google 是通过路径来区分端点的，不像其他 provider 那样靠请求体里的
            // `"stream"` 字段——工具调用之后那次非流式的续写请求必须打到
            // `generateContent`，而不是 `streamGenerateContent`，否则返回的
            // 就不是一个可解析的完整 JSON 对象。
            let method = if streaming { "streamGenerateContent?alt=sse" } else { "generateContent" };
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:{}",
                model, method
            )
        }
        "azure" => {
            // 本项目的约定（参见 settings.ts 里的默认占位符
            // "https://your-resource.openai.azure.com/openai/deployments/"）：
            // 用户填的 base_url 已经包含了 `/openai/deployments/` 这一段，
            // 所以这里只需要再拼上部署名（`model`）+ `/chat/completions`。
            // 这正好对应真实的 Azure OpenAI REST 路径：
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
        "openclaw" => format!("{}/chat/completions", base_url.trim_end_matches('/')),
        _ => {
            if let Some((_, url, _)) = PROVIDER_CONFIGS.iter().find(|(p, _, _)| *p == provider) {
                url.to_string()
            } else {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        }
    }
}

fn build_stream_request_body(provider: &str, model: &str, messages: &[ChatMessage], tools: &[MCPTool], enable_thinking: bool, max_tokens: Option<u32>) -> serde_json::Value {
    // 一次流式请求如果在收到任何 token 之前就被停止了（见 `cancel_stream`），
    // 会留下一条内容为空、也没有附件的 assistant 消息。把这种消息原样传回去
    // 作为历史毫无意义，而且有些要求严格的 provider（比如 Moonshot）会直接
    // 用 "message ... must not be empty" 拒掉整个请求——由于这条空消息永远
    // 不会自动从历史里消失，之后每一轮都会重复触发这个 400。
    let non_empty: Vec<ChatMessage> = messages
        .iter()
        .filter(|m| !m.content.trim().is_empty() || !m.images.is_empty() || !m.videos.is_empty())
        .cloned()
        .collect();
    let messages = non_empty.as_slice();
    match provider {
        "anthropic" => {
            let system_msg = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());

            let non_system: Vec<&ChatMessage> = messages.iter().filter(|m| m.role != "system").collect();
            // Prompt caching：把「最新一条消息之前的那条消息」标记为缓存断点。
            // 断点及其之前的内容都会在服务端被缓存，这样随着对话变长，每次请求
            // 只需为最新这一轮全价付费，而不用把整段历史都重新处理一遍。
            // 如果还没有真正的历史（只有第一条消息），就没什么好缓存的，跳过。
            let cache_breakpoint_idx = non_system.len().checked_sub(2);

            let msgs: Vec<_> = non_system
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let role = if m.role == "assistant" { "assistant" } else { "user" };
                    let mut blocks: Vec<serde_json::Value> = if role == "user" && !m.images.is_empty() {
                        // Anthropic 的图片格式：source.media_type 和 source.data 是分开的字段（不是 data URL）
                        let mut b: Vec<serde_json::Value> = m.images.iter().map(|img| serde_json::json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": img.media_type, "data": img.data }
                        })).collect();
                        if !m.content.is_empty() {
                            b.push(serde_json::json!({"type": "text", "text": m.content}));
                        }
                        b
                    } else {
                        vec![serde_json::json!({"type": "text", "text": m.content})]
                    };

                    if cache_breakpoint_idx == Some(i) {
                        if let Some(last_block) = blocks.last_mut() {
                            last_block["cache_control"] = serde_json::json!({"type": "ephemeral"});
                        }
                    }

                    serde_json::json!({"role": role, "content": blocks})
                })
                .collect();

            // max_tokens 对 Anthropic 的 Messages API 是必填字段（不像这里其他
            // provider 那样可以直接省略），所以哪怕用户没填，也得给一个具体数值。
            // 32000 足够覆盖一段较长的回答，又不会超出模型输出 token 的上限被拒绝
            // （注意：这个上限比 200K 的*上下文*窗口小得多——如果天真地直接拿
            // 上下文大小当默认值，这里会直接 400）。
            //
            // 开启 thinking 时，这个上限必须超过它自己的 budget；具体到旧版
            // budget_tokens 格式，上限必须超过 8000，所以只要用户值搭配了
            // legacy thinking，就不能低于 9000。
            let is_legacy_thinking = enable_thinking
                && (model.contains("claude-3") || model.contains("4-5") || model.contains("4.5"));
            let default_max: u32 = 32000;
            let max_tokens_val = match max_tokens {
                Some(v) if is_legacy_thinking => v.max(9000),
                Some(v) => v,
                None => default_max,
            };
            let mut body = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": max_tokens_val,
                "stream": true,
            });

            if enable_thinking {
                // Claude 4.6 及以上（Opus 4.6、Sonnet 4.6、Opus 4.7、Opus 4.8、Fable 5……）
                // 用的是新的 adaptive thinking API；budget_tokens 在 4.7+ 上会被 400 拒绝。
                // 更早的 Claude 3.x / 4.5 系列模型仍然需要旧版的 enabled+budget_tokens 写法。
                let is_legacy_thinking = model.contains("claude-3")
                    || model.contains("4-5")
                    || model.contains("4.5");
                if is_legacy_thinking {
                    body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 8000});
                } else {
                    body["thinking"] = serde_json::json!({"type": "adaptive"});
                }
            }

            if let Some(sys) = system_msg {
                // 对同一个 Agent/Skill 配置来说，system prompt 每次请求都完全一样，
                // 所以它是最值得缓存的内容——始终标记它。
                body["system"] = serde_json::json!([{
                    "type": "text",
                    "text": sys,
                    "cache_control": {"type": "ephemeral"}
                }]);
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
                    if role == "user" && (!m.images.is_empty() || !m.videos.is_empty()) {
                        // Gemini 的多模态格式：inline_data 里放 mime_type + 原始 base64（不是 data URL）
                        let mut parts: Vec<serde_json::Value> = vec![];
                        if !m.content.is_empty() {
                            parts.push(serde_json::json!({"text": m.content}));
                        }
                        for img in &m.images {
                            parts.push(serde_json::json!({
                                "inline_data": {"mime_type": img.media_type, "data": img.data}
                            }));
                        }
                        for vid in &m.videos {
                            parts.push(serde_json::json!({
                                "inline_data": {"mime_type": vid.media_type, "data": vid.data}
                            }));
                        }
                        serde_json::json!({"role": "user", "parts": parts})
                    } else {
                        serde_json::json!({"role": role, "parts": [{"text": m.content}]})
                    }
                })
                .collect();

            // 和 Anthropic 不同，Gemini 并不要求必须填 maxOutputTokens——省略这个字段
            // 会让模型用它自己（高得多）的默认值，而不会被一个写死的小上限悄悄截断长回复。
            let mut generation_config = serde_json::json!({});
            if let Some(v) = max_tokens {
                generation_config["maxOutputTokens"] = serde_json::json!(v);
            }
            if enable_thinking {
                // Gemini 2.5 系列用 thinkingBudget；3.x 系列用的是 thinkingLevel
                generation_config["thinkingConfig"] = serde_json::json!({"thinkingBudget": 8000});
            }

            let mut body = serde_json::json!({
                "contents": contents,
                "generationConfig": generation_config,
            });

            // Gemini 会忽略 `contents` 里 role 为 system 的条目——system prompt
            // 必须放到单独的顶层字段 `systemInstruction` 里。
            if let Some(sys) = system_msg {
                body["systemInstruction"] = serde_json::json!({
                    "parts": [{ "text": sys }]
                });
            }

            // Gemini 把所有函数声明都归到同一个
            // `tools[0].functionDeclarations` 数组里，不像 OpenAI/Anthropic
            // 那样一个 tool 对象一条记录地平铺列出。
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
            // Mistral 的 Chat Completions 端点把 data URI 直接当作 `image_url`
            // 的值（一个字符串），而不是像 OpenAI 及这里其他"OpenAI 兼容"的
            // provider 那样嵌套在 `{"url": ...}` 对象里。给 Mistral 发嵌套对象
            // 格式的话，服务端会解析失败。
            let is_mistral = provider == "mistral";

            let msgs: Vec<_> = messages
                .iter()
                .map(|m| {
                    if m.role == "user" && !m.images.is_empty() {
                        // OpenAI 兼容的图片格式：image_url 内嵌 data URL
                        let mut content: Vec<serde_json::Value> = vec![];
                        if !m.content.is_empty() {
                            content.push(serde_json::json!({"type": "text", "text": m.content}));
                        }
                        for img in &m.images {
                            let data_uri = format!("data:{};base64,{}", img.media_type, img.data);
                            content.push(serde_json::json!({
                                "type": "image_url",
                                "image_url": if is_mistral { serde_json::json!(data_uri) } else { serde_json::json!({"url": data_uri}) }
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

            // 未设置时直接省略字段，而不是拿一个猜测值去顶替；这些 provider 并不
            // 强制要求这个字段，而一个写死的小数值会让所有没填这项的用户的长回复
            // 被悄悄截断。
            if let Some(v) = max_tokens {
                body["max_tokens"] = serde_json::json!(v);
            }

            // SiliconFlow 的 thinking：enable_thinking + thinking_budget（Qwen3 系列）
            if enable_thinking && provider == "siliconflow" {
                body["enable_thinking"] = serde_json::json!(true);
                body["thinking_budget"] = serde_json::json!(8000);
            }

            // 如果有可用工具就加进去
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

/// 为一个或多个已激活的 skill 构造合并后的 instructions + 可读资源文件文本，
/// 准备好合并进 system prompt。
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

/// 为模型可以自主调用的每个 skill 追加一条合成的工具定义。这个工具只携带
/// name + description——调用它实际返回的是该 skill 的 instructions 作为结果
/// （见 `finalize_turn` 里对 `skill__` 的处理），它本身从不对外发起任何调用。
///
/// 每家 provider 的工具 schema 形状都不一样，所以这里按分支分别构造，但三种
/// 形状都已接入（参见 `build_stream_request_body`，它现在会给每一家 provider
/// 都填充 `tools`，而不只是通用的 OpenAI 兼容分支）。
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

            // Gemini 把所有函数声明都放在同一个
            // `tools[0].functionDeclarations` 数组里，而不是一个 tool 对象
            // 对应一条记录，所以这里要合并进那个嵌套数组，而不是往顶层
            // `tools` 里追加新条目。
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

/// 目标是否回环地址 (localhost/127.0.0.1/::1) —— 本地部署的模型服务
/// (Ollama/LM Studio 等经由 "local"/"custom"/"openclaw" provider 走到这里)
/// 走这条路径时应绕开系统代理，否则用户为访问境外服务商而开启的全局代理
/// 会把本该直连本机的请求也绕出去一圈，白白拖慢 TTFT。
fn is_loopback_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .map(|host| host == "localhost" || host == "127.0.0.1" || host == "::1" || host.starts_with("127."))
        .unwrap_or(false)
}

fn create_http_client(url: &str) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(LLM_REQUEST_TIMEOUT)
        .connect_timeout(LLM_CONNECT_TIMEOUT);
    if is_loopback_url(url) {
        builder = builder.no_proxy();
    }
    builder.build()
}

/// 流式请求专用：`timeout()` 是含读完整个响应体的总时长，SSE 长回复会被
/// 中途掐断（表现为 "Stream error: error decoding response body"），
/// 因此这里只设读间隔超时，流只要还在吐数据就不会被断开。
fn create_streaming_http_client(url: &str) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .read_timeout(LLM_STREAM_READ_TIMEOUT)
        .connect_timeout(LLM_CONNECT_TIMEOUT);
    if is_loopback_url(url) {
        builder = builder.no_proxy();
    }
    builder.build()
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
            // 本地模型（如 Ollama）不需要鉴权
            // 不用加 Authorization 头
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

// 遮蔽密钥，只显示末尾 N 个字符
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

// 解析一行 SSE，提取出内容或者工具调用
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
            // Google Gemini 的格式：candidates[0].content.parts[]——每个 part
            // 要么是 {"text": ...}，要么是 {"functionCall": {"name", "args"}}。
            // Gemini 会把函数调用的 `args` 在单个 chunk 里就一次性发完整（不像
            // OpenAI/Anthropic 那样分片增量发送），而且从来不提供 id，所以这里
            // 纯粹为了内部关联而合成一个——它不会被发回给 Google。
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
            // Anthropic 的文本是通过 content_block_delta{delta.type=text_delta}
            // 流式发送的；工具调用则是先来一个 content_block_start{content_block.type=tool_use}
            // （携带 id+name，input 为空），后面跟着一个或多个
            // content_block_delta{delta.type=input_json_delta} 事件（携带 input
            // 对象的 `partial_json` 片段，需要按 `index` 拼接，跟 OpenAI 的参数
            // 片段拼接方式一样）。这一轮真正的流结束信号是 `message_stop` 事件，
            // 而不是 `[DONE]` 哨兵值。
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
            // OpenAI 格式
            if let Some(choices) = json["choices"].as_array() {
                if let Some(first_choice) = choices.first() {
                    if let Some(content) = first_choice["delta"]["content"].as_str() {
                        return Some(StreamContent::Text(content.to_string()));
                    } else if let Some(tool_calls) = first_choice["delta"]["tool_calls"].as_array() {
                        // OpenAI 是增量流式发送工具调用的：某个 `index` 的第一个
                        // delta 携带 `id` + `function.name`，之后同一个 `index`
                        // 的后续每个 delta 只携带 `function.arguments` 的一个片段
                        // （id/name 不再出现——不过有些 OpenAI 兼容的 provider，比如
                        // SiliconFlow，发的是 `""` 而不是直接省略/置 null，所以这里
                        // 必须把空字符串归一化成"不存在"，否则会把第一个 delta 里
                        // 积累的真实值覆盖掉）。这里的每个 delta 都只是一次局部更新，
                        // 不是完整的工具调用——调用方必须在整个流里按 `index` 把
                        // 片段拼接起来。
                        let deltas: Vec<_> = tool_calls.iter().filter_map(|call| {
                            let index = call["index"].as_u64()? as u32;
                            let id = call["id"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());
                            let name = call["function"]["name"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());
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

/// 流式工具调用的一个片段，以 `index` 为键。`id`/`name` 只出现在某个 index
/// 的第一个片段里；`arguments_fragment` 必须把同一个 index 下的所有片段
/// 依次拼接起来。
#[derive(Debug)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    name: Option<String>,
    arguments_fragment: Option<String>,
}

/// 已经完整拼接好、可以直接执行的工具调用。
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

/// 流还在进行中时，用于累积单个工具调用各片段的累加器。`id`/`name` 只会
/// 出现一次；`arguments` 是把这个 index 收到的所有片段按顺序拼接起来构建的。
#[derive(Debug, Default)]
struct PartialToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

// 流式发送消息命令
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

    // 创建一个取消令牌并注册，这样 `cancel_stream` 就能通知这个正在进行
    // 的请求提前停止。
    let cancel_token = CancellationToken::new();
    {
        let mut streams = ACTIVE_STREAMS.lock().await;
        streams.insert(session_id.clone(), cancel_token.clone());
    }

    // 无论函数从哪条路径返回，都要把这个令牌注销掉——用 spawn 是因为 Drop
    // 里没法直接执行异步的加锁操作。
    let _cleanup = scopeguard::guard(session_id.clone(), |sid| {
        tauri::async_runtime::spawn(async move {
            let mut streams = ACTIVE_STREAMS.lock().await;
            streams.remove(&sid);
        });
    });
    
    // 提前把所有已启用 MCP 服务器的工具都取出来——不管 `enable_mcp` 是什么值
    // 都需要，因为哪怕全局 MCP 开关是关的，手动激活的 Skill 仍然可能带上它
    // 自己绑定的服务器的工具进入对话。
    let all_mcp_tools = match get_all_mcp_tools(state.clone()).await {
        Ok(tools) => tools,
        Err(e) => {
            log::warn!("Failed to get MCP tools: {}", e);
            vec![]
        }
    };

    // 加载 skill 列表，并拆分成"本轮手动激活的"和"已启用但交给模型自己
    // 判断要不要调用的"两组。
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

    // 本轮实际暴露出去的工具：全局 MCP 集合（如果启用了）加上手动激活的
    // skill 各自绑定的工具，去重后的结果。
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

    // 把手动激活的 skill 的 instructions（加上可读资源文件的内容）作为一段
    // system prompt 注入进去，是和已有的 system 消息合并，而不是替换掉它。
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
                    videos: vec![],
                });
            }
        }
    }

    let url = build_url(&request.provider, &request.base_url, &request.model, true);
    // 记录 provider/base/model 便于调试（不要记录 API key）
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

    let client = create_streaming_http_client(&url)?;
    let mut body = build_stream_request_body(&request.provider, &request.model, &effective_messages, &mcp_tools, request.enable_thinking, request.max_tokens);
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

    // 主循环
    loop {
        tokio::select! {
            // 检查取消信号
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
            // 从流里读取下一个数据块
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(chunk)) => {
                        let text = String::from_utf8_lossy(&chunk);
                        buffer.push_str(&text);

                        // 处理已经完整的行
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
                                            request.max_tokens,
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
                        // 流结束了，但没有收到明确的"本轮结束"信号
                        // （Google 从来不发这个信号）——按照收到明确的
                        // `StreamContent::Done` 时同样的方式，把目前累积到的
                        // 工具调用做收尾处理。
                        return finalize_turn(
                            &app_handle,
                            state.clone(),
                            &request,
                            &message_id,
                            &effective_messages,
                            &mcp_tools,
                            &all_skills,
                            std::mem::take(&mut tool_call_acc),
                            request.max_tokens,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

/// 执行一轮工具调用（可能是自主的 Skill 调用，也可能是真正的 MCP 工具调用），
/// 按 `tool_calls` 原来的顺序返回它们各自的结果。
async fn execute_tool_calls(
    app_handle: &AppHandle,
    state: tauri::State<'_, DbState>,
    tool_calls: &[ToolCall],
    mcp_tools: &[MCPTool],
    all_skills: &[Skill],
) -> Vec<serde_json::Value> {
    let mut tool_results = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        if let Some(skill_id) = tool_call.function.name.strip_prefix("skill__") {
            // 模型自主调用的 Skill：这里的"工具结果"其实是该 skill 自己的
            // instructions/资源内容，而不是一次真正的 MCP 调用。
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
    tool_results
}

/// 对本轮结束时累积到的工具调用片段做收尾处理（每个 index 的 id/name 取自
/// 该 index 的第一个片段，arguments 是该 index 所有片段拼接的结果）：如果
/// 有工具调用就执行它们，把结果交给模型继续，最后发出终止的 `done: true`
/// 数据块。
///
/// 这个函数同时被"明确的本轮结束信号"（OpenAI 的 `[DONE]`、Anthropic 的
/// `message_stop`）和"流直接关闭、没有任何结束信号"（Google 就是这样）两种
/// 情况共用——两者都需要完全一样的收尾-继续逻辑。
async fn finalize_turn(
    app_handle: &AppHandle,
    state: tauri::State<'_, DbState>,
    request: &SendMessageRequest,
    message_id: &str,
    effective_messages: &[ChatMessage],
    mcp_tools: &[MCPTool],
    all_skills: &[Skill],
    tool_call_acc: std::collections::BTreeMap<u32, PartialToolCall>,
    max_tokens: Option<u32>,
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
        // 模型在同一轮里确实可能合理地需要不止一次工具调用（比如先"列出
        // 允许访问的目录"，再"列出该目录下的文件"）。循环处理，每次续写
        // 请求都把模型自己的工具重新带上，直到它返回纯文本，或者达到轮次
        // 上限为止——没有这个上限的话，一个行为异常的模型可能会无限循环下去。
        const MAX_TOOL_ROUNDS: usize = 5;
        let mut rounds: Vec<(Vec<ToolCall>, Vec<serde_json::Value>)> = Vec::new();
        let mut current_calls = tool_calls;

        for round in 0..MAX_TOOL_ROUNDS {
            let tool_results = execute_tool_calls(app_handle, state.clone(), &current_calls, mcp_tools, all_skills).await;
            rounds.push((current_calls, tool_results));

            match continue_after_tool_calls(
                &request.provider,
                &request.model,
                &request.api_key,
                &request.base_url,
                effective_messages,
                &rounds,
                mcp_tools,
                all_skills,
                max_tokens,
            )
            .await
            {
                Ok(ContinuationResult::Text(live_reply)) => {
                    let _ = app_handle.emit("stream-chunk", StreamChunk {
                        session_id: request.session_id.clone(),
                        message_id: message_id.to_string(),
                        content: live_reply,
                        done: false,
                    });
                    break;
                }
                Ok(ContinuationResult::ToolCalls(next_calls)) => {
                    if round == MAX_TOOL_ROUNDS - 1 {
                        log::warn!("Tool-call round limit ({}) reached, stopping", MAX_TOOL_ROUNDS);
                    } else {
                        current_calls = next_calls;
                        continue;
                    }
                }
                Err(err) => {
                    log::error!("Failed to continue reasoning after tool calls: {}", err);
                }
            }
            break;
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

/// 一次工具调用续写请求可能得到的两种结果：模型已经完成，给出了纯文本回复；
/// 或者它还想继续调用工具（比如"列出允许访问的目录"接着"列出该目录下的
/// 文件"——一轮里的两次调用）。`finalize_turn` 会对后一种情况循环处理。
enum ContinuationResult {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

/// 在一个或多个工具调用执行完之后，发送一次非流式的续写请求，把调用了什么、
/// 返回了什么告诉模型，从而继续这段对话。这里要重新附上模型自己的工具定义
/// （一次全新的 API 调用并不会记得原始请求里的 `tools` 字段），因为没有这些
/// 定义的话，一个想再次调用工具的模型没有原生方式可以这么做，只能试图用
/// 纯文本假装调用一次。每家 provider 对"我调用了什么"/"这是结果"以及工具
/// 调用响应本身的表达形状都不一样，所以请求体构造和响应解析都要按 provider
/// 分支处理。
async fn continue_after_tool_calls(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    original_messages: &[ChatMessage],
    rounds: &[(Vec<ToolCall>, Vec<serde_json::Value>)],
    mcp_tools: &[MCPTool],
    autonomous_skills: &[Skill],
    max_tokens: Option<u32>,
) -> Result<ContinuationResult, LLMError> {
    let url = build_url(provider, base_url, model, false);
    let client = create_http_client(&url)?;

    // 和 `build_stream_request_body` 一样的"空消息"防护：一条因为流在收到
    // 任何 token 之前就被取消而变成空内容的消息，同样不能在这里被当作历史
    // 重放，否则同样的 "message ... must not be empty" 400 会在第一次工具
    // 调用轮次里重新出现。
    let non_empty: Vec<ChatMessage> = original_messages
        .iter()
        .filter(|m| !m.content.trim().is_empty() || !m.images.is_empty() || !m.videos.is_empty())
        .cloned()
        .collect();
    let original_messages = non_empty.as_slice();

    let mut body = match provider {
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

            // Anthropic 要求 tool_use/tool_result 块必须每轮恰好打包成一条
            // assistant 消息和一条 user 消息（它强制要求 user/assistant 严格
            // 交替），不像下面 OpenAI 那种一个结果对应一条工具消息的形状。
            for (tool_calls, tool_results) in rounds {
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
            }

            // 和 build_stream_request_body 一样的原因：Anthropic 强制要求这个
            // 字段，所以用户没填的话就退回到一个足够宽裕的默认值，而不是一个
            // 会截断长回复的数字。
            let max_tokens_val = max_tokens.unwrap_or(32000);
            let mut b = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": max_tokens_val,
                "stream": false,
            });
            if let Some(sys) = system_msg {
                b["system"] = serde_json::json!(sys);
            }
            if !mcp_tools.is_empty() {
                let tools_json: Vec<_> = mcp_tools.iter().map(|tool| {
                    serde_json::json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.input_schema,
                    })
                }).collect();
                b["tools"] = serde_json::json!(tools_json);
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

            for (tool_calls, tool_results) in rounds {
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
                // Gemini REST API 要求 functionResponse 部分的 role 必须是
                // "user"，不能是 "function"——模型角色是 "model"，用户输入是 "user"。
                contents.push(serde_json::json!({ "role": "user", "parts": response_parts }));
            }

            let mut generation_config = serde_json::json!({});
            if let Some(v) = max_tokens {
                generation_config["maxOutputTokens"] = serde_json::json!(v);
            }
            let mut b = serde_json::json!({
                "contents": contents,
                "generationConfig": generation_config,
            });
            if let Some(sys) = system_msg {
                b["systemInstruction"] = serde_json::json!({ "parts": [{ "text": sys }] });
            }
            if !mcp_tools.is_empty() {
                let declarations: Vec<_> = mcp_tools.iter().map(|tool| {
                    serde_json::json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    })
                }).collect();
                b["tools"] = serde_json::json!([{ "functionDeclarations": declarations }]);
            }
            b
        }
        _ => {
            let mut msgs: Vec<serde_json::Value> = original_messages
                .iter()
                .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
                .collect();

            for (tool_calls, tool_results) in rounds {
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
            }

            let mut b = serde_json::json!({
                "model": model,
                "messages": msgs,
                "stream": false,
            });
            if let Some(v) = max_tokens {
                b["max_tokens"] = serde_json::json!(v);
            }
            if !mcp_tools.is_empty() {
                let tools_json: Vec<_> = mcp_tools.iter().map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema
                        }
                    })
                }).collect();
                b["tools"] = serde_json::json!(tools_json);
            }
            b
        }
    };
    append_skill_tools(&mut body, provider, autonomous_skills);

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
        "anthropic" => {
            let blocks = json.get("content").and_then(|c| c.as_array());
            let tool_use_calls: Vec<ToolCall> = blocks
                .map(|arr| {
                    arr.iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .filter_map(|b| {
                            let id = b.get("id")?.as_str()?.to_string();
                            let name = b.get("name")?.as_str()?.to_string();
                            let arguments = b.get("input").map(|i| i.to_string()).unwrap_or_else(|| "{}".to_string());
                            Some(ToolCall { id, function: ToolFunction { name, arguments } })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !tool_use_calls.is_empty() {
                return Ok(ContinuationResult::ToolCalls(tool_use_calls));
            }
            blocks
                .and_then(|arr| arr.iter().find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text")))
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| ContinuationResult::Text(s.to_string()))
                .ok_or_else(|| LLMError::ApiError("LLM did not return content".to_string()))
        }
        "google" => {
            let parts = json
                .get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|parts| parts.as_array());
            let call_parts: Vec<ToolCall> = parts
                .map(|arr| {
                    arr.iter()
                        .filter_map(|part| {
                            let call = part.get("functionCall")?;
                            let name = call.get("name")?.as_str()?.to_string();
                            let arguments = call.get("args").map(|a| a.to_string()).unwrap_or_else(|| "{}".to_string());
                            // Gemini 不会给 functionCall 部分回传 id；
                            // 这里合成一个，好让下游做工具结果匹配时能用上。
                            Some(ToolCall { id: format!("gemini-{}", name), function: ToolFunction { name, arguments } })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !call_parts.is_empty() {
                return Ok(ContinuationResult::ToolCalls(call_parts));
            }
            parts
                .and_then(|arr| arr.first())
                .and_then(|part| part.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| ContinuationResult::Text(s.to_string()))
                .ok_or_else(|| LLMError::ApiError("LLM did not return content".to_string()))
        }
        _ => {
            if let Some(choices) = json["choices"].as_array() {
                if let Some(first_choice) = choices.first() {
                    if let Some(tool_calls) = first_choice["message"]["tool_calls"].as_array() {
                        let calls: Vec<ToolCall> = tool_calls
                            .iter()
                            .filter_map(|tc| {
                                let id = tc.get("id")?.as_str()?.to_string();
                                let name = tc.get("function")?.get("name")?.as_str()?.to_string();
                                let arguments = tc.get("function")?.get("arguments")?.as_str()?.to_string();
                                Some(ToolCall { id, function: ToolFunction { name, arguments } })
                            })
                            .collect();
                        if !calls.is_empty() {
                            return Ok(ContinuationResult::ToolCalls(calls));
                        }
                    }
                    if let Some(text) = first_choice["message"]["content"].as_str() {
                        return Ok(ContinuationResult::Text(text.to_string()));
                    }
                    if let Some(text) = first_choice["text"].as_str() {
                        return Ok(ContinuationResult::Text(text.to_string()));
                    }
                }
            }
            Err(LLMError::ApiError("LLM did not return content".to_string()))
        }
    }
}

/// 模型想要执行的一次工具调用，已经完整解析好、可以直接派发。和上面私有的、
/// 仅用于流式场景的 `ToolCall`/`ToolFunction` 不同：这是 `run_turn`
/// （Workspace Agent 循环）使用的多轮、非流式版本，其中 `arguments` 已经是
/// 解析好的 JSON 值，而不是还需要拼接的字符串片段。
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// 一次 `run_turn` 往返的结果。
#[derive(Debug)]
pub enum TurnOutcome {
    Text(String),
    ToolCalls(Vec<PendingToolCall>),
}

/// 把一段扁平的 `ChatMessage` 历史，构造成 provider 原生格式的"目前为止的
/// 对话" JSON 数组。这是 `build_stream_request_body` 那种内联消息映射的
/// 多轮版本：一个扁平的 `ChatMessage` 列表没法表示 tool_use/tool_result 这
/// 一轮（Anthropic/Google 是用结构化的 content block 来编码的，不是纯文本），
/// 所以需要多轮工具调用的调用方——也就是 Workspace Agent 循环——在这里
/// 一次性构造出原生数组，然后用 `append_tool_round` / `append_text_reply`
/// 在原地随轮次增长，而不是每次都重新推导一遍。
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

/// 把一轮工具调用（模型的调用请求 + 已执行的结果）追加到原生消息数组上，
/// 用的是每家 provider 期望在下一轮历史里看到的那种回放形状。和
/// `continue_after_tool_calls` 里已经确立的各 provider 形状保持一致。
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
            // Gemini REST API 要求 functionResponse 部分的 role 必须是 "user"。
            native_messages.push(serde_json::json!({ "role": "user", "parts": response_parts }));
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

/// 把模型自己的最终纯文本回复追加到原生消息数组上，这样下一次外层调用
/// `run_turn`（比如等到一条新的 Workspace 消息到达时）就能把它当作
/// 之前的 assistant 历史看到。
pub fn append_text_reply(provider: &str, native_messages: &mut Vec<serde_json::Value>, text: &str) {
    match provider {
        "google" => native_messages.push(serde_json::json!({ "role": "model", "parts": [{ "text": text }] })),
        _ => native_messages.push(serde_json::json!({ "role": "assistant", "content": text })),
    }
}

/// 一次非流式的往返：把目前为止的对话 + 可用工具发出去，返回模型的最终
/// 文本回复，或者它想要执行的工具调用。和 `continue_after_tool_calls`
/// （只发一次续写请求，从不重新提供 `tools`，只会返回文本）不同，这个函数
/// 总是会重新提供 `tools`，也可能再次返回 `ToolCalls`——正是这一点让
/// Workspace Agent 循环能够连续多轮调用工具，而不只是一轮。
pub async fn run_turn(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    system_prompt: Option<&str>,
    native_messages: &[serde_json::Value],
    tools: &[MCPTool],
    max_tokens: Option<u32>,
    enable_thinking: bool,
) -> Result<TurnOutcome, LLMError> {
    let url = build_url(provider, base_url, model, false);
    let client = create_http_client(&url)?;
    let body = build_run_turn_body(provider, model, system_prompt, native_messages, tools, max_tokens, enable_thinking);

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

/// `run_turn` 的纯请求体构造逻辑，单独拆出来是为了让每家 provider 的
/// max_tokens/thinking/cache-breakpoint 处理可以在不发起真实 HTTP 往返的
/// 情况下做单元测试——和流式路径里 `build_stream_request_body` 的拆分方式
/// 是一样的。
fn build_run_turn_body(
    provider: &str,
    model: &str,
    system_prompt: Option<&str>,
    native_messages: &[serde_json::Value],
    tools: &[MCPTool],
    max_tokens: Option<u32>,
    enable_thinking: bool,
) -> serde_json::Value {
    match provider {
        "anthropic" => {
            // 和 build_stream_request_body 一样的原因：max_tokens 对 Anthropic
            // 是必填字段，而且 legacy thinking 需要上限超过它自己 8000 token
            // 的 budget。
            let is_legacy_thinking =
                enable_thinking && (model.contains("claude-3") || model.contains("4-5") || model.contains("4.5"));
            let max_tokens_val = match max_tokens {
                Some(v) if is_legacy_thinking => v.max(9000),
                Some(v) => v,
                None => 32000,
            };

            // Prompt caching：把倒数第二条消息的最后一个 content block 标记为
            // 缓存断点，策略和流式路径一样——断点及之前的内容都会在服务端被
            // 缓存，这样随着对话变长，每次只需为最新一轮全价付费。
            // `native_messages` 本身已经会随轮次在原地增长
            // （见 `append_tool_round`/`append_text_reply`），所以这里是每次
            // 唤醒内的每次调用都重新计算一遍，和流式路径每次请求都重新计算
            // 是同样的做法。
            let cache_breakpoint_idx = native_messages.len().checked_sub(2);
            let msgs: Vec<serde_json::Value> = native_messages
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    if cache_breakpoint_idx != Some(i) {
                        return m.clone();
                    }
                    let mut m = m.clone();
                    match m.get("content") {
                        // 普通字符串内容（常见于 build_native_messages/
                        // append_text_reply 生成的消息）——包装成单个文本块，
                        // 这样 cache_control 才有地方可挂；Anthropic 两种形状
                        // 都能接受。
                        Some(serde_json::Value::String(text)) => {
                            m["content"] = serde_json::json!([{
                                "type": "text", "text": text, "cache_control": {"type": "ephemeral"}
                            }]);
                        }
                        // 已经是 block 数组的内容（来自 append_tool_round 的
                        // 一轮 tool_use/tool_result）——直接标记它的最后一个 block。
                        Some(serde_json::Value::Array(_)) => {
                            if let Some(blocks) = m.get_mut("content").and_then(|c| c.as_array_mut()) {
                                if let Some(last) = blocks.last_mut() {
                                    last["cache_control"] = serde_json::json!({"type": "ephemeral"});
                                }
                            }
                        }
                        _ => {}
                    }
                    m
                })
                .collect();

            let mut b = serde_json::json!({
                "model": model, "messages": msgs, "max_tokens": max_tokens_val, "stream": false,
            });
            if enable_thinking {
                if is_legacy_thinking {
                    b["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 8000});
                } else {
                    b["thinking"] = serde_json::json!({"type": "adaptive"});
                }
            }
            if let Some(sys) = system_prompt.filter(|s| !s.trim().is_empty()) {
                // 对同一个 Agent，system prompt 每次请求都完全一样，所以它是
                // 最值得缓存的内容——始终标记它。
                b["system"] = serde_json::json!([{ "type": "text", "text": sys, "cache_control": {"type": "ephemeral"} }]);
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
            let mut generation_config = serde_json::json!({});
            if let Some(v) = max_tokens {
                generation_config["maxOutputTokens"] = serde_json::json!(v);
            }
            if enable_thinking {
                generation_config["thinkingConfig"] = serde_json::json!({"thinkingBudget": 8000});
            }
            let mut b = serde_json::json!({
                "contents": native_messages, "generationConfig": generation_config,
            });
            if let Some(sys) = system_prompt.filter(|s| !s.trim().is_empty()) {
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
            if let Some(sys) = system_prompt.filter(|s| !s.trim().is_empty()) {
                all_messages.push(serde_json::json!({ "role": "system", "content": sys }));
            }
            all_messages.extend_from_slice(native_messages);
            let mut b = serde_json::json!({ "model": model, "messages": all_messages, "stream": false });
            if let Some(v) = max_tokens {
                b["max_tokens"] = serde_json::json!(v);
            }
            if enable_thinking && provider == "siliconflow" {
                b["enable_thinking"] = serde_json::json!(true);
                b["thinking_budget"] = serde_json::json!(8000);
            }
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
    // 本地模型不需要 API key
    if request.provider == "local" {
        return Ok(String::new());
    }
    if !request.api_key.is_empty() {
        return Ok(request.api_key.clone());
    }
    // 没有传 api_key —— 退回到以 provider 为键的系统 keyring 查找。
    // 前端调用 save_api_key(provider, key) 时，keyring 里的标签就是
    // "api_keys_{provider}"。这样一来，只要密钥已经存在 keyring 里，
    // 调用方就可以逐步不再在 IPC 请求里嵌入明文密钥。
    if !request.provider.is_empty() {
        let label = format!("api_keys_{}", request.provider);
        if let Ok(entry) = KeyringEntry::new("BaiyuAISpace", &label) {
            if let Ok(key) = entry.get_password() {
                if !key.is_empty() {
                    log::info!("[LLM] api_key resolved from keyring ({})", label);
                    return Ok(key);
                }
            }
        }
    }
    Err(LLMError::MissingApiKey)
}

/// 取消某个会话正在进行的流
#[tauri::command]
pub async fn cancel_stream(session_id: String) -> Result<(), String> {
    let streams = ACTIVE_STREAMS.lock().await;
    if let Some(token) = streams.get(&session_id) {
        token.cancel();
        log::info!("Cancelled stream for session: {}", session_id);
    } else {
        // 用户点击停止到这条命令实际执行之间，流可能已经自然结束了——
        // 这不算错误情况。
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
            timestamp: 0, error: None, images: vec![], videos: vec![],
        }];
        let body = build_stream_request_body("anthropic", "claude-3-5-sonnet", &messages, &[sample_tool()], false, None);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert!(tools[0].get("input_schema").is_some(), "anthropic tools use `input_schema`, not `parameters`");
        assert!(tools[0].get("function").is_none(), "anthropic tools must not be nested under `function` like OpenAI");
    }

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            id: content.into(), role: role.into(), content: content.into(),
            timestamp: 0, error: None, images: vec![], videos: vec![],
        }
    }

    #[test]
    fn anthropic_prompt_caching_marks_system_and_last_history_message() {
        let messages = vec![
            msg("system", "you are a helpful assistant"),
            msg("user", "turn 1"),
            msg("assistant", "reply 1"),
            msg("user", "turn 2 (newest)"),
        ];
        let body = build_stream_request_body("anthropic", "claude-opus-4-8", &messages, &[], false, None);

        // 对这个配置来说，system prompt 每次请求都是稳定不变的，所以始终值得缓存。
        let system = body["system"].as_array().expect("system should be a content-block array");
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");

        // 断点落在"最新一条之前的那条消息"上——这里是 "reply 1"——所以最新的
        // 这一轮（"turn 2"）是唯一没被缓存的内容。
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "system message is excluded from `messages`");
        assert_eq!(msgs[0]["content"][0]["text"], "turn 1");
        assert!(msgs[0]["content"][0].get("cache_control").is_none());
        assert_eq!(msgs[1]["content"][0]["text"], "reply 1");
        assert_eq!(msgs[1]["content"][0]["cache_control"]["type"], "ephemeral", "breakpoint belongs on the last historical message");
        assert_eq!(msgs[2]["content"][0]["text"], "turn 2 (newest)");
        assert!(msgs[2]["content"][0].get("cache_control").is_none(), "the newest turn changes every request, so caching it would never hit");
    }

    #[test]
    fn anthropic_prompt_caching_skips_breakpoint_on_first_message() {
        // 只有一条第一条消息时，还没有可重复的历史——没什么好缓存的。
        let messages = vec![msg("user", "hello")];
        let body = build_stream_request_body("anthropic", "claude-opus-4-8", &messages, &[], false, None);
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn google_request_body_groups_tools_under_function_declarations() {
        let messages = vec![ChatMessage {
            id: "1".into(), role: "user".into(), content: "hi".into(),
            timestamp: 0, error: None, images: vec![], videos: vec![],
        }];
        let body = build_stream_request_body("google", "gemini-1.5-pro", &messages, &[sample_tool()], false, None);
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
            timestamp: 0, error: None, images: vec![], videos: vec![],
        }];
        let body = build_stream_request_body("openai", "gpt-4o", &messages, &[sample_tool()], false, None);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
    }

    fn image_message() -> ChatMessage {
        ChatMessage {
            id: "1".into(), role: "user".into(), content: "what is this".into(),
            timestamp: 0, error: None,
            images: vec![ImageAttachment { data: "AAAA".into(), media_type: "image/png".into() }],
            videos: vec![],
        }
    }

    #[test]
    fn openai_compatible_image_url_is_nested_under_url_object() {
        let messages = vec![image_message()];
        // 除了 Mistral 之外，每家"OpenAI 兼容"的 provider（DeepSeek、SiliconFlow、
        // Zhipu 的 OpenAI 兼容端点、Aliyun、Baidu、Doubao、Moonshot、MiniMax……）
        // 都期望标准 OpenAI 的 `image_url: {"url": "data:..."}` 对象形状。
        for provider in ["openai", "deepseek", "siliconflow", "zhipu", "aliyun", "moonshot"] {
            let body = build_stream_request_body(provider, "some-model", &messages, &[], false, None);
            let image_url = &body["messages"][0]["content"][1]["image_url"];
            assert!(image_url.is_object(), "{provider}: image_url should be an object with a `url` key, got {image_url:?}");
            assert_eq!(image_url["url"], "data:image/png;base64,AAAA");
        }
    }

    #[test]
    fn mistral_image_url_is_a_bare_data_uri_string_not_an_object() {
        // Mistral 的 Chat Completions API 把 data URI 直接当作 `image_url`
        // 的值——已经对照 docs.mistral.ai/capabilities/vision 确认过。
        // 这里如果发嵌套对象形状，服务端会解析失败。
        let messages = vec![image_message()];
        let body = build_stream_request_body("mistral", "pixtral-large-latest", &messages, &[], false, None);
        let image_url = &body["messages"][0]["content"][1]["image_url"];
        assert!(image_url.is_string(), "mistral: image_url should be a bare string, got {image_url:?}");
        assert_eq!(image_url, "data:image/png;base64,AAAA");
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
        // Gemini REST API 拒绝把 "function" 当作 role——functionResponse
        // 部分必须用 "user"（见 append_tool_round 的 google 分支）。
        assert_eq!(native[2]["role"], "user");
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
            ChatMessage { id: "0".into(), role: "system".into(), content: "be nice".into(), timestamp: 0, error: None, images: vec![], videos: vec![] },
            ChatMessage { id: "1".into(), role: "user".into(), content: "hi".into(), timestamp: 0, error: None, images: vec![], videos: vec![] },
            ChatMessage { id: "2".into(), role: "assistant".into(), content: "hello".into(), timestamp: 0, error: None, images: vec![], videos: vec![] },
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

    // -----------------------------------------------------------------
    // `continue_after_tool_calls` 是*常规聊天流程*（不是上面用
    // `append_tool_round` 的 Workspace Agent 循环）在 MCP 工具调用之后发给
    // 模型的内容。上面的测试都没有覆盖到它。下面这些测试用一个真实的本地
    // HTTP 服务器充当 provider，配合一个真实的 MCP 工具结果（而不是手写的
    // 测试夹具），来证明工具的输出确实以模型期望的形状发到了网络上，并且
    // 模型的最终回复能正确地流回来。
    // -----------------------------------------------------------------

    /// 极简的 HTTP/1.1 服务器，用来充当 OpenAI 兼容端点。依次接受
    /// `responses.len()` 个连接，捕获每个请求解析后的 JSON body，并回复
    /// 对应的预置 JSON 响应。
    async fn mock_llm_server(responses: Vec<serde_json::Value>) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock server");
        let addr = listener.local_addr().expect("local_addr");
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => return,
                };

                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let headers_end = loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(pos);
                    }
                };
                let Some(headers_end) = headers_end else { continue };

                let header_str = String::from_utf8_lossy(&buf[..headers_end]).to_string();
                let content_length: usize = header_str
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            v.trim().parse().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);

                let mut body = buf[headers_end + 4..].to_vec();
                while body.len() < content_length {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..n]);
                }

                if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&body) {
                    captured_clone.lock().await.push(parsed);
                }

                let resp_str = response.to_string();
                let http_response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_str.as_bytes().len(),
                    resp_str
                );
                let _ = socket.write_all(http_response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{}", addr), captured)
    }

    #[tokio::test]
    async fn continue_after_tool_calls_embeds_real_mcp_result_and_returns_final_text() {
        // 使用真实的生产环境工具执行路径（而不是手写的测试夹具），这样这个
        // 测试才能捕捉到 MCP 工具实际返回内容的漂移，而不只是我们以为它会
        // 返回什么。
        let real_result = crate::commands::mcp::handle_demo_tool_call(
            "demo_calculator",
            serde_json::json!({ "a": 3, "b": 4, "operation": "add" }),
            "test-request",
        )
        .await
        .expect("demo tool should succeed");
        assert_eq!(real_result["result"], 7.0, "sanity: demo_calculator actually computed 3+4");

        let (base_url, captured) = mock_llm_server(vec![
            serde_json::json!({ "choices": [{ "message": { "content": "3加4等于7。" } }] }),
        ])
        .await;

        let original_messages = vec![
            msg("system", "你是一个助手"),
            msg("user", "3加4等于多少？用计算器工具算一下"),
        ];
        let calls = vec![ToolCall {
            id: "call_1".to_string(),
            function: ToolFunction {
                name: "demo_calculator".to_string(),
                arguments: serde_json::json!({ "a": 3, "b": 4, "operation": "add" }).to_string(),
            },
        }];
        let rounds = vec![(calls, vec![real_result.clone()])];

        let outcome = continue_after_tool_calls(
            "custom", "test-model", "test-key", &base_url,
            &original_messages, &rounds, &[], &[], None,
        ).await.expect("continuation call should succeed");

        match outcome {
            ContinuationResult::Text(text) => assert_eq!(text, "3加4等于7。"),
            ContinuationResult::ToolCalls(_) => panic!("expected final text, got another tool-call request"),
        }

        // 精确检查第二轮请求实际发到网络上的内容——这才是证明工具的真实输出
        // 确实到达了模型上下文，而不只是我们的代码自己以为它到达了。
        let sent = captured.lock().await;
        let sent_messages = sent[0]["messages"].as_array().expect("messages array");

        let assistant_msg = sent_messages
            .iter()
            .find(|m| m["role"] == "assistant" && m["tool_calls"].is_array())
            .expect("assistant tool_calls message present");
        assert_eq!(assistant_msg["tool_calls"][0]["id"], "call_1");
        assert_eq!(assistant_msg["tool_calls"][0]["function"]["name"], "demo_calculator");

        let tool_msg = sent_messages
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("tool result message present");
        assert_eq!(tool_msg["tool_call_id"], "call_1");
        let embedded_result: serde_json::Value = serde_json::from_str(
            tool_msg["content"].as_str().expect("tool content must be a string for OpenAI-shape providers"),
        ).expect("tool content must be valid JSON");
        assert_eq!(embedded_result, real_result, "the exact MCP tool result must round-trip into the model context untouched");
    }

    #[tokio::test]
    async fn continue_after_tool_calls_supports_a_second_round_after_more_tool_calls() {
        // 模型在看到第一个结果之后，确实可能合理地要求再调用一个工具
        // （finalize_turn 会循环最多 MAX_TOOL_ROUNDS 次）；这里验证第二次
        // 续写请求能正确把两轮结果都叠加进去。
        let result_1 = crate::commands::mcp::handle_demo_tool_call(
            "demo_calculator", serde_json::json!({"a": 3, "b": 4, "operation": "add"}), "r1",
        ).await.unwrap();
        let result_2 = crate::commands::mcp::handle_demo_tool_call(
            "demo_calculator", serde_json::json!({"a": 7, "b": 5, "operation": "multiply"}), "r2",
        ).await.unwrap();

        let (base_url, captured) = mock_llm_server(vec![
            serde_json::json!({
                "choices": [{ "message": { "content": null, "tool_calls": [
                    { "id": "call_2", "type": "function", "function": { "name": "demo_calculator", "arguments": "{\"a\":7,\"b\":5,\"operation\":\"multiply\"}" } }
                ] } }]
            }),
            serde_json::json!({ "choices": [{ "message": { "content": "3+4=7，再乘以5等于35。" } }] }),
        ])
        .await;

        let original_messages = vec![msg("user", "先加后乘")];
        let call_1 = ToolCall {
            id: "call_1".into(),
            function: ToolFunction { name: "demo_calculator".into(), arguments: "{\"a\":3,\"b\":4,\"operation\":\"add\"}".into() },
        };
        let mut rounds = vec![(vec![call_1], vec![result_1])];

        let outcome = continue_after_tool_calls("custom", "test-model", "test-key", &base_url, &original_messages, &rounds, &[], &[], None)
            .await
            .expect("round 1 continuation");
        let next_calls = match outcome {
            ContinuationResult::ToolCalls(calls) => calls,
            ContinuationResult::Text(_) => panic!("expected another tool call round"),
        };
        assert_eq!(next_calls[0].id, "call_2");

        rounds.push((next_calls, vec![result_2]));
        let outcome_2 = continue_after_tool_calls("custom", "test-model", "test-key", &base_url, &original_messages, &rounds, &[], &[], None)
            .await
            .expect("round 2 continuation");
        match outcome_2 {
            ContinuationResult::Text(text) => assert_eq!(text, "3+4=7，再乘以5等于35。"),
            ContinuationResult::ToolCalls(_) => panic!("expected final text after 2 rounds"),
        }

        let sent = captured.lock().await;
        let second_request_messages = sent[1]["messages"].as_array().expect("messages array");
        let tool_msgs: Vec<_> = second_request_messages.iter().filter(|m| m["role"] == "tool").collect();
        assert_eq!(tool_msgs.len(), 2, "both rounds' tool results must both be present in the second follow-up request");
        assert_eq!(tool_msgs[0]["tool_call_id"], "call_1");
        assert_eq!(tool_msgs[1]["tool_call_id"], "call_2");
    }

    fn native_msg(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "content": text })
    }

    #[test]
    fn run_turn_body_max_tokens_defaults_match_streaming_path_per_provider() {
        let msgs = vec![native_msg("user", "hi")];
        // Anthropic 强制要求这个字段——未设置时退回到一个宽裕的默认值。
        let anthropic = build_run_turn_body("anthropic", "claude-3-5-sonnet", None, &msgs, &[], None, false);
        assert_eq!(anthropic["max_tokens"], 32000);
        let anthropic_set = build_run_turn_body("anthropic", "claude-3-5-sonnet", None, &msgs, &[], Some(1000), false);
        assert_eq!(anthropic_set["max_tokens"], 1000);

        // Google/OpenAI 兼容：未设置时直接省略字段，而不是猜一个会截断长回复
        // 的小上限。
        let google = build_run_turn_body("google", "gemini-2.5-pro", None, &msgs, &[], None, false);
        assert!(google["generationConfig"].get("maxOutputTokens").is_none());
        let google_set = build_run_turn_body("google", "gemini-2.5-pro", None, &msgs, &[], Some(2048), false);
        assert_eq!(google_set["generationConfig"]["maxOutputTokens"], 2048);

        let openai = build_run_turn_body("deepseek", "deepseek-chat", None, &msgs, &[], None, false);
        assert!(openai.get("max_tokens").is_none());
        let openai_set = build_run_turn_body("deepseek", "deepseek-chat", None, &msgs, &[], Some(4096), false);
        assert_eq!(openai_set["max_tokens"], 4096);
    }

    #[test]
    fn run_turn_body_legacy_thinking_forces_anthropic_max_tokens_floor() {
        let msgs = vec![native_msg("user", "hi")];
        // Claude 3.x 的旧版 thinking 要求 max_tokens 必须大于 8000，budget_tokens
        // 才有效——一个较小的显式值必须被拉高，不能原样采用。
        let body = build_run_turn_body("anthropic", "claude-3-5-sonnet", None, &msgs, &[], Some(2000), true);
        assert_eq!(body["max_tokens"], 9000);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 8000);

        // 更新的（非 legacy）模型改用 adaptive 形式，没有 budget_tokens。
        let adaptive = build_run_turn_body("anthropic", "claude-opus-4-6", None, &msgs, &[], Some(2000), true);
        assert_eq!(adaptive["max_tokens"], 2000, "adaptive thinking doesn't force the 9000 floor");
        assert_eq!(adaptive["thinking"]["type"], "adaptive");
    }

    #[test]
    fn run_turn_body_thinking_only_applied_to_siliconflow_among_openai_compatible() {
        let msgs = vec![native_msg("user", "hi")];
        let sf = build_run_turn_body("siliconflow", "qwen3", None, &msgs, &[], None, true);
        assert_eq!(sf["enable_thinking"], true);
        assert_eq!(sf["thinking_budget"], 8000);

        // 其他每一家 OpenAI 兼容的 provider 都是静默不处理，而不是发送一个
        // API 根本不认识的字段。
        let deepseek = build_run_turn_body("deepseek", "deepseek-chat", None, &msgs, &[], None, true);
        assert!(deepseek.get("enable_thinking").is_none());

        let google = build_run_turn_body("google", "gemini-2.5-pro", None, &msgs, &[], None, true);
        assert_eq!(google["generationConfig"]["thinkingConfig"]["thinkingBudget"], 8000);
    }

    #[test]
    fn run_turn_body_anthropic_marks_cache_breakpoint_on_second_to_last_message() {
        // 3 条消息：断点应该落在 index 1（len - 2）上，把它原本的纯字符串内容
        // 包装成一个带缓存标记的 block；最新的消息（index 2）和最早的第一条
        // （index 0）都必须保持不被标记。
        let msgs = vec![native_msg("user", "first"), native_msg("assistant", "second"), native_msg("user", "third")];
        let body = build_run_turn_body("anthropic", "claude-3-5-sonnet", Some("be helpful"), &msgs, &[], None, false);
        let sent_msgs = body["messages"].as_array().unwrap();

        assert_eq!(sent_msgs[0]["content"], "first", "message before the breakpoint stays a plain string");
        assert_eq!(sent_msgs[1]["content"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(sent_msgs[1]["content"][0]["text"], "second");
        assert_eq!(sent_msgs[2]["content"], "third", "newest message must not be cache-marked");

        // system prompt 只要存在，就一定会被标记缓存。
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][0]["text"], "be helpful");
    }

    #[test]
    fn run_turn_body_anthropic_cache_breakpoint_marks_last_block_of_tool_round_message() {
        // 一条已经是 content block 形状的消息（比如 append_tool_round 追加的
        // 一轮 tool_result）必须只标记它*最后*一个 block，而不能把整个 content
        // 替换掉。
        let tool_result_msg = serde_json::json!({
            "role": "user",
            "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "42" }]
        });
        let msgs = vec![native_msg("user", "first"), tool_result_msg, native_msg("assistant", "reply")];
        let body = build_run_turn_body("anthropic", "claude-3-5-sonnet", None, &msgs, &[], None, false);
        let sent_msgs = body["messages"].as_array().unwrap();

        let marked_block = &sent_msgs[1]["content"][0];
        assert_eq!(marked_block["type"], "tool_result");
        assert_eq!(marked_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn run_turn_body_tools_use_correct_shape_per_provider() {
        let msgs = vec![native_msg("user", "hi")];
        let tool = sample_tool();

        let anthropic = build_run_turn_body("anthropic", "claude-3-5-sonnet", None, &msgs, &[tool.clone()], None, false);
        assert!(anthropic["tools"][0].get("input_schema").is_some());

        let google = build_run_turn_body("google", "gemini-2.5-pro", None, &msgs, &[tool.clone()], None, false);
        assert!(google["tools"][0]["functionDeclarations"][0].get("parameters").is_some());

        let openai = build_run_turn_body("openai", "gpt-4o", None, &msgs, &[tool], None, false);
        assert_eq!(openai["tools"][0]["type"], "function");
        assert!(openai["tools"][0]["function"].get("parameters").is_some());
    }
}
