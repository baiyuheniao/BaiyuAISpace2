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

use crate::commands::constants::{
    DEFAULT_LLM_RETRY_COUNT, DEFAULT_LLM_RETRY_INTERVAL_SECS, LLM_CONNECT_TIMEOUT,
    LLM_REQUEST_TIMEOUT, LLM_STREAM_READ_TIMEOUT,
};
use crate::commands::mcp::{get_all_mcp_tools, call_mcp_tool_with_context, BuiltinToolContext, MCPTool};
use crate::commands::file_tools::{self, FileAccessRule};
use crate::commands::skills::{read_skill_resource_text, Skill};
use crate::db::DbState;
use keyring::Entry as KeyringEntry;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// 普通 Chat 的保护线。这里是确定性压缩，不会再调用一个模型产生费用或新失败链路。
const DEFAULT_MAX_TOOL_ROUNDS: usize = 20;
const MAX_TOOL_ROUNDS: usize = 100;
const TOOL_RESULT_TOKEN_BUDGET: usize = 6_000;
const TOOL_ROUND_TOKEN_BUDGET: usize = 12_000;
const TOOL_RESULT_CHAR_BUDGET: usize = TOOL_RESULT_TOKEN_BUDGET * 4;

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
    #[serde(default)]
    pub token_usage: Option<TokenUsage>,
    /// 本条助手回复触发的工具调用记录（前端 `ToolCallInfo[]` 的 JSON）。后端
    /// 不解析其内部结构，只负责原样落库、原样返回，这样界面展示字段以后增减
    /// 都不用改后端。用户消息和历史上的旧消息没有这个字段。
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
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
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub file_access_mode: String,
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
    /// API 密钥。必须有 default：settings 持久化时会把 apiKey 字段剥掉（密钥
    /// 只进系统 keyring），重启后本地 provider 的配置在 keyring 里没有条目可
    /// 回填，前端会发出不带 apiKey 的请求——没有 default 时 serde 直接以
    /// "missing field `apiKey`" 拒收整个请求，本地模型重启后就全用不了。
    /// 空串对本地 provider 本来就是合法值（不鉴权），对其他 provider 则会
    /// 走 get_api_key 的 keyring 兜底查找。
    /// 仅传递配置 ID；明文密钥只由 Rust 从系统 keyring 读取。
    #[serde(default)]
    pub api_config_id: String,
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
    /// 遇到限流/过载类错误时的自动重试次数（None 时用 DEFAULT_LLM_RETRY_COUNT）
    #[serde(default)]
    pub retry_count: Option<u32>,
    /// 每次重试之间的等待秒数（None 时用 DEFAULT_LLM_RETRY_INTERVAL_SECS）
    #[serde(default)]
    pub retry_interval_secs: Option<u32>,
    /// 普通 Chat 每次回答允许连续调用工具的最大轮数。前端设置缺失或异常时后端仍兜底。
    #[serde(default)]
    pub max_tool_rounds: Option<u32>,
    /// 当前会话的文件授权由会话配置持久化；请求携带副本用于本轮工具调用。
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub file_access_mode: String,
    #[serde(default)]
    pub terminal_shell: String,
}

/// 手动压缩上下文的请求。它不携带明文密钥，仍由 Rust 通过配置 ID 从 keyring 读取。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactChatContextRequest {
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_config_id: String,
    pub base_url: String,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 用户可选的压缩重点，例如“保留认证改动和当前报错”。
    #[serde(default)]
    pub focus: String,
}

/// 前端只需要知道摘要和从哪条消息开始仍保留原文；完整聊天记录从不删除。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactChatContextResult {
    pub summary: String,
    pub preserve_from_timestamp: i64,
}

/// 数据库中的“活动上下文”快照；原消息仍保留在 messages 表内供查看和导出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompaction {
    pub summary: String,
    pub preserve_from_timestamp: i64,
    pub created_at: i64,
}

/// 工具调用状态事件结构（前端据此展示"正在调用工具/工具调用结果"）
#[derive(Clone, Serialize)]
pub struct ToolCallEvent {
    /// 会话 ID
    pub session_id: String,
    /// 消息 ID
    pub message_id: String,
    /// 工具调用 ID（同一次调用的 calling/done 事件用它配对）
    pub call_id: String,
    /// 工具名称
    pub tool_name: String,
    /// 调用参数（JSON 字符串）
    pub arguments: String,
    /// 状态："calling" | "done" | "error"
    pub status: String,
    /// 调用结果（仅 done/error 状态携带，JSON 字符串）
    #[serde(default)]
    pub result: Option<String>,
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
    /// 是否为思考过程增量。思考型模型（DeepSeek R1 系、Ollama 上的 qwen3.5
    /// 等）会把思考内容放在 reasoning_content/reasoning 字段流式返回，前端
    /// 据此把这部分归到"思考过程"折叠区，而不是混进正文。
    pub is_thinking: bool,
    /// 是否完成
    pub done: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageEvent {
    pub session_id: String,
    pub message_id: String,
    pub usage: TokenUsage,
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
    /// 一次非流式往返（包括全部重试和响应体读取）超过总预算
    #[error("LLM 请求总时长超过限制（{0}）")]
    TotalTimeout(String),
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
/// 第三个字段仅供人工参考，实际请求头由 build_headers() 按 provider 名
/// 单独匹配决定（google/azure/anthropic 各有专属分支，其余走 Bearer）。
/// - bearer: 使用 Authorization: Bearer token
/// - x_api_key: 使用 x-api-key 头
/// - api_key: 使用 api-key 头（Azure）
/// - x_goog_api_key: 使用 x-goog-api-key 头（Google）
const PROVIDER_CONFIGS: &[(&str, &str, &str)] = &[
    ("openai", "https://api.openai.com/v1/chat/completions", "bearer"),
    ("anthropic", "https://api.anthropic.com/v1/messages", "x_api_key"),
    ("google", "https://generativelanguage.googleapis.com/v1beta/models/", "x_goog_api_key"),
    ("azure", "", "api_key"),
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

/// 判断某个 provider 的「思考模式」开关是否真的会改变请求行为。
///
/// 必须与 `build_stream_request_body` / `build_run_turn_body` 里实际发送思考参数
/// 的分支保持同步：只有这些 provider 的 `enable_thinking` 会落到请求里，其余
/// provider（OpenAI / Azure / Mistral / Moonshot / 智谱 / 通义 / 文心 / 豆包 /
/// DeepSeek / MiniMax / 零一万物等）的思考开关是空操作——前端据此隐藏按钮，
/// 避免用户点了个没反应的开关。
///
/// - anthropic / google：发送 thinking 配置
/// - siliconflow：发送 enable_thinking + thinking_budget
/// - local / custom / openclaw：关掉时发送 reasoning_effort=none（关思考生效）
#[tauri::command]
pub fn supports_thinking(provider: String) -> bool {
    matches!(
        provider.as_str(),
        "anthropic" | "google" | "siliconflow" | "local" | "custom" | "openclaw"
    )
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
                "stream_options": { "include_usage": true },
            });

            // 未设置时直接省略字段，而不是拿一个猜测值去顶替；这些 provider 并不
            // 强制要求这个字段，而一个写死的小数值会让所有没填这项的用户的长回复
            // 被悄悄截断。
            if let Some(v) = max_tokens {
                body["max_tokens"] = serde_json::json!(v);
            }

            // SiliconFlow 的推理开关默认是开启的。关闭时也必须显式传 false，
            // 否则 Qwen3.5 会只返回 reasoning_content，压缩等只需要正文的调用会失败。
            if provider == "siliconflow" {
                body["enable_thinking"] = serde_json::json!(enable_thinking);
                if enable_thinking {
                    body["thinking_budget"] = serde_json::json!(8000);
                }
            }

            // 本地 OpenAI 兼容服务：思考开关关闭时，显式发送 reasoning_effort=none
            // 关闭思考。此前这个开关对 local 是摆设——qwen3.5 这类默认思考的模型
            // 在 LM Studio 上每条消息都要先想几百上千个 token 才吐正文（实测
            // chat_template_kwargs 和 /no_think 都关不掉，只有 reasoning_effort
            // 有效）。范围只限自托管服务：Ollama 等不认识该字段的会静默忽略；
            // 不发给远端商业 provider（Mistral 等严格实现会拒收未知字段）。
            if !enable_thinking && matches!(provider, "local" | "custom" | "openclaw") {
                body["reasoning_effort"] = serde_json::json!("none");
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

/// 判断服务商返回的非 2xx 响应是不是"稍后重试大概率会成功"的临时性错误：
/// 429/503/529（Anthropic 的过载码）本身就是明确信号；有些 provider 把过载/
/// 限流塞进 200 以外的状态码但错误文本里带关键字（比如题述的
/// `engine_overloaded_error`），一并按文本兜底识别。
fn is_retryable_status(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 529 || status.is_server_error() {
        return true;
    }
    let lower = body.to_lowercase();
    lower.contains("overloaded") || lower.contains("rate_limit") || lower.contains("rate limit")
}

fn is_retryable_reqwest_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect()
}

/// 带自动重试地发送一个已经构造好（`.json(body)` 等已调用完）的请求。只
/// 覆盖"请求还没等到响应/服务商直接拒绝"这个阶段——调用方必须保证传入的
/// 是流式请求真正开始读取 SSE 内容之前的那次 `send()`，不能是流已经吐出部分
/// 内容之后的重试，否则会在前端产生重复/错乱的部分回复。
async fn send_with_retry(
    request_builder: &reqwest::RequestBuilder,
    retry_count: u32,
    retry_interval_secs: u32,
    cancel_token: Option<&CancellationToken>,
) -> Result<reqwest::Response, LLMError> {
    let mut attempt = 0u32;
    loop {
        let builder = request_builder.try_clone().ok_or_else(|| {
            LLMError::ApiError("internal error: request body not clonable for retry".to_string())
        })?;
        let send_result = match cancel_token {
            Some(token) => {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        return Err(LLMError::StreamError("请求已取消".to_string()));
                    }
                    result = builder.send() => result,
                }
            }
            None => builder.send().await,
        };
        match send_result {
            Ok(response) => {
                if response.status().is_success() {
                    return Ok(response);
                }
                let status = response.status();
                let error_text = response.text().await.unwrap_or_else(|_| "unknown".to_string());
                if attempt >= retry_count || !is_retryable_status(status, &error_text) {
                    return Err(LLMError::ApiError(error_text));
                }
                log::warn!(
                    "LLM 请求被服务商拒绝，判定为可重试错误（状态码 {}，第 {}/{} 次重试）：{}",
                    status, attempt + 1, retry_count, error_text
                );
            }
            Err(e) => {
                if attempt >= retry_count || !is_retryable_reqwest_error(&e) {
                    return Err(e.into());
                }
                log::warn!(
                    "LLM 请求发送失败，判定为可重试错误（第 {}/{} 次重试）：{}",
                    attempt + 1, retry_count, e
                );
            }
        }
        attempt += 1;
        let wait = tokio::time::sleep(Duration::from_secs(retry_interval_secs as u64));
        match cancel_token {
            Some(token) => {
                tokio::select! {
                    _ = wait => {}
                    _ = token.cancelled() => {
                        return Err(LLMError::StreamError("请求已取消".to_string()));
                    }
                }
            }
            None => wait.await,
        }
    }
}

/// 给一整段非流式请求流程加总时长预算。`reqwest::Client::timeout` 对每一次
/// 重试分别计时；如果直接把 600 秒客户端复用 4 次，默认 3 次重试会把用户
/// 实际等待放大到约 40 分钟。这里的外层预算覆盖首次请求、重试间隔、所有
/// 重试以及成功响应的 body 读取，确保常量表达的确实是整轮总上限。
async fn with_total_timeout<T, F>(duration: Duration, future: F) -> Result<T, LLMError>
where
    F: std::future::Future<Output = Result<T, LLMError>>,
{
    match tokio::time::timeout(duration, future).await {
        Ok(result) => result,
        Err(_) => Err(LLMError::TotalTimeout(format!("{:?}", duration))),
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
                        // Extended Thinking 的思考增量。此前被静默丢弃——对
                        // Anthropic 影响不大（思考后总会有 text_delta 正文），
                        // 但既然前端有了思考过程展示区，一并转发保持一致。
                        Some("thinking_delta") => delta
                            .get("thinking")
                            .and_then(|t| t.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| StreamContent::Thinking(s.to_string())),
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
            if let Some(usage) = parse_openai_usage(&json) {
                return Some(StreamContent::Usage(usage));
            }
            if let Some(choices) = json["choices"].as_array() {
                if let Some(first_choice) = choices.first() {
                    let delta = &first_choice["delta"];

                    // 必须先看 tool_calls、再看 content：Ollama 等实现会在携带
                    // tool_calls 的 chunk 里同时带上 `"content": ""`（OpenAI 官方
                    // 是 null）。如果先按 content 分支处理，空字符串照样命中
                    // as_str()，同一个 chunk 里的 tool_calls 就被整个吞掉——
                    // 表现为模型明明发起了工具调用，前端却毫无反应、收到空回复。
                    if let Some(tool_calls) = delta["tool_calls"].as_array() {
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

                    if let Some(content) = delta["content"].as_str() {
                        if !content.is_empty() {
                            return Some(StreamContent::Text(content.to_string()));
                        }
                    }

                    // 思考型模型的思考增量：DeepSeek/SiliconFlow 系用
                    // reasoning_content，Ollama 用 reasoning。这类 chunk 的
                    // content 恒为空串/null——丢弃它们的话，思考占比高的模型
                    // （比如 Ollama 上的 qwen3.5）在用户看来就是等了半分钟
                    // 收到一条空回复。单独作为思考过程转发，不混入正文。
                    if let Some(reasoning) = delta["reasoning_content"]
                        .as_str()
                        .or_else(|| delta["reasoning"].as_str())
                    {
                        if !reasoning.is_empty() {
                            return Some(StreamContent::Thinking(reasoning.to_string()));
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
    /// 思考型模型的思考过程增量（reasoning/reasoning_content/thinking_delta）
    Thinking(String),
    ToolCallDeltas(Vec<ToolCallDelta>),
    Usage(TokenUsage),
    Done,
}

fn parse_openai_usage(json: &serde_json::Value) -> Option<TokenUsage> {
    let usage = json.get("usage")?;
    Some(TokenUsage {
        prompt_tokens: usage.get("prompt_tokens")?.as_u64()?,
        completion_tokens: usage.get("completion_tokens")?.as_u64()?,
        total_tokens: usage.get("total_tokens")?.as_u64()?,
    })
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

/// 一轮流式请求（首轮请求，或工具调用之后的续写请求）读完后的产出。
#[derive(Debug, Default)]
struct StreamedTurn {
    /// 这一轮流出的正文（不含思考过程）。模型常会在发起工具调用的同时先说
    /// 一段话，这段话必须保留：既要显示给用户，也要在下一轮请求里作为它自己
    /// 说过的话带回去。
    text: String,
    /// 这一轮模型要求执行的工具调用。为空说明这一轮就是最终回答。
    tool_calls: Vec<ToolCall>,
    /// 服务端在流末尾回报的 token 用量（不是每家 provider 都会回报）。
    usage: Option<TokenUsage>,
    /// 这一轮是否收到过思考增量。部分思考型模型续写时正文为空、只有思考，
    /// 这种情况不能当成"模型什么都没返回"。
    saw_thinking: bool,
}

#[derive(Debug)]
enum StreamedTurnEnd {
    Finished(StreamedTurn),
    /// 用户在流进行期间点了"停止"。
    Cancelled,
}

/// 流读取过程中需要立刻转发给界面的增量。
#[derive(Debug, PartialEq, Eq)]
enum TurnDelta {
    Text(String),
    Thinking(String),
}

/// 把一批工具调用片段并入累加器。
///
/// OpenAI/Anthropic 的片段以 `index` 标识同一个调用，`id`/`name` 只在该 index
/// 的第一个片段出现，`arguments` 要按序拼接。Gemini 则不同：每个 chunk 里的
/// `index` 都是从 0 开始的 part 序号，每个调用的 id/name/args 一次给全，而且
/// 每个调用都带一个全新的 id——同一个 index 上出现不同的 id，就说明是另一个
/// 调用，必须另起一格，否则两个调用的参数会被拼成一段非法 JSON。
fn accumulate_tool_deltas(
    acc: &mut std::collections::BTreeMap<u32, PartialToolCall>,
    deltas: Vec<ToolCallDelta>,
) {
    for delta in deltas {
        let mut index = delta.index;
        if let (Some(new_id), Some(existing)) = (delta.id.as_ref(), acc.get(&index)) {
            if existing.id.as_ref().is_some_and(|old| old != new_id) {
                index = acc.keys().next_back().map_or(0, |max| max + 1);
            }
        }
        let entry = acc.entry(index).or_default();
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

/// 把累加器收尾成可以直接执行的工具调用。没有参数的工具，各家 provider 流出
/// 来的 arguments 都是空串，统一规整成 `{}`——空串既不是合法 JSON，原样回传给
/// 部分 provider 还会被拒收。
fn finish_tool_calls(acc: std::collections::BTreeMap<u32, PartialToolCall>) -> Vec<ToolCall> {
    acc.into_values()
        .filter_map(|p| {
            Some(ToolCall {
                id: p.id?,
                function: ToolFunction {
                    name: p.name?,
                    arguments: if p.arguments.trim().is_empty() { "{}".to_string() } else { p.arguments },
                },
            })
        })
        .collect()
}

/// 处理一行完整的 SSE 文本。返回 `true` 表示这一轮已经收到明确的结束信号。
fn apply_sse_line(
    provider: &str,
    line: &str,
    turn: &mut StreamedTurn,
    acc: &mut std::collections::BTreeMap<u32, PartialToolCall>,
    on_delta: &mut (dyn FnMut(TurnDelta) + Send),
) -> bool {
    match parse_sse_line(provider, line) {
        Some(StreamContent::Text(text)) => {
            turn.text.push_str(&text);
            on_delta(TurnDelta::Text(text));
            false
        }
        Some(StreamContent::Thinking(text)) => {
            turn.saw_thinking = true;
            on_delta(TurnDelta::Thinking(text));
            false
        }
        Some(StreamContent::ToolCallDeltas(deltas)) => {
            accumulate_tool_deltas(acc, deltas);
            false
        }
        Some(StreamContent::Usage(usage)) => {
            turn.usage = Some(usage);
            false
        }
        Some(StreamContent::Done) => true,
        None => false,
    }
}

/// 读完一个 SSE 响应：正文/思考增量通过 `on_delta` 实时交出，工具调用片段在
/// 内部累加，流结束（收到结束信号，或者像 Google 那样直接关闭连接）后一并返回。
///
/// 按字节缓冲、遇到换行才整行解码：一个多字节字符（比如汉字）可能恰好被拆
/// 在两个网络分片之间，按分片逐个解码会把它变成乱码。
async fn read_turn_stream(
    provider: &str,
    response: reqwest::Response,
    cancel_token: &CancellationToken,
    on_delta: &mut (dyn FnMut(TurnDelta) + Send),
) -> Result<StreamedTurnEnd, LLMError> {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut turn = StreamedTurn::default();
    let mut acc: std::collections::BTreeMap<u32, PartialToolCall> = std::collections::BTreeMap::new();

    'read: loop {
        tokio::select! {
            _ = cancel_token.cancelled() => return Ok(StreamedTurnEnd::Cancelled),
            chunk = stream.next() => match chunk {
                Some(Ok(bytes)) => {
                    buffer.extend_from_slice(&bytes);
                    while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
                        let raw: Vec<u8> = buffer.drain(..=pos).collect();
                        let line = String::from_utf8_lossy(&raw);
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        if apply_sse_line(provider, line, &mut turn, &mut acc, on_delta) {
                            break 'read;
                        }
                    }
                }
                Some(Err(e)) => return Err(LLMError::StreamError(e.to_string())),
                None => {
                    // 流结束了，但最后一行可能没有换行；Google 也从不发明确的结束信号。
                    let line = String::from_utf8_lossy(&buffer).trim().to_string();
                    if !line.is_empty() {
                        apply_sse_line(provider, &line, &mut turn, &mut acc, on_delta);
                    }
                    break 'read;
                }
            }
        }
    }

    turn.tool_calls = finish_tool_calls(acc);
    Ok(StreamedTurnEnd::Finished(turn))
}

/// 发出一个流式请求并读完整个响应。流式请求只设读间隔超时、不设总时长
/// （见 `create_streaming_http_client`），所以工具调用之后的续写即使要想很久，
/// 只要还在吐数据就不会被掐断。
///
/// 用户点"停止"时（无论正在等响应头还是正在读流）都返回 `Cancelled`，而不是
/// 报错——这和首轮流被取消时的表现一致。
async fn run_streaming_request(
    provider: &str,
    url: &str,
    body: &serde_json::Value,
    api_key: &str,
    retry_count: u32,
    retry_interval_secs: u32,
    cancel_token: &CancellationToken,
    on_delta: &mut (dyn FnMut(TurnDelta) + Send),
) -> Result<StreamedTurnEnd, LLMError> {
    let client = create_streaming_http_client(url)?;
    let headers = build_headers(provider, api_key);
    let request_builder = client.post(url).headers(headers).json(body);
    let response = match send_with_retry(&request_builder, retry_count, retry_interval_secs, Some(cancel_token)).await {
        Ok(response) => response,
        Err(_) if cancel_token.is_cancelled() => return Ok(StreamedTurnEnd::Cancelled),
        Err(e) => return Err(e),
    };
    read_turn_stream(provider, response, cancel_token, on_delta).await
}

fn emit_stream_chunk(app_handle: &AppHandle, request: &SendMessageRequest, message_id: &str, content: String, is_thinking: bool, done: bool) {
    let _ = app_handle.emit("stream-chunk", StreamChunk {
        session_id: request.session_id.clone(),
        message_id: message_id.to_string(),
        content,
        is_thinking,
        done,
    });
}

/// 带界面事件转发的 `run_streaming_request`：正文/思考增量立刻通过
/// `stream-chunk` 发给前端。`separate_text` 为 true 表示这条回复里前面已经
/// 有过正文（工具调用之前的开场白），本轮的第一段正文前补一个段落分隔，免得
/// 落库后的正文把两轮话直接粘在一起。
async fn stream_turn(
    app_handle: &AppHandle,
    request: &SendMessageRequest,
    message_id: &str,
    url: &str,
    body: &serde_json::Value,
    api_key: &str,
    cancel_token: &CancellationToken,
    separate_text: bool,
) -> Result<StreamedTurnEnd, LLMError> {
    let mut separator_pending = separate_text;
    let mut forward = |delta: TurnDelta| match delta {
        TurnDelta::Text(text) => {
            if std::mem::take(&mut separator_pending) {
                emit_stream_chunk(app_handle, request, message_id, "\n\n".to_string(), false, false);
            }
            emit_stream_chunk(app_handle, request, message_id, text, false, false);
        }
        TurnDelta::Thinking(text) => emit_stream_chunk(app_handle, request, message_id, text, true, false),
    };
    run_streaming_request(
        &request.provider,
        url,
        body,
        api_key,
        request.retry_count.unwrap_or(DEFAULT_LLM_RETRY_COUNT),
        request.retry_interval_secs.unwrap_or(DEFAULT_LLM_RETRY_INTERVAL_SECS),
        cancel_token,
        &mut forward,
    )
    .await
}

fn add_usage(total: &mut Option<TokenUsage>, usage: Option<TokenUsage>) {
    let Some(usage) = usage else { return };
    match total {
        Some(sum) => {
            sum.prompt_tokens += usage.prompt_tokens;
            sum.completion_tokens += usage.completion_tokens;
            sum.total_tokens += usage.total_tokens;
        }
        None => *total = Some(usage),
    }
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
    let memory_state = app_handle.state::<crate::memory::MemoryState>();
    let _memory_activity = memory_state.activity();
    let memory_context = crate::memory::Context::chat(&request.session_id, request.working_directory.as_deref());
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
    // 内置文件工具独立于 MCP 总开关，只由当前 Chat 的持久化授权决定。
    let file_rules = file_tools::rules_for_directory(request.working_directory.as_deref(), &request.file_access_mode);
    mcp_tools.extend(file_tools::tool_defs(&file_rules));
    if memory_state.settings().enabled { mcp_tools.extend(crate::memory::tool_defs()); }

    // 把手动激活的 skill 的 instructions（加上可读资源文件的内容）作为一段
    // system prompt 注入进去，是和已有的 system 消息合并，而不是替换掉它。
    let mut effective_messages = request.messages.clone();

    // 压缩不会删掉历史消息；它只改变这一轮实际送入模型的“活动上下文”。
    // 系统消息继续完整保留，较早对话由最新摘要替代，最近几轮仍原样保留。
    let context_compaction = {
        let db = state.0.lock().await;
        db.get_context_compaction(&request.session_id)
            .map_err(|e| LLMError::ApiError(format!("读取上下文摘要失败: {e}")))?
    };
    if let Some(compaction) = context_compaction {
        effective_messages = apply_context_compaction(effective_messages, &compaction);
    }
    let memory_settings = memory_state.settings();
    if memory_settings.enabled {
        append_memory_prompt(&mut effective_messages,format!("【当前会话环境（由应用提供）】{}",serde_json::json!({"session_id":request.session_id,"working_directory":request.working_directory,"file_access":request.file_access_mode,"date":chrono::Local::now().format("%Y-%m-%d").to_string()})));
    }
    if memory_settings.enabled && memory_settings.auto_compact {
        if let Some(compaction) = auto_compact_messages(&request.provider, &request.model, &api_key, &request.base_url, &effective_messages, memory_settings.context_tokens, &cancel_token).await? {
            state.0.lock().await.save_context_compaction(&request.session_id, &compaction)
                .map_err(|e| LLMError::ApiError(format!("保存自动摘要失败：{e}")))?;
            effective_messages = apply_context_compaction(effective_messages, &compaction);
        }
    }
    if let Some(latest) = request.messages.iter().rev().find(|m|m.role=="user") {
        let memory_prompt = crate::memory::prompt(&app_handle, &memory_context, &latest.content).await;
        if !memory_prompt.is_empty() {
            append_memory_prompt(&mut effective_messages, memory_prompt);
        }
    }
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
                    token_usage: None,
                    tool_calls: None,
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

    let first_turn = match stream_turn(
        &app_handle,
        &request,
        &message_id,
        &url,
        &body,
        &api_key,
        &cancel_token,
        false,
    )
    .await
    {
        Ok(StreamedTurnEnd::Finished(turn)) => turn,
        Ok(StreamedTurnEnd::Cancelled) => {
            log::info!("Stream cancelled for session: {}", session_id);
            emit_stream_chunk(&app_handle, &request, &message_id, String::new(), false, true);
            return Ok(());
        }
        Err(e) => {
            log::error!("LLM request failed for url '{}': {:?}", url, e);
            return Err(e);
        }
    };

    finalize_turn(
        &app_handle,
        state.clone(),
        &request,
        &message_id,
        &effective_messages,
        &mcp_tools,
        &all_skills,
        &autonomous_skills,
        first_turn,
        request.max_tokens,
        &file_rules,
        &api_key,
        &cancel_token,
    )
    .await
}

/// 执行一轮工具调用（可能是自主的 Skill 调用，也可能是真正的 MCP 工具调用），
/// 按 `tool_calls` 原来的顺序返回它们各自的结果。
fn truncate_text(value: &str, limit: usize) -> (String, bool) {
    if value.chars().count() <= limit { return (value.to_string(), false); }
    (format!("{}…[已截断]", value.chars().take(limit).collect::<String>()), true)
}

fn find_text_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter().find_map(|key| object.get(*key).and_then(|v| v.as_str()).map(str::to_string))
}

fn is_web_tool(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("search") || name.contains("web") || name.contains("browser") || name.contains("fetch")
}

/// 供模型继续推理的确定性压缩结果。小结果保持原样；大结果限制字符串、数组和嵌套，
/// 并附带明确标记，避免模型误以为它看到了完整原始资料。
fn compact_tool_result(value: &serde_json::Value, tool_name: &str) -> serde_json::Value {
    let original = value.to_string();
    if original.len() <= TOOL_RESULT_CHAR_BUDGET && !is_web_tool(tool_name) { return value.clone(); }

    fn compact(value: &serde_json::Value, depth: usize) -> serde_json::Value {
        if depth >= 4 { return serde_json::json!("[嵌套内容已省略]"); }
        match value {
            serde_json::Value::String(s) => {
                if s.len() > 12_000 || s.starts_with("data:") || s.len() > 512 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_')) {
                    serde_json::json!(truncate_text(s, 4_000).0)
                } else { value.clone() }
            }
            serde_json::Value::Array(items) => serde_json::Value::Array(items.iter().take(12).map(|item| compact(item, depth + 1)).collect()),
            serde_json::Value::Object(map) => {
                let mut result = serde_json::Map::new();
                for (key, item) in map.iter().take(24) {
                    if matches!(key.as_str(), "html" | "rawHtml" | "base64" | "binary" | "data") && item.is_string() { continue; }
                    result.insert(key.clone(), compact(item, depth + 1));
                }
                serde_json::Value::Object(result)
            }
            _ => value.clone(),
        }
    }

    let mut compacted = compact(value, 0);
    if is_web_tool(tool_name) {
        // 搜索结果是一个数组，不能只在顶层找标题/URL，否则会把对模型最有用的
        // 定位信息压成“网页结果”。抓取则保留 URL 与有限正文节选。
        if tool_name.to_ascii_lowercase().contains("search") {
            let results: Vec<_> = value.get("results").and_then(|v| v.as_array()).into_iter().flatten().take(10)
                .map(|item| serde_json::json!({
                    "title": find_text_field(item, &["title", "name"]).unwrap_or_else(|| "未命名结果".to_string()),
                    "url": find_text_field(item, &["url", "link", "href"]).unwrap_or_default(),
                    "snippet": find_text_field(item, &["snippet", "description", "content"]).map(|s| truncate_text(&s, 500).0).unwrap_or_default(),
                }))
                .collect();
            compacted = serde_json::json!({ "query": value.get("query").cloned().unwrap_or_default(), "results": results, "truncated": true });
        } else {
            let title = find_text_field(value, &["title", "name"]).unwrap_or_else(|| "网页结果".to_string());
            let url = find_text_field(value, &["url", "link", "href"]).unwrap_or_default();
            let excerpt = find_text_field(value, &["snippet", "description", "content", "text", "body"])
                .map(|s| truncate_text(&s, 2_000).0).unwrap_or_default();
            compacted = serde_json::json!({ "title": title, "url": url, "excerpt": excerpt, "truncated": true });
        }
    }
    let before = original.len();
    let (serialized, truncated) = truncate_text(&compacted.to_string(), TOOL_RESULT_CHAR_BUDGET);
    let mut output = serde_json::from_str(&serialized).unwrap_or_else(|_| serde_json::json!({ "content": serialized }));
    if truncated || before > output.to_string().len() {
        if let Some(object) = output.as_object_mut() { object.insert("_truncated".to_string(), serde_json::json!(true)); }
    }
    output
}

/// 仅用于界面，绝不影响模型上下文。网页工具显示黑白卡片所需的元数据，不透传正文。
fn tool_display_result(value: &serde_json::Value, tool_name: &str) -> serde_json::Value {
    if is_web_tool(tool_name) {
        let search_results = value.get("results").and_then(|v| v.as_array());
        let first_result = search_results.and_then(|results| results.first());
        let title = first_result.and_then(|item| find_text_field(item, &["title", "name"]))
            .or_else(|| find_text_field(value, &["title", "name"]))
            .unwrap_or_else(|| "网页结果".to_string());
        let url = first_result.and_then(|item| find_text_field(item, &["url", "link", "href"]))
            .or_else(|| find_text_field(value, &["url", "link", "href"]))
            .unwrap_or_default();
        let count = search_results.map(|v| v.len());
        return serde_json::json!({ "display_type": if tool_name.to_ascii_lowercase().contains("search") { "web_search" } else { "web_fetch" }, "title": title, "url": url, "result_count": count, "truncated": true });
    }
    let (preview, truncated) = truncate_text(&value.to_string(), 1_500);
    serde_json::json!({ "display_type": "preview", "preview": preview, "truncated": truncated })
}

fn enforce_round_tool_budget(rounds: &mut [ToolRound]) {
    let mut used = 0usize;
    let mut seen = std::collections::HashMap::new();
    // 倒序优先保留最近工具结果；较早结果退化为可追溯元信息。
    for round in rounds.iter_mut().rev() {
        for (call, result) in round.calls.iter().zip(round.results.iter_mut()).rev() {
            // 只合并完全相同的只读调用结果，保留每次调用 ID 和最近一份正文。
            if ["search","read","list","fetch"].iter().any(|word|call.function.name.contains(word)) {
                let key=(call.function.name.clone(),call.function.arguments.clone(),result.to_string());
                if let Some(latest_id)=seen.get(&key) {
                    *result=serde_json::json!({"status":"completed","duplicate_of":latest_id,"note":"结果与该次调用完全相同，请参考其正文"});
                } else { seen.insert(key,call.id.clone()); }
            }
            let size = result.to_string().len();
            if used + size <= TOOL_ROUND_TOKEN_BUDGET * 4 {
                used += size;
                continue;
            }
            let title = find_text_field(result, &["title", "name"]).unwrap_or_default();
            let url = find_text_field(result, &["url", "link", "href"]).unwrap_or_default();
            *result = serde_json::json!({
                "tool": call.function.name,
                "status": "completed",
                "title": title,
                "url": url,
                "_truncated": true,
                "note": "较早工具结果因本轮上下文预算已省略；如有必要请缩小查询范围后重新调用工具。"
            });
            used += result.to_string().len();
        }
    }
}

async fn execute_tool_calls(
    app_handle: &AppHandle,
    state: tauri::State<'_, DbState>,
    session_id: &str,
    message_id: &str,
    tool_calls: &[ToolCall],
    mcp_tools: &[MCPTool],
    all_skills: &[Skill],
    file_rules: &[FileAccessRule],
    working_directory: Option<&str>,
    terminal_shell: &str,
) -> Vec<serde_json::Value> {
    let mut tool_results = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let _ = app_handle.emit("tool-call-status", ToolCallEvent {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            call_id: tool_call.id.clone(),
            tool_name: tool_call.function.name.clone(),
            arguments: tool_call.function.arguments.clone(),
            status: "calling".to_string(),
            result: None,
        });

        let result = if let Some(skill_id) = tool_call.function.name.strip_prefix("skill__") {
            // 模型自主调用的 Skill：这里的"工具结果"其实是该 skill 自己的
            // instructions/资源内容，而不是一次真正的 MCP 调用。
            if let Some(skill) = all_skills.iter().find(|s| s.id == skill_id) {
                log::info!("Model invoked skill: {}", skill.name);
                let content = build_skill_context(std::slice::from_ref(skill), app_handle).await;
                serde_json::json!({ "skill": skill.name, "content": content })
            } else {
                log::warn!("Skill not found for autonomous call: {}", skill_id);
                serde_json::json!({ "error": format!("skill '{}' not found", skill_id) })
            }
        } else if tool_call.function.name.starts_with("memory_") && mcp_tools.iter().any(|t|t.name==tool_call.function.name) {
            let context = crate::memory::Context::chat(session_id,working_directory);
            crate::memory::execute(app_handle,&context,&tool_call.function.name,&serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null)).await
        } else if tool_call.function.name.starts_with("baiyu_file_") {
            file_tools::execute(file_rules, &tool_call.function.name, &serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null))
        } else if let Some(tool) = mcp_tools.iter().find(|t| t.name == tool_call.function.name) {
            log::info!("Executing MCP tool: {}", tool.name);
            match call_mcp_tool_with_context(
                state.clone(),
                Some(tool.server_id.clone()),
                tool.name.clone(),
                serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::Value::Null),
                BuiltinToolContext {
                    app_handle: Some(app_handle.clone()),
                    session_id: Some(session_id.to_string()),
                    working_directory: working_directory.map(str::to_string),
                    terminal_shell: terminal_shell.to_string(),
                },
            ).await {
                Ok(result) => {
                    log::info!("Tool execution result: tool={} original_bytes={}", tool.name, result.to_string().len());
                    result
                }
                Err(e) => {
                    log::error!("Tool execution failed: {}", e);
                    serde_json::json!({ "error": e.to_string() })
                }
            }
        } else {
            log::warn!("MCP tool not found: {}", tool_call.function.name);
            serde_json::json!({ "error": format!("tool '{}' not found", tool_call.function.name) })
        };

        let is_error = result.get("error").is_some();
        let original_size = result.to_string().len();
        let display_result = tool_display_result(&result, &tool_call.function.name);
        let compact_result = compact_tool_result(&result, &tool_call.function.name);
        log::info!("Tool result compacted: tool={} original_bytes={} kept_bytes={} truncated={}", tool_call.function.name, original_size, compact_result.to_string().len(), original_size > compact_result.to_string().len());
        let _ = app_handle.emit("tool-call-status", ToolCallEvent {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            call_id: tool_call.id.clone(),
            tool_name: tool_call.function.name.clone(),
            arguments: tool_call.function.arguments.clone(),
            status: if is_error { "error".to_string() } else { "done".to_string() },
            result: Some(display_result.to_string()),
        });

        tool_results.push(compact_result);
    }
    tool_results
}

/// 一轮已经执行完的工具调用：模型随调用一起说的话、它发起的调用、以及各调用
/// （已压缩过的）结果。
#[derive(Debug)]
struct ToolRound {
    text: String,
    calls: Vec<ToolCall>,
    results: Vec<serde_json::Value>,
}

/// 首轮流结束后的收尾：如果模型要求调用工具，就执行它们，把结果交给模型继续
/// ——续写请求同样是流式的，模型在两次工具调用之间说的话会实时显示出来——
/// 直到模型给出最终回答，最后发出终止的 `done: true` 数据块。
///
/// 这个函数同时被"明确的本轮结束信号"（OpenAI 的 `[DONE]`、Anthropic 的
/// `message_stop`）和"流直接关闭、没有任何结束信号"（Google 就是这样）两种
/// 情况共用——它们在 `read_turn_stream` 里已经被统一成一个 `StreamedTurn`。
async fn finalize_turn(
    app_handle: &AppHandle,
    state: tauri::State<'_, DbState>,
    request: &SendMessageRequest,
    message_id: &str,
    effective_messages: &[ChatMessage],
    mcp_tools: &[MCPTool],
    all_skills: &[Skill],
    autonomous_skills: &[Skill],
    first_turn: StreamedTurn,
    max_tokens: Option<u32>,
    file_rules: &[FileAccessRule],
    api_key: &str,
    cancel_token: &CancellationToken,
) -> Result<(), LLMError> {
    // 模型在同一轮里确实可能合理地需要不止一次工具调用（比如先"列出允许访问的
    // 目录"，再"列出该目录下的文件"）。循环处理，每次续写请求都把模型自己的
    // 工具重新带上，直到它返回纯文本，或者达到轮次上限为止——没有这个上限的话，
    // 一个行为异常的模型可能会无限循环下去。
    let max_tool_rounds = request.max_tool_rounds
        .map(|value| value.clamp(1, MAX_TOOL_ROUNDS as u32) as usize)
        .unwrap_or(DEFAULT_MAX_TOOL_ROUNDS);
    let url = build_url(&request.provider, &request.base_url, &request.model, true);

    let mut usage_total: Option<TokenUsage> = None;
    let mut turn = first_turn;
    let mut rounds: Vec<ToolRound> = Vec::new();
    let mut emitted_text = !turn.text.trim().is_empty();
    report_usage(app_handle, request, message_id, &mut usage_total, turn.usage.take());

    while !turn.tool_calls.is_empty() {
        if cancel_token.is_cancelled() {
            emit_stream_chunk(app_handle, request, message_id, String::new(), false, true);
            return Ok(());
        }

        // 已经执行满 `max_tool_rounds` 轮、模型却还想继续调用工具：不再执行，
        // 改发一次禁用工具的续写，要求它基于已有结果收尾。
        let force_final = rounds.len() >= max_tool_rounds;
        if force_final {
            log::warn!("Tool-call round limit ({}) reached; requesting a final answer without tools", max_tool_rounds);
        } else {
            let tool_calls = std::mem::take(&mut turn.tool_calls);
            let results = execute_tool_calls(
                app_handle,
                state.clone(),
                &request.session_id,
                message_id,
                &tool_calls,
                mcp_tools,
                all_skills,
                file_rules,
                request.working_directory.as_deref(),
                &request.terminal_shell,
            )
            .await;
            rounds.push(ToolRound { text: std::mem::take(&mut turn.text), calls: tool_calls, results });
            enforce_round_tool_budget(&mut rounds);
        }

        let body = build_continuation_body(
            &request.provider,
            &request.model,
            effective_messages,
            &rounds,
            mcp_tools,
            autonomous_skills,
            max_tokens,
            force_final,
        );
        match stream_turn(app_handle, request, message_id, &url, &body, api_key, cancel_token, emitted_text).await {
            Ok(StreamedTurnEnd::Finished(next)) => turn = next,
            Ok(StreamedTurnEnd::Cancelled) => {
                emit_stream_chunk(app_handle, request, message_id, String::new(), false, true);
                return Ok(());
            }
            Err(err) => {
                log::error!("Failed to continue reasoning after tool calls: {}", err);
                return Err(LLMError::ApiError(if force_final {
                    format!("达到工具调用上限后的强制收尾失败: {}", err)
                } else {
                    format!("工具调用后的续写失败: {}", err)
                }));
            }
        }
        emitted_text |= !turn.text.trim().is_empty();
        report_usage(app_handle, request, message_id, &mut usage_total, turn.usage.take());

        if turn.tool_calls.is_empty() {
            // 最终回答。既没有正文、也没有思考，说明模型什么都没给出来。
            if turn.text.trim().is_empty() && !turn.saw_thinking {
                return Err(LLMError::ApiError("LLM did not return content".to_string()));
            }
        } else if force_final {
            return Err(LLMError::ApiError("达到工具调用上限后，模型仍试图调用工具，无法生成最终回答".to_string()));
        }
    }

    log::info!("[LLM] stream_message 完成: session={}", request.session_id);
    emit_stream_chunk(app_handle, request, message_id, String::new(), false, true);
    Ok(())
}

/// 把这一轮回报的 token 用量并入整条回复的累计值，并把累计值发给前端——一条
/// 带工具调用的回复会发起多次模型请求，用户关心的是整条回复实际消耗的总量。
fn report_usage(
    app_handle: &AppHandle,
    request: &SendMessageRequest,
    message_id: &str,
    total: &mut Option<TokenUsage>,
    usage: Option<TokenUsage>,
) {
    if usage.is_none() {
        return;
    }
    add_usage(total, usage);
    if let Some(sum) = total.clone() {
        let _ = app_handle.emit("token-usage", TokenUsageEvent {
            session_id: request.session_id.clone(),
            message_id: message_id.to_string(),
            usage: sum,
        });
    }
}

/// 构造工具调用之后的续写请求体（流式）：把原始对话、之前每一轮"模型说了什么
/// ＋调用了什么＋返回了什么"一并告诉模型，从而继续这段对话。这里要重新附上
/// 模型自己的工具定义（一次全新的 API 调用并不会记得原始请求里的 `tools`
/// 字段），因为没有这些定义的话，一个想再次调用工具的模型没有原生方式可以这么
/// 做，只能试图用纯文本假装调用一次。每家 provider 对"我调用了什么"/"这是
/// 结果"的表达形状都不一样，所以按分支分别构造。
///
/// 模型随工具调用一起说的话（`ToolRound::text`）会原样放回它自己那条 assistant
/// 消息里，这样模型看到的是完整的上下文，而不是一串没有任何说明的工具调用。
fn build_continuation_body(
    provider: &str,
    model: &str,
    original_messages: &[ChatMessage],
    rounds: &[ToolRound],
    mcp_tools: &[MCPTool],
    autonomous_skills: &[Skill],
    max_tokens: Option<u32>,
    force_final: bool,
) -> serde_json::Value {
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

    const FINAL_NOTICE: &str = "工具调用次数已达上限。现在禁止再调用任何工具；请仅基于已有结果给出最终回答，并明确列出仍未完成或无法确认的工作。";

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
            for round in rounds {
                let mut assistant_blocks: Vec<serde_json::Value> = Vec::new();
                if !round.text.trim().is_empty() {
                    assistant_blocks.push(serde_json::json!({ "type": "text", "text": round.text.trim() }));
                }
                assistant_blocks.extend(round.calls.iter().map(|tc| {
                    serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .unwrap_or_else(|_| serde_json::json!({})),
                    })
                }));
                msgs.push(serde_json::json!({ "role": "assistant", "content": assistant_blocks }));

                let mut content: Vec<serde_json::Value> = round
                    .calls
                    .iter()
                    .zip(round.results.iter())
                    .map(|(tc, result)| {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tc.id,
                            "content": serde_json::to_string(result).unwrap_or_else(|_| "null".to_string()),
                        })
                    })
                    .collect();
                if force_final {
                    content.push(serde_json::json!({ "type": "text", "text": FINAL_NOTICE }));
                }
                msgs.push(serde_json::json!({ "role": "user", "content": content }));
            }

            // 和 build_stream_request_body 一样的原因：Anthropic 强制要求这个
            // 字段，所以用户没填的话就退回到一个足够宽裕的默认值，而不是一个
            // 会截断长回复的数字。
            let max_tokens_val = max_tokens.unwrap_or(32000);
            let mut b = serde_json::json!({
                "model": model,
                "messages": msgs,
                "max_tokens": max_tokens_val,
                "stream": true,
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
            if force_final { b["tool_choice"] = serde_json::json!({ "type": "none" }); }
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

            for round in rounds {
                let mut call_parts: Vec<serde_json::Value> = Vec::new();
                if !round.text.trim().is_empty() {
                    call_parts.push(serde_json::json!({ "text": round.text.trim() }));
                }
                call_parts.extend(round.calls.iter().map(|tc| {
                    serde_json::json!({
                        "functionCall": {
                            "name": tc.function.name,
                            "args": serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                .unwrap_or_else(|_| serde_json::json!({})),
                        }
                    })
                }));
                contents.push(serde_json::json!({ "role": "model", "parts": call_parts }));

                // Gemini REST API 要求 functionResponse 部分的 role 必须是
                // "user"，不能是 "function"——模型角色是 "model"，用户输入是 "user"。
                let mut parts: Vec<serde_json::Value> = round
                    .calls
                    .iter()
                    .zip(round.results.iter())
                    .map(|(tc, result)| {
                        serde_json::json!({
                            "functionResponse": {
                                "name": tc.function.name,
                                "response": result,
                            }
                        })
                    })
                    .collect();
                if force_final {
                    parts.push(serde_json::json!({ "text": FINAL_NOTICE }));
                }
                contents.push(serde_json::json!({ "role": "user", "parts": parts }));
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
            if force_final { b["toolConfig"] = serde_json::json!({ "functionCallingConfig": { "mode": "NONE" } }); }
            b
        }
        _ => {
            let mut msgs: Vec<serde_json::Value> = original_messages
                .iter()
                .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
                .collect();

            for round in rounds {
                let tool_calls_json: Vec<_> = round
                    .calls
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
                    "content": if round.text.trim().is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::json!(round.text.trim())
                    },
                    "tool_calls": tool_calls_json,
                }));

                for (tc, result) in round.calls.iter().zip(round.results.iter()) {
                    msgs.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": serde_json::to_string(result).unwrap_or_else(|_| "null".to_string()),
                    }));
                }
            }
            if force_final {
                msgs.push(serde_json::json!({ "role": "user", "content": FINAL_NOTICE }));
            }

            let mut b = serde_json::json!({
                "model": model,
                "messages": msgs,
                "stream": true,
                "stream_options": { "include_usage": true },
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
            if force_final { b["tool_choice"] = serde_json::json!("none"); }
            b
        }
    };
    append_skill_tools(&mut body, provider, autonomous_skills);
    body
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
                let role = if m.role == "assistant" { "assistant" } else { "user" };
                if role == "user" && !m.images.is_empty() {
                    // 与 build_stream_request_body 同款的图片块形状
                    let mut blocks: Vec<serde_json::Value> = m.images.iter().map(|img| serde_json::json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": img.media_type, "data": img.data }
                    })).collect();
                    if !m.content.is_empty() {
                        blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                    }
                    serde_json::json!({ "role": role, "content": blocks })
                } else {
                    serde_json::json!({ "role": role, "content": m.content })
                }
            })
            .collect(),
        "google" => messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                let role = if m.role == "assistant" { "model" } else { "user" };
                if role == "user" && !m.images.is_empty() {
                    let mut parts: Vec<serde_json::Value> = vec![];
                    if !m.content.is_empty() {
                        parts.push(serde_json::json!({"text": m.content}));
                    }
                    for img in &m.images {
                        parts.push(serde_json::json!({
                            "inline_data": {"mime_type": img.media_type, "data": img.data}
                        }));
                    }
                    serde_json::json!({ "role": "user", "parts": parts })
                } else {
                    serde_json::json!({ "role": role, "parts": [{ "text": m.content }] })
                }
            })
            .collect(),
        _ => messages
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
                            "image_url": {"url": data_uri}
                        }));
                    }
                    serde_json::json!({ "role": m.role, "content": content })
                } else {
                    serde_json::json!({ "role": m.role, "content": m.content })
                }
            })
            .collect(),
    }
}

/// 把一轮工具调用（模型的调用请求 + 已执行的结果）追加到原生消息数组上，
/// 用的是每家 provider 期望在下一轮历史里看到的那种回放形状。和
/// `build_continuation_body` 里已经确立的各 provider 形状保持一致。
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
/// 文本回复，或者它想要执行的工具调用。Workspace Agent 走的是这条非流式路径，
/// 它总是会重新提供 `tools`，也可能再次返回 `ToolCalls`——正是这一点让
/// Workspace Agent 循环能够连续多轮调用工具，而不只是一轮。（普通聊天页的
/// 工具循环走的是流式路径，见 `finalize_turn`。）
/// 从 OpenAI 兼容响应中提取压缩摘要。正文优先；只有正文为空时，才退回服务端
/// 已返回的 reasoning 字段。
fn extract_compaction_response_text(message: &serde_json::Value) -> (String, bool) {
    let content = message
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !content.trim().is_empty() {
        return (content.to_string(), false);
    }
    let reasoning = message
        .get("reasoning_content")
        .and_then(|value| value.as_str())
        .or_else(|| message.get("reasoning").and_then(|value| value.as_str()))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("");
    (reasoning.to_string(), !reasoning.is_empty())
}

/// 非流式模型调用。`allow_reasoning_fallback` 仅供上下文压缩使用，避免将
/// reasoning 内容意外作为 Agent Team 的普通回复。
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
    allow_reasoning_fallback: bool,
) -> Result<TurnOutcome, LLMError> {
    run_turn_with_cancel(
        provider,
        model,
        api_key,
        base_url,
        system_prompt,
        native_messages,
        tools,
        max_tokens,
        enable_thinking,
        allow_reasoning_fallback,
        None,
    )
    .await
}

/// Agent Team 版本的非流式往返。与普通 `run_turn` 共用请求构造和解析，
/// 但允许工作组/Agent 删除时立即中断正在等待响应的 HTTP 请求，而不是继续
/// 占用后台任务直到 600 秒总超时和后续重试全部结束。
pub(crate) async fn run_turn_with_cancel(
    provider: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    system_prompt: Option<&str>,
    native_messages: &[serde_json::Value],
    tools: &[MCPTool],
    max_tokens: Option<u32>,
    enable_thinking: bool,
    allow_reasoning_fallback: bool,
    cancel_token: Option<&CancellationToken>,
) -> Result<TurnOutcome, LLMError> {
    let url = build_url(provider, base_url, model, false);
    let client = create_http_client(&url)?;
    let body = build_run_turn_body(provider, model, system_prompt, native_messages, tools, max_tokens, enable_thinking);

    let headers = build_headers(provider, api_key);
    let request_builder = client.post(&url).headers(headers).json(&body);
    let request_flow = async {
        let response = send_with_retry(
            &request_builder,
            DEFAULT_LLM_RETRY_COUNT,
            DEFAULT_LLM_RETRY_INTERVAL_SECS,
            cancel_token,
        )
        .await?;

        let json: serde_json::Value = match cancel_token {
            Some(token) => {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        return Err(LLMError::StreamError("请求已取消".to_string()));
                    }
                    result = response.json() => result.map_err(LLMError::RequestError)?,
                }
            }
            None => response.json().await.map_err(LLMError::RequestError)?,
        };
        Ok(json)
    };
    let json = with_total_timeout(LLM_REQUEST_TIMEOUT, request_flow).await?;

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
            let (text, used_reasoning_fallback) = if allow_reasoning_fallback {
                extract_compaction_response_text(&message)
            } else {
                (
                    message.get("content").and_then(|value| value.as_str()).unwrap_or("").to_string(),
                    false,
                )
            };
            // 某些推理模型会把压缩器要求的摘要错误地只放进 reasoning 字段。
            // 本函数仅由 compact_chat_context 调用，故可安全兜底；普通聊天绝不能
            // 复用此逻辑，以免将模型思考过程显示给用户。
            if used_reasoning_fallback {
                log::warn!(
                    "[LLM] 上下文压缩响应正文为空，改用 {} 字符的 reasoning 字段",
                    text.len()
                );
            }
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
            // 与流式请求保持一致：SiliconFlow 默认开启思考，因此关掉时也要明确传 false。
            if provider == "siliconflow" {
                b["enable_thinking"] = serde_json::json!(enable_thinking);
                if enable_thinking {
                    b["thinking_budget"] = serde_json::json!(8000);
                }
            }
            // 与流式路径同款：本地服务在思考开关关闭时显式关掉思考，
            // Workspace Agent 也能享受到（不开思考的 Agent 不再白等思考）
            if !enable_thinking && matches!(provider, "local" | "custom" | "openclaw") {
                b["reasoning_effort"] = serde_json::json!("none");
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

fn get_api_key(request: &SendMessageRequest) -> Result<String, LLMError> {
    get_api_key_for_config(&request.provider, &request.api_config_id)
}

/// 普通对话和“压缩摘要”共用同一条 keyring 读取路径，避免任何密钥进入 IPC。
pub(crate) fn get_api_key_for_config(provider: &str, api_config_id: &str) -> Result<String, LLMError> {
    // 本地模型不需要 API key
    if provider == "local" {
        return Ok(String::new());
    }
    // 没有传 api_key —— 退回到以 provider 为键的系统 keyring 查找。
    // 前端调用 save_api_key(provider, key) 时，keyring 里的标签就是
    // "api_keys_{provider}"。这样一来，只要密钥已经存在 keyring 里，
    // 调用方就可以逐步不再在 IPC 请求里嵌入明文密钥。
    if !api_config_id.is_empty() {
        let label = format!("api_keys_{}", api_config_id);
        if let Ok(entry) = KeyringEntry::new("BaiyuAISpace", &label) {
            if let Ok(key) = entry.get_password() {
                if !key.is_empty() {
                    log::info!("[LLM] API key resolved from keyring for configured provider");
                    return Ok(key);
                }
            }
        }
    }
    Err(LLMError::MissingApiKey)
}

const COMPACTION_SYSTEM_PROMPT: &str = r#"你是桌面 AI 助手的上下文压缩器。请把对话写成供下一轮模型继续工作的结构化交接摘要。

必须保留：用户目标和硬约束；已完成工作；已证实事实及证据；人物、日期、预算、偏好、决策及其理由；涉及的文件、路径、命令、配置键、版本号和报错原文；已尝试且排除的方案；未完成事项和下一步。适用于研究、写作、规划、开发等各种任务。不要编造，不要把“可能”写成事实。具体名称和数值尽量原样保留。只输出中文 Markdown 摘要，不要回答原对话中的问题。"#;

/// Token 数是保守估算；仅替换活动上下文，摘要失败则明确返回错误，绝不悄悄丢弃历史。
pub(crate) async fn auto_compact_messages(provider: &str, model: &str, api_key: &str, base_url: &str, messages: &[ChatMessage], budget: usize, cancel: &CancellationToken) -> Result<Option<ContextCompaction>, LLMError> {
    if budget == 0 {
        return Ok(None);
    }
    let total: usize = messages.iter().map(|m|crate::memory::estimate_tokens(&m.content)+8).sum();
    let history: Vec<_> = messages.iter().filter(|m|m.role!="system").collect();
    if total < budget * 4 / 5 || history.is_empty() { return Ok(None); }
    // 以预算决定要保留多少“最近原文”，而不是用固定消息条数。这样一条
    // 超长用户消息也会触发压缩；最新用户消息始终留在活动上下文中。
    let preserve_budget = (budget * 3 / 5).max(1);
    let mut preserved_tokens = 0usize;
    let mut boundary_index = history.len() - 1;
    for index in (0..history.len()).rev() {
        let cost = crate::memory::estimate_tokens(&history[index].content) + 8;
        if index != history.len() - 1 && preserved_tokens.saturating_add(cost) > preserve_budget { break; }
        preserved_tokens = preserved_tokens.saturating_add(cost);
        boundary_index = index;
    }
    boundary_index = (boundary_index..history.len())
        .find(|index| history[*index].role == "user")
        .or_else(|| history.iter().rposition(|message| message.role == "user"))
        .unwrap_or(boundary_index);
    let boundary = history[boundary_index].timestamp;
    let old: Vec<_> = messages.iter().filter(|m|m.role=="system" || m.timestamp<boundary).map(|m|serde_json::json!({"role":m.role,"content":crate::memory::redact(&m.content)})).collect();
    if old.is_empty() && boundary_index == 0 {
        return Err(LLMError::ApiError("最新一条消息本身超出自动摘要预算，请缩短消息或使用手动压缩".into()));
    }
    let source = serde_json::to_string(&old).map_err(|e|LLMError::ApiError(e.to_string()))?;
    // 每轮有界输入；超限不截掉关键早期决策，而是交给手动压缩处理。
    if source.len()>600_000 { return Err(LLMError::ApiError("历史超出自动摘要输入上限，请先手动压缩上下文".into())); }
    let outcome = run_turn_with_cancel(provider,model,api_key,base_url,Some(COMPACTION_SYSTEM_PROMPT),&[serde_json::json!({"role":"user","content":source})],&[],Some(3000),false,true,Some(cancel)).await?;
    match outcome {
        TurnOutcome::Text(summary) if !summary.trim().is_empty() => Ok(Some(ContextCompaction {summary,preserve_from_timestamp:boundary,created_at:chrono::Utc::now().timestamp_millis()})),
        _ => Err(LLMError::ApiError("自动摘要未返回有效正文，完整历史已保留".into()))
    }
}

/// Anthropic/Gemini 构造器只读取第一条 system，因此必须合并而非新增并列消息。
fn append_memory_prompt(messages: &mut Vec<ChatMessage>, prompt: String) {
    if let Some(system) = messages.iter_mut().find(|m|m.role=="system") {
        system.content.push_str("\n\n"); system.content.push_str(&prompt);
    } else {
        messages.insert(0, ChatMessage {id:Uuid::new_v4().to_string(),role:"system".into(),content:prompt,timestamp:0,error:None,images:vec![],videos:vec![],token_usage:None, tool_calls: None});
    }
}

fn build_compaction_source(messages: &[ChatMessage]) -> String {
    const MAX_MESSAGE_CHARS: usize = 8_000;
    const MAX_SOURCE_CHARS: usize = 96_000;
    let mut source = String::new();

    for message in messages.iter().filter(|message| message.role != "system") {
        if source.len() >= MAX_SOURCE_CHARS {
            source.push_str("\n\n【其余较早消息因压缩输入预算未纳入；请勿猜测其内容】");
            break;
        }
        let role = match message.role.as_str() {
            "assistant" => "助手",
            _ => "用户",
        };
        let content: String = message.content.chars().take(MAX_MESSAGE_CHARS).collect();
        source.push_str(&format!("\n\n### {role}\n{content}"));
        if message.content.chars().count() > MAX_MESSAGE_CHARS {
            source.push_str("\n【该条消息过长，后续部分未纳入摘要输入】");
        }
    }
    source
}

/// 用已保存的摘要替换较早的活动上下文，但不修改数据库中的完整消息历史。
/// 单独抽出这一步，既让流式请求路径更直观，也能离线验证真正发给模型的消息边界。
pub(crate) fn apply_context_compaction(
    mut messages: Vec<ChatMessage>,
    compaction: &ContextCompaction,
) -> Vec<ChatMessage> {
    messages.retain(|message| {
        message.role == "system" || message.timestamp >= compaction.preserve_from_timestamp
    });
    let summary_block = format!(
        "【此前对话的压缩交接摘要】\n{}\n【摘要结束；请以最近原始消息为准】",
        compaction.summary
    );
    if let Some(system) = messages.iter_mut().find(|message| message.role == "system") {
        // Anthropic 的 system 字段在请求构造时只取一条，必须在这里合并，
        // 不能再单独插入第二条 system 消息。
        system.content = format!("{}\n\n{}", system.content, summary_block);
    } else {
        messages.insert(0, ChatMessage {
            id: Uuid::new_v4().to_string(),
            role: "system".to_string(),
            content: summary_block,
            timestamp: chrono::Utc::now().timestamp_millis(),
            error: None,
            images: vec![],
            videos: vec![],
            token_usage: None,
            tool_calls: None,
        });
    }
    messages
}

/// 手动创建上下文摘要并持久化活动上下文边界。该操作不删除任何历史消息。
#[tauri::command]
pub async fn compact_chat_context(
    request: CompactChatContextRequest,
    state: tauri::State<'_, DbState>,
) -> Result<CompactChatContextResult, LLMError> {
    let non_system: Vec<&ChatMessage> = request
        .messages
        .iter()
        .filter(|message| message.role != "system" && !message.content.trim().is_empty())
        .collect();
    if non_system.len() < 4 {
        return Err(LLMError::ApiError("至少需要两轮完整对话才能压缩上下文".to_string()));
    }

    let preserve_from_timestamp = non_system
        .iter()
        .rev()
        .take(6)
        .last()
        .map(|message| message.timestamp)
        .unwrap_or(0);
    let focus = request.focus.trim();
    let source = build_compaction_source(&request.messages);
    let prompt = if focus.is_empty() {
        format!("请压缩下面的完整历史。\n{source}")
    } else {
        format!("压缩重点：{focus}\n\n请压缩下面的完整历史。\n{source}")
    };
    let api_key = get_api_key_for_config(&request.provider, &request.api_config_id)?;
    let native_messages = vec![serde_json::json!({ "role": "user", "content": prompt })];
    let outcome = run_turn(
        &request.provider,
        &request.model,
        &api_key,
        &request.base_url,
        Some(COMPACTION_SYSTEM_PROMPT),
        &native_messages,
        &[],
        Some(request.max_tokens.unwrap_or(3_000).clamp(1_000, 6_000)),
        false,
        true,
    )
    .await?;
    let summary = match outcome {
        TurnOutcome::Text(text) if !text.trim().is_empty() => text,
        TurnOutcome::Text(_) => return Err(LLMError::ApiError("模型没有返回可用的上下文摘要".to_string())),
        TurnOutcome::ToolCalls(_) => return Err(LLMError::ApiError("压缩模型意外请求了工具调用".to_string())),
    };
    let compaction = ContextCompaction {
        summary: summary.clone(),
        preserve_from_timestamp,
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    {
        let db = state.0.lock().await;
        db.save_context_compaction(&request.session_id, &compaction)
            .map_err(|e| LLMError::ApiError(format!("保存上下文摘要失败: {e}")))?;
    }
    log::info!("[LLM] Context compacted: session={} preserve_from={}", request.session_id, preserve_from_timestamp);
    Ok(CompactChatContextResult { summary, preserve_from_timestamp })
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

    #[tokio::test]
    async fn cancellation_interrupts_an_in_flight_request_before_timeout() {
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            // 故意不返回响应头，模拟一个仍保持连接但迟迟没有结果的模型端点。
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let request = client.post(format!("http://{address}/v1/chat/completions")).body("{}");
        let cancel = CancellationToken::new();
        let cancel_from_task = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cancel_from_task.cancel();
        });

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            send_with_retry(&request, 3, 1, Some(&cancel)),
        )
        .await
        .expect("cancellation should finish well before the HTTP timeout");

        assert!(matches!(result, Err(LLMError::StreamError(ref message)) if message == "请求已取消"));
        assert!(started.elapsed() < Duration::from_secs(2));
        server.abort();
    }

    #[tokio::test]
    async fn timeout_retries_exactly_the_configured_number_of_times() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = std::sync::Arc::new(AtomicUsize::new(0));
        let attempts_from_server = attempts.clone();
        let server = tokio::spawn(async move {
            // retry_count=2 表示首次请求失败后再试 2 次，总共应建立 3 次连接。
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                attempts_from_server.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut request = [0u8; 1024];
                    let _ = socket.read(&mut request).await;
                    // 保持连接但不返回响应，稳定触发客户端总超时。
                    tokio::time::sleep(Duration::from_secs(2)).await;
                });
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let request = client.post(format!("http://{address}/v1/chat/completions")).body("{}");
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            send_with_retry(&request, 2, 0, None),
        )
        .await
        .expect("short test timeout and retries should finish within the test deadline");

        assert!(matches!(result, Err(LLMError::RequestError(ref error)) if error.is_timeout()));
        server.await.unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn total_timeout_caps_all_retries_instead_of_resetting_per_attempt() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::AsyncReadExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = std::sync::Arc::new(AtomicUsize::new(0));
        let attempts_from_server = attempts.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                attempts_from_server.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut request = [0u8; 1024];
                    let _ = socket.read(&mut request).await;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                });
            }
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let request = client.post(format!("http://{address}/v1/chat/completions")).body("{}");
        let started = std::time::Instant::now();
        let result = with_total_timeout(
            Duration::from_millis(250),
            send_with_retry(&request, 10, 0, None),
        )
        .await;

        assert!(matches!(result, Err(LLMError::TotalTimeout(_))));
        assert!(started.elapsed() < Duration::from_secs(1));
        let observed = attempts.load(Ordering::SeqCst);
        assert!((2..=3).contains(&observed), "total budget should stop retries early; observed {observed}");
        server.abort();
    }

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
            timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None,
            tool_calls: None,
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
            timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None,
            tool_calls: None,
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
            timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None,
            tool_calls: None,
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
            timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None,
            tool_calls: None,
        }];
        let body = build_stream_request_body("openai", "gpt-4o", &messages, &[sample_tool()], false, None);
        let tools = body["tools"].as_array().expect("tools should be an array");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn openai_usage_chunk_is_parsed_before_empty_choices() {
        let parsed = parse_sse_line(
            "openai",
            r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20}}"#,
        );
        match parsed {
            Some(StreamContent::Usage(usage)) => assert_eq!(usage.total_tokens, 20),
            _ => panic!("expected OpenAI usage chunk"),
        }
    }

    #[test]
    fn openai_tool_calls_not_swallowed_by_empty_content_field() {
        // Ollama 在携带 tool_calls 的 chunk 里同时带 `"content": ""`（OpenAI
        // 官方是 null）。曾经的解析顺序先命中 content 分支，空串直接 return，
        // 同一 chunk 的 tool_calls 被整个吞掉。
        let parsed = parse_sse_line(
            "local",
            r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","content":"","tool_calls":[{"id":"call_1","index":0,"type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"SF\"}"}}]},"finish_reason":null}]}"#,
        ).expect("tool_calls must be parsed even when content is an empty string");
        match parsed {
            StreamContent::ToolCallDeltas(deltas) => {
                assert_eq!(deltas.len(), 1);
                assert_eq!(deltas[0].id.as_deref(), Some("call_1"));
                assert_eq!(deltas[0].name.as_deref(), Some("get_weather"));
            }
            other => panic!("expected ToolCallDeltas, got {:?}", other),
        }
    }

    #[test]
    fn openai_reasoning_deltas_parse_as_thinking() {
        // Ollama 用 `reasoning`，DeepSeek/SiliconFlow 系用 `reasoning_content`；
        // 两种拼写都必须归到 Thinking，而不是被当作空 content 丢弃。
        let ollama = parse_sse_line(
            "local",
            r#"data: {"choices":[{"index":0,"delta":{"role":"assistant","content":"","reasoning":"想一想"},"finish_reason":null}]}"#,
        );
        assert!(matches!(ollama, Some(StreamContent::Thinking(ref s)) if s == "想一想"));

        let deepseek = parse_sse_line(
            "deepseek",
            r#"data: {"choices":[{"index":0,"delta":{"content":null,"reasoning_content":"hmm"},"finish_reason":null}]}"#,
        );
        assert!(matches!(deepseek, Some(StreamContent::Thinking(ref s)) if s == "hmm"));

        // 普通正文 chunk 不受影响
        let text = parse_sse_line(
            "local",
            r#"data: {"choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}"#,
        );
        assert!(matches!(text, Some(StreamContent::Text(ref s)) if s == "你好"));
    }

    #[test]
    fn anthropic_thinking_delta_parses_as_thinking() {
        let parsed = parse_sse_line(
            "anthropic",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think"}}"#,
        );
        assert!(matches!(parsed, Some(StreamContent::Thinking(ref s)) if s == "Let me think"));
    }

    #[test]
    fn local_providers_get_reasoning_effort_none_only_when_thinking_disabled() {
        let messages = vec![ChatMessage {
            id: "1".into(), role: "user".into(), content: "hi".into(),
            timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None,
            tool_calls: None,
        }];

        // 本地服务 + 思考关闭：显式关思考（LM Studio 上 qwen3.5 这类默认思考
        // 的模型只有 reasoning_effort 能关掉，chat_template_kwargs 和 /no_think
        // 实测均无效）
        let body = build_stream_request_body("local", "qwen3.5-9b", &messages, &[], false, None);
        assert_eq!(body["reasoning_effort"], "none");

        // 本地服务 + 思考开启：不干预，让模型按默认思考
        let body = build_stream_request_body("local", "qwen3.5-9b", &messages, &[], true, None);
        assert!(body.get("reasoning_effort").is_none());

        // 远端商业 provider 一律不发该字段（Mistral 等严格实现会拒收未知字段）
        let body = build_stream_request_body("openai", "gpt-4o", &messages, &[], false, None);
        assert!(body.get("reasoning_effort").is_none());

        // Workspace Agent 的非流式路径同款
        let native = build_native_messages("local", &messages);
        let body = build_run_turn_body("local", "qwen3.5-9b", None, &native, &[], None, false);
        assert_eq!(body["reasoning_effort"], "none");
        let body = build_run_turn_body("local", "qwen3.5-9b", None, &native, &[], None, true);
        assert!(body.get("reasoning_effort").is_none());
    }

    fn image_message() -> ChatMessage {
        ChatMessage {
            id: "1".into(), role: "user".into(), content: "what is this".into(),
            timestamp: 0, error: None,
            images: vec![ImageAttachment { data: "AAAA".into(), media_type: "image/png".into() }],
            videos: vec![],
            token_usage: None,
            tool_calls: None,
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
            ChatMessage { id: "0".into(), role: "system".into(), content: "be nice".into(), timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None, tool_calls: None },
            ChatMessage { id: "1".into(), role: "user".into(), content: "hi".into(), timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None, tool_calls: None },
            ChatMessage { id: "2".into(), role: "assistant".into(), content: "hello".into(), timestamp: 0, error: None, images: vec![], videos: vec![], token_usage: None, tool_calls: None },
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
    // -----------------------------------------------------------------
    // 常规聊天流程（不是上面用 `append_tool_round` 的 Workspace Agent 循环）在
    // MCP 工具调用之后发给模型的续写请求，现在也是流式的。下面这些测试用真实的
    // 本地 HTTP 服务器充当 provider，配合真实的 MCP 工具结果（而不是手写的
    // 测试夹具），来证明工具的输出确实以模型期望的形状发到了网络上，并且
    // 模型的最终回复能一段一段正确地流回来。
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

    #[test]
    fn memory_prompt_preserves_system_for_every_provider() {
        let mut messages=vec![msg("system","原始用户规则"),msg("user","回忆偏好")];
        append_memory_prompt(&mut messages,"记忆参考内容".into());
        for provider in ["openai","anthropic","google"] {
            let body=build_stream_request_body(provider,"test-model",&messages,&crate::memory::tool_defs(),false,None).to_string();
            assert!(body.contains("原始用户规则"),"{provider}");
            assert!(body.contains("记忆参考内容"),"{provider}");
            assert!(body.contains("memory_search"),"{provider}");
        }
    }

    #[tokio::test]
    async fn memory_auto_compaction_preserves_recent_turns_and_previous_summary() {
        let (url,captured)=mock_llm_server(vec![serde_json::json!({"choices":[{"message":{"content":"保留决策：预算 500 元，下一步确认行程。"}}]})]).await;
        let mut messages=vec![msg("system","此前摘要：预算 500 元")];
        for i in 0..12 { let mut m=msg(if i%2==0 {"user"} else {"assistant"},&format!("第 {i} 轮行程安排"));m.timestamp=i+1;messages.push(m); }
        let original=messages.clone();
        let summary=auto_compact_messages("local","test","",&url,&messages,160,&CancellationToken::new()).await.unwrap().unwrap();
        let active=apply_context_compaction(messages,&summary);
        assert_eq!(active.iter().filter(|m|m.role!="system").count(),6);
        assert_eq!(active.last().unwrap().content,original.last().unwrap().content);
        assert!(active[0].content.contains("保留决策"));
        assert_eq!(original.len(),13);
        let requests=captured.lock().await;
        let request=requests[0].to_string();
        assert!(request.contains("此前摘要"));
        assert!(!request.contains("第 11 轮"));
    }

    #[tokio::test]
    async fn memory_auto_compaction_handles_one_oversized_message_before_nine_turns() {
        let (url, captured) = mock_llm_server(vec![serde_json::json!({
            "choices": [{ "message": { "content": "压缩摘要保留：超长材料暗号海豚-418。" } }]
        })]).await;
        let mut messages = vec![msg("system", "保留系统规则")];
        let mut long = msg("user", &format!("{}\n超长材料暗号：海豚-418。", "重复材料。".repeat(900)));
        long.timestamp = 1;
        messages.push(long);
        let mut assistant = msg("assistant", "已收到超长材料"); assistant.timestamp = 2; messages.push(assistant);
        let mut latest = msg("user", "请继续处理刚才的材料"); latest.timestamp = 3; messages.push(latest.clone());
        let summary = auto_compact_messages("local", "test", "", &url, &messages, 1000, &CancellationToken::new()).await.unwrap().unwrap();
        let active = apply_context_compaction(messages, &summary);
        assert_eq!(active.iter().filter(|m| m.role != "system").count(), 1);
        assert_eq!(active.last().unwrap().content, latest.content);
        assert!(active[0].content.contains("海豚-418"));
        assert!(captured.lock().await[0].to_string().contains("海豚-418"));
    }

    // ---- 流式续写：下面的测试用真实的本地 SSE 服务器验证 `run_streaming_request` ----

    /// 读出一个 HTTP 请求的 JSON body（SSE mock 服务器共用）。
    async fn read_mock_request(socket: &mut tokio::net::TcpStream) -> Option<serde_json::Value> {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let headers_end = loop {
            let n = socket.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let header_str = String::from_utf8_lossy(&buf[..headers_end]).to_string();
        let content_length: usize = header_str
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.trim().eq_ignore_ascii_case("content-length") { v.trim().parse().ok() } else { None }
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
        serde_json::from_slice(&body).ok()
    }

    /// 流式（SSE）mock 服务器：每个连接按脚本依次写出若干个分片（分片之间留一点
    /// 间隔，让客户端真的分多次读到），然后关闭连接。同时捕获每个请求的 JSON body。
    async fn mock_sse_server(scripts: Vec<Vec<Vec<u8>>>) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock sse server");
        let addr = listener.local_addr().expect("local_addr");
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        tokio::spawn(async move {
            for script in scripts {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                if let Some(body) = read_mock_request(&mut socket).await {
                    captured_clone.lock().await.push(body);
                }
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                    .await;
                for chunk in script {
                    let _ = socket.write_all(&chunk).await;
                    let _ = socket.flush().await;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let _ = socket.shutdown().await;
            }
        });
        (format!("http://{}", addr), captured)
    }

    /// 发出第一个分片后就不再写任何数据、也不关闭连接的服务器，用来模拟"模型还在想"。
    async fn mock_stalling_sse_server(first_chunk: Vec<u8>) -> String {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind stalling server");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let _ = read_mock_request(&mut socket).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(&first_chunk).await;
            let _ = socket.flush().await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        format!("http://{}", addr)
    }

    /// 每个事件一个分片：`data: ...\n\n`。
    fn sse_chunks(events: &[&str]) -> Vec<Vec<u8>> {
        events.iter().map(|event| format!("{event}\n\n").into_bytes()).collect()
    }

    async fn collect_turn(provider: &str, url: &str, body: &serde_json::Value) -> (StreamedTurn, Vec<TurnDelta>) {
        let mut deltas: Vec<TurnDelta> = Vec::new();
        let mut on_delta = |delta: TurnDelta| deltas.push(delta);
        let result = run_streaming_request(provider, url, body, "test-key", 0, 0, &CancellationToken::new(), &mut on_delta).await;
        match result {
            Ok(StreamedTurnEnd::Finished(turn)) => (turn, deltas),
            other => panic!("expected a finished turn, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn streaming_request_collects_text_tool_calls_and_usage_for_openai_shape() {
        let (url, captured) = mock_sse_server(vec![sse_chunks(&[
            r#"data: {"choices":[{"delta":{"content":"我先算一下。"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"demo_calculator","arguments":""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"a\":3,"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"b\":4}"}}]}}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#,
            "data: [DONE]",
        ])])
        .await;

        let body = serde_json::json!({ "model": "m", "stream": true });
        let (turn, deltas) = collect_turn("custom", &url, &body).await;

        assert_eq!(deltas, vec![TurnDelta::Text("我先算一下。".into())]);
        assert_eq!(turn.text, "我先算一下。", "模型随工具调用一起说的话必须保留下来");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call_1");
        assert_eq!(turn.tool_calls[0].function.name, "demo_calculator");
        assert_eq!(turn.tool_calls[0].function.arguments, "{\"a\":3,\"b\":4}");
        assert_eq!(turn.usage, Some(TokenUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }));
        assert_eq!(captured.lock().await[0], body, "请求体必须原样发给服务器");
    }

    #[tokio::test]
    async fn streaming_request_does_not_garble_a_multibyte_char_split_across_network_chunks() {
        let line = r#"data: {"choices":[{"delta":{"content":"你好世界"}}]}"#.to_string() + "\n\n";
        let bytes = line.into_bytes();
        // 在"好"的 3 个字节中间切开，模拟一个汉字被拆在两个网络分片之间。
        let good = "好".as_bytes();
        let at = bytes.windows(good.len()).position(|w| w == good).expect("find 好") + 1;
        let (url, _) = mock_sse_server(vec![vec![
            bytes[..at].to_vec(),
            bytes[at..].to_vec(),
            b"data: [DONE]\n\n".to_vec(),
        ]])
        .await;

        let (turn, deltas) = collect_turn("custom", &url, &serde_json::json!({})).await;

        assert_eq!(turn.text, "你好世界");
        assert!(!turn.text.contains('\u{FFFD}'), "不能出现乱码替换符");
        assert_eq!(deltas, vec![TurnDelta::Text("你好世界".into())]);
    }

    #[tokio::test]
    async fn streaming_request_parses_anthropic_text_and_tool_use() {
        let (url, _) = mock_sse_server(vec![sse_chunks(&[
            r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"让我算一下。"}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"demo_calculator","input":{}}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1"}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":",\"b\":2}"}}"#,
            r#"event: message_stop
data: {"type":"message_stop"}"#,
        ])])
        .await;

        let (turn, _) = collect_turn("anthropic", &url, &serde_json::json!({})).await;

        assert_eq!(turn.text, "让我算一下。");
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "toolu_1");
        assert_eq!(turn.tool_calls[0].function.arguments, "{\"a\":1,\"b\":2}");
    }

    #[tokio::test]
    async fn streaming_request_keeps_gemini_calls_that_arrive_in_separate_chunks_apart() {
        // Gemini 每个 chunk 里的 part 序号都从 0 开始；不能因此把两个调用的参数拼到一起。
        // 这个流也没有任何结束信号，靠连接关闭收尾。
        let (url, _) = mock_sse_server(vec![sse_chunks(&[
            r#"data: {"candidates":[{"content":{"parts":[{"text":"先查两个。"}]}}]}"#,
            r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"tool_a","args":{"x":1}}}]}}]}"#,
            r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"tool_b","args":{"y":2}}}]}}]}"#,
        ])])
        .await;

        let (turn, _) = collect_turn("google", &url, &serde_json::json!({})).await;

        assert_eq!(turn.text, "先查两个。");
        assert_eq!(turn.tool_calls.len(), 2);
        assert_eq!(turn.tool_calls[0].function.name, "tool_a");
        assert_eq!(turn.tool_calls[0].function.arguments, "{\"x\":1}");
        assert_eq!(turn.tool_calls[1].function.name, "tool_b");
        assert_eq!(turn.tool_calls[1].function.arguments, "{\"y\":2}");
    }

    #[tokio::test]
    async fn streaming_request_turns_empty_tool_arguments_into_an_empty_object() {
        let (url, _) = mock_sse_server(vec![sse_chunks(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"no_args_tool","arguments":""}}]}}]}"#,
            "data: [DONE]",
        ])])
        .await;

        let (turn, _) = collect_turn("custom", &url, &serde_json::json!({})).await;

        assert_eq!(turn.tool_calls[0].function.arguments, "{}");
    }

    #[tokio::test]
    async fn streaming_request_forwards_thinking_separately_and_flags_it() {
        let (url, _) = mock_sse_server(vec![sse_chunks(&[
            r#"data: {"choices":[{"delta":{"reasoning_content":"先想一想"}}]}"#,
            "data: [DONE]",
        ])])
        .await;

        let (turn, deltas) = collect_turn("custom", &url, &serde_json::json!({})).await;

        assert_eq!(deltas, vec![TurnDelta::Thinking("先想一想".into())]);
        assert!(turn.saw_thinking);
        assert!(turn.text.is_empty(), "思考内容不能混进正文");
    }

    #[tokio::test]
    async fn streaming_request_returns_cancelled_when_stopped_mid_stream() {
        let first = format!("data: {}\n\n", r#"{"choices":[{"delta":{"content":"写到一半"}}]}"#).into_bytes();
        let url = mock_stalling_sse_server(first).await;
        let cancel = CancellationToken::new();
        let canceller = cancel.clone();
        let mut deltas: Vec<TurnDelta> = Vec::new();
        let mut on_delta = |delta: TurnDelta| {
            deltas.push(delta);
            canceller.cancel(); // 用户在收到第一段文字之后点了"停止"
        };
        let body = serde_json::json!({ "stream": true });
        let run = run_streaming_request("custom", &url, &body, "test-key", 0, 0, &cancel, &mut on_delta);

        let result = tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .expect("点停止后必须及时返回，而不是等到服务器关闭连接");

        assert!(matches!(result, Ok(StreamedTurnEnd::Cancelled)), "{:?}", result);
        assert_eq!(deltas, vec![TurnDelta::Text("写到一半".into())]);
    }

    #[test]
    fn accumulate_tool_deltas_merges_fragments_and_splits_calls_with_different_ids() {
        let delta = |index, id: Option<&str>, name: Option<&str>, fragment: Option<&str>| ToolCallDelta {
            index,
            id: id.map(str::to_string),
            name: name.map(str::to_string),
            arguments_fragment: fragment.map(str::to_string),
        };
        let mut acc = std::collections::BTreeMap::new();
        // 同一个 index 的后续片段不带 id；个别 provider 会在每个片段里重复同一个 id，也不能被拆开。
        accumulate_tool_deltas(&mut acc, vec![delta(0, Some("a"), Some("first"), Some("{\"k\":"))]);
        accumulate_tool_deltas(&mut acc, vec![delta(0, None, None, Some("1"))]);
        accumulate_tool_deltas(&mut acc, vec![delta(0, Some("a"), None, Some("}"))]);
        // 同一个 index 上换了 id，是另一个调用。
        accumulate_tool_deltas(&mut acc, vec![delta(0, Some("b"), Some("second"), Some("{}"))]);

        let calls = finish_tool_calls(acc);
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0].id.as_str(), calls[0].function.name.as_str(), calls[0].function.arguments.as_str()), ("a", "first", "{\"k\":1}"));
        assert_eq!((calls[1].id.as_str(), calls[1].function.name.as_str(), calls[1].function.arguments.as_str()), ("b", "second", "{}"));
    }

    #[test]
    fn add_usage_sums_every_round_of_a_reply() {
        let mut total = None;
        add_usage(&mut total, None);
        assert_eq!(total, None);
        add_usage(&mut total, Some(TokenUsage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }));
        add_usage(&mut total, Some(TokenUsage { prompt_tokens: 30, completion_tokens: 8, total_tokens: 38 }));
        assert_eq!(total, Some(TokenUsage { prompt_tokens: 40, completion_tokens: 13, total_tokens: 53 }));
    }

    #[test]
    fn enforce_round_tool_budget_keeps_the_newest_results_and_elides_older_ones() {
        let call = |id: &str| ToolCall { id: id.into(), function: ToolFunction { name: "baiyu_file_write".into(), arguments: "{}".into() } };
        let big = |tag: &str| serde_json::json!({ "content": format!("{}{}", tag, "x".repeat(30_000)) });
        let mut rounds = vec![
            ToolRound { text: String::new(), calls: vec![call("old")], results: vec![big("OLD")] },
            ToolRound { text: String::new(), calls: vec![call("new")], results: vec![big("NEW")] },
        ];

        enforce_round_tool_budget(&mut rounds);

        assert!(rounds[1].results[0].to_string().contains("NEW"), "最近一轮的结果要完整保留");
        assert_eq!(rounds[0].results[0]["_truncated"], true, "超出预算时较早的结果要退化成元信息");
        assert_eq!(rounds[0].results[0]["tool"], "baiyu_file_write");
    }

    // ---- 续写请求体：每家 provider 的形状 ----

    fn sample_round(text: &str) -> ToolRound {
        ToolRound {
            text: text.to_string(),
            calls: vec![ToolCall {
                id: "call_1".into(),
                function: ToolFunction { name: "demo_calculator".into(), arguments: "{\"a\":3,\"b\":4}".into() },
            }],
            results: vec![serde_json::json!({ "result": 7.0 })],
        }
    }

    #[test]
    fn continuation_body_openai_shape_is_streaming_and_replays_the_models_own_words() {
        let original = vec![msg("user", "3加4等于多少")];
        let body = build_continuation_body("custom", "m", &original, &[sample_round("我来算一下。")], &[], &[], Some(512), false);

        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["max_tokens"], 512);
        let messages = body["messages"].as_array().unwrap();
        let assistant = messages.iter().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["content"], "我来算一下。", "模型随工具调用说的话要放回它自己那条消息里");
        assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
        let tool = messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool["tool_call_id"], "call_1");
        assert!(body.get("tool_choice").is_none());

        // 模型这一轮没说话：content 必须是 null，而不是空字符串。
        let silent = build_continuation_body("custom", "m", &original, &[sample_round("  ")], &[], &[], None, false);
        let assistant = silent["messages"].as_array().unwrap().iter().find(|m| m["role"] == "assistant").unwrap().clone();
        assert!(assistant["content"].is_null());
    }

    #[test]
    fn continuation_body_force_final_forbids_tools_for_every_provider() {
        let original = vec![msg("user", "hi")];
        let rounds = [sample_round("")];

        let openai = build_continuation_body("custom", "m", &original, &rounds, &[], &[], None, true);
        assert_eq!(openai["tool_choice"], "none");
        assert!(openai["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().contains("工具调用次数已达上限"));

        let anthropic = build_continuation_body("anthropic", "claude", &original, &rounds, &[], &[], None, true);
        assert_eq!(anthropic["tool_choice"]["type"], "none");

        let google = build_continuation_body("google", "gemini", &original, &rounds, &[], &[], None, true);
        assert_eq!(google["toolConfig"]["functionCallingConfig"]["mode"], "NONE");
    }

    #[test]
    fn continuation_body_anthropic_puts_the_text_block_before_tool_use_and_streams() {
        let original = vec![msg("system", "你是助手"), msg("user", "算一下")];
        let body = build_continuation_body("anthropic", "claude", &original, &[sample_round("我来算一下。")], &[], &[], None, false);

        assert_eq!(body["stream"], true);
        assert_eq!(body["system"], "你是助手");
        let messages = body["messages"].as_array().unwrap();
        let assistant = messages.iter().rev().find(|m| m["role"] == "assistant").unwrap();
        assert_eq!(assistant["content"][0]["type"], "text");
        assert_eq!(assistant["content"][0]["text"], "我来算一下。");
        assert_eq!(assistant["content"][1]["type"], "tool_use");
        assert_eq!(assistant["content"][1]["id"], "call_1");
        let result_message = messages.last().unwrap();
        assert_eq!(result_message["role"], "user");
        assert_eq!(result_message["content"][0]["type"], "tool_result");

        // 这一轮没说话：不能出现空文本块（Anthropic 会拒收）。
        let silent = build_continuation_body("anthropic", "claude", &original, &[sample_round("")], &[], &[], None, false);
        let assistant = silent["messages"].as_array().unwrap().iter().rev().find(|m| m["role"] == "assistant").unwrap().clone();
        assert_eq!(assistant["content"].as_array().unwrap().len(), 1);
        assert_eq!(assistant["content"][0]["type"], "tool_use");
    }

    #[test]
    fn continuation_body_google_replays_text_then_function_call_and_has_no_stream_field() {
        let original = vec![msg("user", "算一下")];
        let body = build_continuation_body("google", "gemini", &original, &[sample_round("我来算一下。")], &[], &[], None, false);

        // Gemini 靠 URL（streamGenerateContent）区分流式，请求体里没有 stream 字段。
        assert!(body.get("stream").is_none());
        let contents = body["contents"].as_array().unwrap();
        let model_turn = contents.iter().rev().find(|c| c["role"] == "model").unwrap();
        assert_eq!(model_turn["parts"][0]["text"], "我来算一下。");
        assert_eq!(model_turn["parts"][1]["functionCall"]["name"], "demo_calculator");
        assert_eq!(contents.last().unwrap()["parts"][0]["functionResponse"]["name"], "demo_calculator");
        assert!(build_url("google", "", "gemini", true).contains("streamGenerateContent?alt=sse"));
    }

    // ---- 流式续写的完整往返（配合真实的 MCP 工具结果） ----

    #[tokio::test]
    async fn streaming_continuation_embeds_real_mcp_result_and_streams_the_final_answer() {
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

        let (base_url, captured) = mock_sse_server(vec![sse_chunks(&[
            r#"data: {"choices":[{"delta":{"content":"3加4"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"等于7。"}}]}"#,
            "data: [DONE]",
        ])])
        .await;

        let original_messages = vec![msg("system", "你是一个助手"), msg("user", "3加4等于多少？用计算器工具算一下")];
        let round = ToolRound {
            text: "好，我用计算器算一下。".to_string(),
            calls: vec![ToolCall {
                id: "call_1".to_string(),
                function: ToolFunction {
                    name: "demo_calculator".to_string(),
                    arguments: serde_json::json!({ "a": 3, "b": 4, "operation": "add" }).to_string(),
                },
            }],
            results: vec![real_result.clone()],
        };
        let body = build_continuation_body("custom", "test-model", &original_messages, &[round], &[], &[], None, false);

        let (turn, deltas) = collect_turn("custom", &base_url, &body).await;

        assert_eq!(turn.text, "3加4等于7。");
        assert!(turn.tool_calls.is_empty(), "这一轮是最终回答");
        assert_eq!(deltas.len(), 2, "最终回答是一段一段流出来的，而不是一次性返回");

        // 精确检查续写请求实际发到网络上的内容——这才是证明工具的真实输出
        // 确实到达了模型上下文，而不只是我们的代码自己以为它到达了。
        let sent = captured.lock().await;
        assert_eq!(sent[0]["stream"], true);
        let sent_messages = sent[0]["messages"].as_array().expect("messages array");

        let assistant_msg = sent_messages
            .iter()
            .find(|m| m["role"] == "assistant" && m["tool_calls"].is_array())
            .expect("assistant tool_calls message present");
        assert_eq!(assistant_msg["content"], "好，我用计算器算一下。");
        assert_eq!(assistant_msg["tool_calls"][0]["id"], "call_1");
        assert_eq!(assistant_msg["tool_calls"][0]["function"]["name"], "demo_calculator");

        let tool_msg = sent_messages.iter().find(|m| m["role"] == "tool").expect("tool result message present");
        assert_eq!(tool_msg["tool_call_id"], "call_1");
        let embedded_result: serde_json::Value = serde_json::from_str(
            tool_msg["content"].as_str().expect("tool content must be a string for OpenAI-shape providers"),
        )
        .expect("tool content must be valid JSON");
        assert_eq!(embedded_result, real_result, "the exact MCP tool result must round-trip into the model context untouched");
    }

    #[tokio::test]
    async fn streaming_continuation_supports_a_second_round_after_more_tool_calls() {
        // 模型在看到第一个结果之后，确实可能合理地要求再调用一个工具
        // （finalize_turn 会循环最多 MAX_TOOL_ROUNDS 次）；这里验证第二次
        // 续写请求能正确把两轮结果、以及两轮里模型说过的话都叠加进去。
        let result_1 = crate::commands::mcp::handle_demo_tool_call(
            "demo_calculator", serde_json::json!({"a": 3, "b": 4, "operation": "add"}), "r1",
        ).await.unwrap();
        let result_2 = crate::commands::mcp::handle_demo_tool_call(
            "demo_calculator", serde_json::json!({"a": 7, "b": 5, "operation": "multiply"}), "r2",
        ).await.unwrap();

        let (base_url, captured) = mock_sse_server(vec![
            sse_chunks(&[
                r#"data: {"choices":[{"delta":{"content":"加法是7，再乘以5。"}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_2","function":{"name":"demo_calculator","arguments":"{\"a\":7,\"b\":5,\"operation\":\"multiply\"}"}}]}}]}"#,
                "data: [DONE]",
            ]),
            sse_chunks(&[
                r#"data: {"choices":[{"delta":{"content":"3+4=7，再乘以5等于35。"}}]}"#,
                "data: [DONE]",
            ]),
        ])
        .await;

        let original_messages = vec![msg("user", "先加后乘")];
        let call_1 = ToolCall {
            id: "call_1".into(),
            function: ToolFunction { name: "demo_calculator".into(), arguments: "{\"a\":3,\"b\":4,\"operation\":\"add\"}".into() },
        };
        let mut rounds = vec![ToolRound { text: "先做加法。".into(), calls: vec![call_1], results: vec![result_1] }];

        let body_1 = build_continuation_body("custom", "test-model", &original_messages, &rounds, &[], &[], None, false);
        let (turn_1, _) = collect_turn("custom", &base_url, &body_1).await;
        assert_eq!(turn_1.text, "加法是7，再乘以5。", "带着工具调用一起说的话不能丢");
        assert_eq!(turn_1.tool_calls.len(), 1);
        assert_eq!(turn_1.tool_calls[0].id, "call_2");

        rounds.push(ToolRound { text: turn_1.text, calls: turn_1.tool_calls, results: vec![result_2] });
        let body_2 = build_continuation_body("custom", "test-model", &original_messages, &rounds, &[], &[], None, false);
        let (turn_2, _) = collect_turn("custom", &base_url, &body_2).await;
        assert_eq!(turn_2.text, "3+4=7，再乘以5等于35。");
        assert!(turn_2.tool_calls.is_empty());

        let sent = captured.lock().await;
        let second_request_messages = sent[1]["messages"].as_array().expect("messages array");
        let tool_msgs: Vec<_> = second_request_messages.iter().filter(|m| m["role"] == "tool").collect();
        assert_eq!(tool_msgs.len(), 2, "both rounds' tool results must both be present in the second follow-up request");
        assert_eq!(tool_msgs[0]["tool_call_id"], "call_1");
        assert_eq!(tool_msgs[1]["tool_call_id"], "call_2");
        let assistant_texts: Vec<_> = second_request_messages
            .iter()
            .filter(|m| m["role"] == "assistant")
            .map(|m| m["content"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(assistant_texts, vec!["先做加法。", "加法是7，再乘以5。"]);
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

        // SiliconFlow 服务端默认开启思考；关闭时同样必须明确传 false。
        let sf_without_thinking = build_run_turn_body("siliconflow", "qwen3", None, &msgs, &[], None, false);
        assert_eq!(sf_without_thinking["enable_thinking"], false);
        assert!(sf_without_thinking.get("thinking_budget").is_none());

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

    #[test]
    fn compact_tool_result_keeps_small_values_and_limits_large_payloads() {
        let small = serde_json::json!({ "answer": 42, "unit": "km" });
        assert_eq!(compact_tool_result(&small, "calculator"), small);

        let large = serde_json::json!({ "html": "x".repeat(40_000), "base64": "A".repeat(40_000), "items": (0..30).collect::<Vec<_>>() });
        let compacted = compact_tool_result(&large, "external_mcp_tool");
        assert!(compacted.to_string().len() <= TOOL_RESULT_CHAR_BUDGET + 128);
        assert_eq!(compacted.get("base64"), None);
    }

    #[test]
    fn web_display_never_exposes_page_body() {
        let result = serde_json::json!({ "title": "Example", "url": "https://example.test", "content": "private body" });
        let display = tool_display_result(&result, "web_search");
        assert_eq!(display["display_type"], "web_search");
        assert_eq!(display["title"], "Example");
        assert!(display.get("content").is_none());
    }

    #[test]
    fn web_search_display_uses_first_result_title_and_count() {
        let result = serde_json::json!({ "query": "OpenAI", "results": [
            { "title": "OpenAI", "url": "https://openai.com", "snippet": "ignored in UI" },
            { "title": "News", "url": "https://openai.com/news" }
        ] });
        let display = tool_display_result(&result, "builtin__web_search");
        assert_eq!(display["title"], "OpenAI");
        assert_eq!(display["url"], "https://openai.com");
        assert_eq!(display["result_count"], 2);
        let context = compact_tool_result(&result, "builtin__web_search");
        assert_eq!(context["results"][0]["title"], "OpenAI");
        assert_eq!(context["results"][0]["url"], "https://openai.com");
    }

    #[test]
    fn context_compaction_replaces_old_history_with_summary() {
        let mut messages = vec![ChatMessage {
            id: "system".into(),
            role: "system".into(),
            content: "你是一个可靠的助手".into(),
            timestamp: 0,
            error: None,
            images: vec![],
            videos: vec![],
            token_usage: None,
            tool_calls: None,
        }];
        for index in 1..=8 {
            messages.push(ChatMessage {
                id: format!("message-{index}"),
                role: if index % 2 == 0 { "assistant".into() } else { "user".into() },
                content: format!("原始消息 {index}"),
                timestamp: index * 10,
                error: None,
                images: vec![],
                videos: vec![],
                token_usage: None,
                tool_calls: None,
            });
        }

        // 假装上一步的压缩模型已经返回了这份摘要；这里不依赖真实网络或模型。
        let compaction = ContextCompaction {
            summary: "目标：完成上下文压缩测试。已完成：写入摘要。".into(),
            preserve_from_timestamp: 30,
            created_at: 999,
        };
        let effective = apply_context_compaction(messages, &compaction);

        // 系统提示仍在，1-2 两条旧原文被摘要替代，3-8 六条最近原文继续保留。
        assert_eq!(effective.len(), 7);
        assert_eq!(effective[0].role, "system");
        assert!(effective[0].content.contains("你是一个可靠的助手"));
        assert!(effective[0].content.contains(&compaction.summary));
        assert!(effective.iter().all(|message| message.content != "原始消息 1"));
        assert!(effective.iter().all(|message| message.content != "原始消息 2"));
        for index in 3..=8 {
            assert!(effective.iter().any(|message| message.content == format!("原始消息 {index}")));
        }

        // 再检查通用 OpenAI 格式的请求体，确保这正是会被发送给模型的消息集合。
        let body = build_stream_request_body("openai", "test-model", &effective, &[], false, None);
        let request_messages = body["messages"].as_array().expect("OpenAI 请求必须带 messages 数组");
        assert_eq!(request_messages.len(), 7);
        assert!(request_messages[0]["content"].as_str().unwrap().contains("压缩交接摘要"));
        assert_eq!(request_messages[6]["content"], "原始消息 8");
    }

    #[test]
    fn compaction_response_uses_reasoning_only_when_content_is_empty() {
        let normal = serde_json::json!({
            "content": "正式摘要",
            "reasoning_content": "不应使用的推理内容"
        });
        assert_eq!(extract_compaction_response_text(&normal), ("正式摘要".to_string(), false));

        let reasoning_only = serde_json::json!({
            "content": "",
            "reasoning_content": "模型放错字段的摘要"
        });
        assert_eq!(
            extract_compaction_response_text(&reasoning_only),
            ("模型放错字段的摘要".to_string(), true)
        );
    }
}
