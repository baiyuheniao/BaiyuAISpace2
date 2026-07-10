// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * MCP (Model Context Protocol) 模块
 * 
 * 功能说明:
 * - MCP 服务器管理 (stdio/SSE/HTTP 类型)
 * - 工具列表获取
 * - 工具调用
 * - 服务器连接测试
 * 
 * MCP 服务器类型:
 * - stdio: 标准输入输出 (本地进程)
 * - SSE: 服务器发送事件
 * - HTTP: HTTP API
 */

use crate::commands::constants::{MCP_HTTP_TIMEOUT, MCP_STDIO_TIMEOUT, MCP_TOOL_CALL_TIMEOUT};
use crate::db::DbState;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// `stream_message` calls `get_all_mcp_tools` on every single chat turn
/// (needed even when MCP is off, since a manually-activated Skill can still
/// bind a server) -- without a cache that means re-running `tools/list`
/// against every enabled server before the LLM request can even be sent.
/// For stdio servers that's a fresh child process (e.g. `npx ...`) spawned
/// and torn down per message, which can cost anywhere from hundreds of ms to
/// the full `MCP_STDIO_TIMEOUT`/`MCP_HTTP_TIMEOUT` ceiling if the server is
/// slow to start or briefly unreachable -- directly inflating TTFT. Cache
/// successful lookups for a short TTL; failures are never cached so a
/// misbehaving server keeps surfacing instead of silently vanishing.
static MCP_TOOLS_CACHE: Lazy<Mutex<HashMap<String, (Vec<MCPTool>, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const MCP_TOOLS_CACHE_TTL: Duration = Duration::from_secs(300);

/// MCP 错误类型
#[derive(Error, Debug)]
pub enum MCPError {
    /// 服务器未找到
    #[error("MCP server not found: {0}")]
    ServerNotFound(String),
    /// 启动服务器失败
    #[error("Failed to launch MCP server: {0}")]
    LaunchError(String),
    /// 通信错误
    #[error("MCP communication error: {0}")]
    CommunicationError(String),
    /// 配置无效
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    /// JSON 解析错误
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    /// HTTP 请求错误
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
}

/// 实现 Serialize trait 用于 Tauri 命令返回
impl Serialize for MCPError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServer {
    /// 服务器 ID
    pub id: String,
    /// 服务器名称
    pub name: String,
    /// 服务器描述
    pub description: String,
    /// 服务器类型 (stdio/SSE/HTTP)
    pub server_type: MCPServerType,
    /// 启动命令 (stdio 类型使用)
    pub command: String,
    /// 命令行参数
    pub args: Vec<String>,
    /// 环境变量
    pub env: HashMap<String, String>,
    /// 端口号 (SSE/HTTP 类型使用)
    pub port: Option<u16>,
    /// 服务器 URL (HTTP 类型使用)
    pub url: Option<String>,
    /// API 密钥 (可选)
    pub api_key: Option<String>,
    /// 是否启用
    pub enabled: bool,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
}

/// MCP 服务器类型枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MCPServerType {
    /// 标准输入输出 (本地进程)
    #[serde(rename = "stdio")]
    Stdio,
    /// 服务器发送事件
    #[serde(rename = "sse")]
    SSE,
    /// HTTP API
    #[serde(rename = "http")]
    HTTP,
}

/// MCP 工具定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPTool {
    /// 所属服务器 ID
    pub server_id: String,
    /// 服务器名称
    pub server_name: String,
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
    /// 输入参数 JSON Schema
    pub input_schema: serde_json::Value,
}

/// MCP 工具调用请求
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCall {
    pub server_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// MCP Tool Call Result
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolResult {
    pub tool_name: String,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

/// JSON-RPC 2.0 Request for MCP tools/call
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallRequest {
    jsonrpc: String,
    method: String,
    params: ToolCallParams,
    id: String,
}

/// Parameters for tools/call method
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallParams {
    name: String,
    arguments: serde_json::Value,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: Option<String>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcErrorData>,
    id: Option<String>,
}

/// JSON-RPC 2.0 Error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcErrorData {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

// Tauri Commands

/// Create or update MCP server configuration
#[tauri::command]
pub async fn create_mcp_server(
    state: tauri::State<'_, DbState>,
    server: MCPServer,
) -> Result<MCPServer, MCPError> {
    // Validate configuration
    match server.server_type {
        MCPServerType::Stdio => {
            if server.command.is_empty() {
                return Err(MCPError::InvalidConfig(
                    "stdio server requires command".to_string(),
                ));
            }
        }
        MCPServerType::SSE | MCPServerType::HTTP => {
            if server.url.is_none() || server.url.as_ref().unwrap().is_empty() {
                return Err(MCPError::InvalidConfig(
                    "HTTP/SSE server requires URL".to_string(),
                ));
            }
        }
    }

    // Generate ID if not provided
    let mut config = server;
    if config.id.is_empty() {
        config.id = Uuid::new_v4().to_string();
    }
    config.created_at = chrono::Utc::now().timestamp_millis();
    config.updated_at = chrono::Utc::now().timestamp_millis();

    // Save to database
    let db = state.0.lock().await;
    db.save_mcp_server(&config)
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    drop(db);

    // The server's command/args/url may have just changed -- drop any cached
    // tool list so the next lookup re-discovers instead of serving stale data.
    MCP_TOOLS_CACHE.lock().await.remove(&config.id);

    log::info!(
        "MCP server configured: {} (type: {}) [ID: {}]",
        config.name,
        match config.server_type {
            MCPServerType::Stdio => "stdio",
            MCPServerType::SSE => "sse",
            MCPServerType::HTTP => "http",
        },
        config.id
    );

    Ok(config)
}

/// Get list of MCP servers
#[tauri::command]
pub async fn list_mcp_servers(state: tauri::State<'_, DbState>) -> Result<Vec<MCPServer>, MCPError> {
    let db = state.0.lock().await;
    let servers = db
        .get_mcp_servers()
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    log::info!("Retrieved {} MCP servers", servers.len());
    Ok(servers)
}

/// Delete MCP server configuration
#[tauri::command]
pub async fn delete_mcp_server(
    state: tauri::State<'_, DbState>,
    server_id: String
) -> Result<(), MCPError> {
    let db = state.0.lock().await;
    db.delete_mcp_server(&server_id)
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    drop(db);
    MCP_TOOLS_CACHE.lock().await.remove(&server_id);
    log::info!("MCP server deleted: {}", server_id);
    Ok(())
}

const ALLOWED_MCP_COMMANDS: &[&str] = &[
    "npx", "npm", "node", "python", "python3", "pip", "uvx", "uv",
    "bun", "deno", "go", "cargo", "ruby", "perl", "php",
];

/// Translate a bare-runtime "program not found" spawn failure into a message
/// that actually tells the user what to install, instead of the raw OS
/// `NotFound` text -- stdio MCP servers are just "run this runtime with these
/// args", so a missing runtime is by far the most common launch failure and
/// the least self-explanatory one to a non-developer.
fn friendly_missing_runtime_message(command: &str) -> String {
    let cmd_name = Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    match cmd_name {
        "npx" | "npm" | "node" => "需要先安装 Node.js（https://nodejs.org/），安装后重启软件再试".to_string(),
        "uvx" | "uv" => "需要先安装 uv（https://docs.astral.sh/uv/），安装后重启软件再试".to_string(),
        "python" | "python3" | "pip" => "需要先安装 Python（https://www.python.org/downloads/），安装后重启软件再试".to_string(),
        "bun" => "需要先安装 Bun（https://bun.sh/），安装后重启软件再试".to_string(),
        "deno" => "需要先安装 Deno（https://deno.com/），安装后重启软件再试".to_string(),
        "go" => "需要先安装 Go（https://go.dev/dl/），安装后重启软件再试".to_string(),
        "cargo" => "需要先安装 Rust（https://rustup.rs/），安装后重启软件再试".to_string(),
        "ruby" => "需要先安装 Ruby（https://www.ruby-lang.org/），安装后重启软件再试".to_string(),
        _ => format!("系统未找到命令 \"{}\"，请确认已安装并加入系统 PATH", cmd_name),
    }
}

fn validate_mcp_command(command: &str, args: &[String]) -> Result<(), MCPError> {
    let cmd_path = Path::new(command);
    let cmd_name = cmd_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    if !ALLOWED_MCP_COMMANDS.contains(&cmd_name) {
        return Err(MCPError::LaunchError(format!(
            "Command '{}' is not allowed. Allowed commands: {:?}",
            cmd_name, ALLOWED_MCP_COMMANDS
        )));
    }

    let dangerous_patterns = ["--eval", "-e", "&&", "||", "|", ">", ">>", "<", "`", "$(", ";"];
    for arg in args {
        for pattern in dangerous_patterns {
            if arg.contains(pattern) {
                return Err(MCPError::LaunchError(format!(
                    "Argument '{}' contains dangerous pattern '{}'",
                    arg, pattern
                )));
            }
        }
    }

    Ok(())
}

/// Resolve a bare command name (e.g. "npx") to a spawnable path on Windows.
///
/// `std::process::Command::new` calls `CreateProcessW` directly and does
/// *not* do the PATHEXT-based extension search a shell does -- so commands
/// installed as `.cmd`/`.bat` shims (which is how npm installs `npx`/`npm`
/// themselves on Windows, unlike single-file `.exe` tools such as `node` or
/// `cargo`) fail to spawn with a plain "program not found" even though the
/// shim is right there on PATH. Confirmed directly: `Command::new("npx")`
/// errors with `NotFound` on Windows, while `Command::new("node")` works.
/// Search PATH for the first matching extension and spawn that exact file
/// instead. Only applied at the actual spawn call -- `validate_mcp_command`
/// still checks the original bare name against the allowlist.
#[cfg(target_os = "windows")]
fn resolve_windows_command(command: &str) -> String {
    let path = Path::new(command);
    // Already has an extension or is a path with a separator: leave as-is.
    if path.extension().is_some() || command.contains(['/', '\\']) {
        return command.to_string();
    }

    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let dirs = std::env::var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&dirs) {
        for ext in pathext.split(';') {
            let candidate = dir.join(format!("{command}{ext}"));
            if candidate.is_file() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    command.to_string()
}

#[cfg(not(target_os = "windows"))]
fn resolve_windows_command(command: &str) -> String {
    command.to_string()
}

fn parse_mcp_tools_from_result(result: &serde_json::Value, server: &MCPServer) -> Result<Vec<MCPTool>, MCPError> {
    // MCP 协议的 tools/list 响应形如 {"result":{"tools":[...]}}} —— 工具数组在
    // result.tools 字段下面，result 本身是一个对象而不是数组。之前直接对 result
    // 调用 as_array() 必定返回 None，导致每一个符合协议规范的服务器（包括全部
    // 推荐预设）解析 tools/list 时都会失败，这正是"测试连接"全部失败的根因。
    let array = result
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| MCPError::CommunicationError("tools/list response missing tools array".to_string()))?;

    let mut tools = Vec::new();
    for item in array {
        let name = item["name"]
            .as_str()
            .ok_or_else(|| MCPError::CommunicationError("missing tool name".to_string()))?;
        let description = item["description"].as_str().unwrap_or("No description").to_string();
        // MCP 协议字段名是 camelCase 的 inputSchema，不是 input_schema
        let input_schema = item["inputSchema"].clone();

        tools.push(MCPTool {
            server_id: server.id.clone(),
            server_name: server.name.clone(),
            name: name.to_string(),
            description,
            input_schema,
        });
    }
    log::info!("Parsed {} tools from MCP server '{}'", tools.len(), server.name);
    Ok(tools)
}

async fn call_mcp_tools_stdio(server: &MCPServer) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Calling MCP tools/list via stdio for server: {}", server.id);

    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/list".to_string(),
        params: ToolCallParams {
            name: String::new(),
            arguments: serde_json::json!({}),
        },
        id: Uuid::new_v4().to_string(),
    };

    let request_json = serde_json::to_string(&request).map_err(MCPError::JsonError)?;

    validate_mcp_command(&server.command, &server.args)?;

    let mut cmd = Command::new(resolve_windows_command(&server.command));
    cmd.args(&server.args)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&server.env);
    crate::commands::local_model::hide_console_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MCPError::LaunchError(friendly_missing_runtime_message(&server.command))
        } else {
            MCPError::LaunchError(e.to_string())
        }
    })?;

    // Read stderr in a background task to prevent pipe blocking
    let stderr = child.stderr.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stderr".to_string()))?;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::debug!("[MCP stderr] {}", line);
        }
    });

    {
        let mut stdin = child.stdin.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stdin".to_string()))?;
        stdin
            .write_all((request_json + "\n").as_bytes())
            .await
            .map_err(|e| MCPError::CommunicationError(format!("Failed to write to stdin: {}", e)))?;
    }

    let stdout = child.stdout.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stdout".to_string()))?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();

    let response_line = tokio::time::timeout(MCP_STDIO_TIMEOUT, lines.next_line())
        .await
        .map_err(|_| MCPError::CommunicationError("Tool list timeout".to_string()))?
        .map_err(|e| MCPError::CommunicationError(format!("Failed to read response: {}", e)))?
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(MCPError::JsonError)?;

    // Ensure the child process is terminated
    let _ = child.kill().await;
    let _ = child.wait().await;

    if let Some(error) = response.error {
        return Err(MCPError::CommunicationError(format!("MCP error ({}): {}", error.code, error.message)));
    }

    let result = response
        .result
        .ok_or_else(|| MCPError::CommunicationError("tools/list response missing result".to_string()))?;

    parse_mcp_tools_from_result(&result, server)
}

async fn call_mcp_tools_http(server: &MCPServer) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Calling MCP tools/list via HTTP for server: {}", server.id);

    let url = server.url.as_ref().ok_or_else(|| MCPError::InvalidConfig("HTTP/SSE server requires URL".to_string()))?;

    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/list".to_string(),
        params: ToolCallParams {
            name: String::new(),
            arguments: serde_json::json!({}),
        },
        id: Uuid::new_v4().to_string(),
    };

    let client = reqwest::Client::new();
    let mut req_builder = client.post(url).json(&request);
    if let Some(api_key) = &server.api_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = tokio::time::timeout(
        MCP_HTTP_TIMEOUT,
        req_builder
            .header("Content-Type", "application/json")
            .send(),
    )
    .await
    .map_err(|_| MCPError::CommunicationError("HTTP request timeout".to_string()))?
    .map_err(MCPError::ReqwestError)?;

    if !response.status().is_success() {
        return Err(MCPError::CommunicationError(format!("HTTP error: {}", response.status())));
    }

    let resp_json = response
        .json::<JsonRpcResponse>()
        .await
        .map_err(|e| MCPError::CommunicationError(format!("Failed to parse HTTP response: {}", e)))?;

    if let Some(error) = resp_json.error {
        return Err(MCPError::CommunicationError(format!("MCP error ({}): {}", error.code, error.message)));
    }

    let result = resp_json
        .result
        .ok_or_else(|| MCPError::CommunicationError("tools/list response missing result".to_string()))?;

    parse_mcp_tools_from_result(&result, server)
}

/// Get available tools from a MCP server (by making a tools/list call)
#[tauri::command]
pub async fn get_mcp_tools(
    state: tauri::State<'_, DbState>,
    server_id: String
) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Fetching tools from MCP server: {}", server_id);

    // Load server config from database
    let db = state.0.lock().await;
    let servers = db
        .get_mcp_servers()
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    let server = servers.into_iter().find(|s| s.id == server_id)
        .ok_or_else(|| MCPError::ServerNotFound(server_id.clone()))?;

    let tools = match server.server_type {
        MCPServerType::Stdio => call_mcp_tools_stdio(&server).await?,
        MCPServerType::HTTP | MCPServerType::SSE => call_mcp_tools_http(&server).await?,
    };

    Ok(tools)
}

/// Get all available tools from all enabled MCP servers
#[tauri::command]
pub async fn get_all_mcp_tools(state: tauri::State<'_, DbState>) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Fetching all available MCP tools");

    // Scoped so the DB lock is released before the concurrent fetches below.
    let enabled_servers: Vec<_> = {
        let db = state.0.lock().await;
        let servers = db
            .get_mcp_servers()
            .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
        servers.into_iter().filter(|s| s.enabled).collect()
    };

    // Fetch every server concurrently instead of one-at-a-time -- a serial
    // loop means N servers each pay their own worst-case latency back to
    // back, which compounds fast once there's more than one.
    let fetches = enabled_servers.into_iter().map(|server| async move {
        if let Some((tools, cached_at)) = MCP_TOOLS_CACHE.lock().await.get(&server.id) {
            if cached_at.elapsed() < MCP_TOOLS_CACHE_TTL {
                return (server.id, Ok(tools.clone()));
            }
        }

        let result = match server.server_type {
            MCPServerType::Stdio => call_mcp_tools_stdio(&server).await,
            MCPServerType::HTTP | MCPServerType::SSE => call_mcp_tools_http(&server).await,
        };

        if let Ok(ref tools) = result {
            MCP_TOOLS_CACHE
                .lock()
                .await
                .insert(server.id.clone(), (tools.clone(), Instant::now()));
        }

        (server.id, result)
    });

    let mut all_tools = Vec::new();
    for (server_id, result) in futures::future::join_all(fetches).await {
        match result {
            Ok(tools) => all_tools.extend(tools),
            Err(e) => log::warn!("Failed to get tools from server {}: {}", server_id, e),
        }
    }

    // Built-in tools (web search, page fetch) ship with the app itself --
    // no external runtime/process required -- so they're always available
    // regardless of what the user has configured or installed.
    all_tools.extend(builtin_tool_defs());

    log::info!("Total MCP tools available: {}", all_tools.len());
    Ok(all_tools)
}

/// Call a MCP tool with full JSON-RPC 2.0 support
#[tauri::command]
pub async fn call_mcp_tool(
    state: tauri::State<'_, DbState>,
    server_id: Option<String>,
    tool_name: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    log::info!("MCP tool call requested: server_id={:?}, tool={} input={:?}", server_id, tool_name, input);

    // Handle built-in test/demo tools first
    if tool_name.starts_with("demo_") || tool_name.starts_with("test_") {
        let request_id = Uuid::new_v4().to_string();
        return handle_demo_tool_call(&tool_name, input, &request_id).await;
    }

    // Built-in web search / page fetch have no corresponding DB server row --
    // dispatch directly instead of trying (and failing) to look one up.
    if tool_name.starts_with("builtin__") {
        return execute_builtin_tool(&tool_name, input).await;
    }

    // Load server configurations from database
    let servers = {
        let db = state.0.lock().await;
        db.get_mcp_servers()
            .map_err(|e| MCPError::CommunicationError(e.to_string()))?
    };

    let target_server = if let Some(server_id) = server_id {
        servers
            .into_iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| MCPError::ServerNotFound(server_id.clone()))?
    } else {
        // 1st preference: find enabled server that exposes the requested tool
        let mut found = None;
        for server in servers.into_iter().filter(|s| s.enabled) {
            match get_mcp_tools(state.clone(), server.id.clone()).await {
                Ok(tools) => {
                    if tools.iter().any(|t| t.name == tool_name) {
                        found = Some(server);
                        break;
                    }
                }
                Err(err) => {
                    log::warn!("Failed to list tools for server {}: {}", server.id, err);
                }
            }
        }
        found.ok_or_else(|| MCPError::ServerNotFound(tool_name.clone()))?
    };

    let result = match target_server.server_type {
        MCPServerType::Stdio => call_mcp_tool_stdio(&target_server, &tool_name, input).await,
        MCPServerType::HTTP | MCPServerType::SSE => call_mcp_tool_http(&target_server, &tool_name, input).await,
    };

    match result {
        Ok(v) => Ok(v),
        Err(err) => Err(MCPError::CommunicationError(format!("Failed to execute tool {}: {}", tool_name, err))),
    }
}

/// Handle demo/test tool calls (for development/testing)
async fn handle_demo_tool_call(
    tool_name: &str,
    input: serde_json::Value,
    request_id: &str,
) -> Result<serde_json::Value, MCPError> {
    log::info!("Executing demo tool: {}", tool_name);

    let response = match tool_name {
        "demo_echo" => {
            // Echo back the input
            serde_json::json!({
                "success": true,
                "tool_name": tool_name,
                "echo": input,
                "timestamp": chrono::Local::now().to_rfc3339()
            })
        }
        "demo_calculator" => {
            // Simple calculator demo
            if let Some(obj) = input.as_object() {
                let a = obj.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = obj.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let op = obj
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("add");

                let result = match op {
                    "add" => a + b,
                    "subtract" => a - b,
                    "multiply" => a * b,
                    "divide" if b != 0.0 => a / b,
                    _ => 0.0,
                };

                serde_json::json!({
                    "success": true,
                    "tool_name": tool_name,
                    "operation": op,
                    "operands": {"a": a, "b": b},
                    "result": result,
                    "timestamp": chrono::Local::now().to_rfc3339()
                })
            } else {
                serde_json::json!({
                    "success": false,
                    "error": "Invalid input format for calculator"
                })
            }
        }
        "test_connection" => {
            // Test connectivity
            serde_json::json!({
                "success": true,
                "tool_name": tool_name,
                "status": "MCP service is responsive",
                "request_id": request_id,
                "timestamp": chrono::Local::now().to_rfc3339()
            })
        }
        _ => {
            serde_json::json!({
                "success": false,
                "error": format!("Unknown demo tool: {}", tool_name)
            })
        }
    };

    Ok(response)
}

/// Definitions for the tools the app ships with directly (web search, page
/// fetch) -- these run in-process via `reqwest`, not as an external stdio/
/// HTTP MCP server, so they need no `MCPServer` DB row and work regardless
/// of what's installed on the user's machine. `server_id: "builtin"` is how
/// `execute_tool_calls` (llm.rs) and `call_mcp_tool` recognize them.
fn builtin_tool_defs() -> Vec<MCPTool> {
    vec![
        MCPTool {
            server_id: "builtin".to_string(),
            server_name: "内置工具".to_string(),
            name: "builtin__web_search".to_string(),
            description: "通过 DuckDuckGo 搜索网页，返回标题/链接/摘要列表，用于获取实时信息。内置能力，无需安装任何依赖。".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词" },
                    "max_results": { "type": "integer", "description": "返回结果条数，默认 5，最多 10" }
                },
                "required": ["query"]
            }),
        },
        MCPTool {
            server_id: "builtin".to_string(),
            server_name: "内置工具".to_string(),
            name: "builtin__fetch_url".to_string(),
            description: "抓取指定网页并提取正文文本。内置能力，无需安装任何依赖。".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "要抓取的网页 URL" }
                },
                "required": ["url"]
            }),
        },
    ]
}

const BUILTIN_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const BUILTIN_FETCH_TEXT_LIMIT: usize = 15_000;

/// Execute one of the built-in tools directly -- no subprocess, no external
/// MCP transport, just an in-process HTTP call.
async fn execute_builtin_tool(tool_name: &str, input: serde_json::Value) -> Result<serde_json::Value, MCPError> {
    match tool_name {
        "builtin__web_search" => builtin_web_search(input).await,
        "builtin__fetch_url" => builtin_fetch_url(input).await,
        _ => Err(MCPError::CommunicationError(format!("Unknown builtin tool: {}", tool_name))),
    }
}

async fn builtin_web_search(input: serde_json::Value) -> Result<serde_json::Value, MCPError> {
    let query = input
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MCPError::InvalidConfig("web_search requires a 'query' string".to_string()))?;
    let max_results = input
        .get("max_results")
        .and_then(|v| v.as_u64())
        .map(|n| n.clamp(1, 10) as usize)
        .unwrap_or(5);

    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding::encode(query));

    let client = reqwest::Client::new();
    let response = tokio::time::timeout(
        MCP_HTTP_TIMEOUT,
        client.get(&url).header("User-Agent", BUILTIN_USER_AGENT).send(),
    )
    .await
    .map_err(|_| MCPError::CommunicationError("搜索请求超时".to_string()))?
    .map_err(|e| MCPError::CommunicationError(format!("搜索请求失败: {}", e)))?;

    if !response.status().is_success() {
        return Err(MCPError::CommunicationError(format!("搜索请求失败: HTTP {}", response.status())));
    }

    let html = response
        .text()
        .await
        .map_err(|e| MCPError::CommunicationError(format!("读取搜索结果失败: {}", e)))?;

    let document = scraper::Html::parse_document(&html);
    let result_selector = scraper::Selector::parse(".result__body").unwrap();
    let title_selector = scraper::Selector::parse(".result__title a").unwrap();
    let snippet_selector = scraper::Selector::parse(".result__snippet").unwrap();

    let mut results = Vec::new();
    for result_el in document.select(&result_selector) {
        if results.len() >= max_results {
            break;
        }
        let Some(title_el) = result_el.select(&title_selector).next() else { continue };
        let title: String = title_el.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }
        // DuckDuckGo's HTML results wrap the real target URL in an
        // `uddg=`-encoded redirect link (`//duckduckgo.com/l/?uddg=...`)
        // rather than linking to it directly.
        let raw_href = title_el.value().attr("href").unwrap_or_default();
        let link = raw_href
            .split("uddg=")
            .nth(1)
            .map(|s| s.split('&').next().unwrap_or(s))
            .map(|s| urlencoding::decode(s).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string()))
            .unwrap_or_else(|| raw_href.to_string());
        let snippet: String = result_el
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        results.push(serde_json::json!({ "title": title, "url": link, "snippet": snippet }));
    }

    Ok(serde_json::json!({ "query": query, "results": results }))
}

async fn builtin_fetch_url(input: serde_json::Value) -> Result<serde_json::Value, MCPError> {
    let url = input
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MCPError::InvalidConfig("fetch_url requires a 'url' string".to_string()))?;

    let client = reqwest::Client::new();
    let response = tokio::time::timeout(
        MCP_HTTP_TIMEOUT,
        client.get(url).header("User-Agent", BUILTIN_USER_AGENT).send(),
    )
    .await
    .map_err(|_| MCPError::CommunicationError("网页抓取超时".to_string()))?
    .map_err(|e| MCPError::CommunicationError(format!("网页抓取失败: {}", e)))?;

    if !response.status().is_success() {
        return Err(MCPError::CommunicationError(format!("网页抓取失败: HTTP {}", response.status())));
    }

    let html = response
        .text()
        .await
        .map_err(|e| MCPError::CommunicationError(format!("读取网页内容失败: {}", e)))?;

    let document = scraper::Html::parse_document(&html);
    // Pull text from block-level content tags rather than the whole document --
    // real content almost always lives in these, while nav/script/style/footer
    // chrome doesn't, so this sidesteps needing to explicitly strip them out.
    let content_selector = scraper::Selector::parse(
        "h1, h2, h3, h4, h5, h6, p, li, td, th, blockquote, pre, article, dd, dt"
    ).unwrap();

    let mut text = String::new();
    for el in document.select(&content_selector) {
        let line: String = el.text().collect::<String>().trim().to_string();
        if !line.is_empty() {
            text.push_str(&line);
            text.push('\n');
        }
    }

    if text.trim().is_empty() {
        // Fallback for pages that don't use semantic block tags: grab the body wholesale.
        if let Some(body) = document.select(&scraper::Selector::parse("body").unwrap()).next() {
            text = body.text().collect::<Vec<_>>().join(" ");
        }
    }

    let mut text = text.trim().to_string();
    let truncated = text.chars().count() > BUILTIN_FETCH_TEXT_LIMIT;
    if truncated {
        text = text.chars().take(BUILTIN_FETCH_TEXT_LIMIT).collect();
    }

    Ok(serde_json::json!({ "url": url, "content": text, "truncated": truncated }))
}

/// Call MCP tool via Stdio (JSON-RPC over stdin/stdout)
#[allow(dead_code)]
async fn call_mcp_tool_stdio(
    server: &MCPServer,
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    log::info!("Calling MCP tool via stdio: {}", tool_name);

    // Build JSON-RPC request
    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: ToolCallParams {
            name: tool_name.to_string(),
            arguments: input,
        },
        id: Uuid::new_v4().to_string(),
    };

    let request_json = serde_json::to_string(&request)
        .map_err(|e| MCPError::JsonError(e))?;

    log::debug!("MCP Request: {}", request_json);

    validate_mcp_command(&server.command, &server.args)?;

    // Execute the server process
    let mut cmd = Command::new(resolve_windows_command(&server.command));
    cmd.args(&server.args)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&server.env);
    crate::commands::local_model::hide_console_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MCPError::LaunchError(friendly_missing_runtime_message(&server.command))
        } else {
            MCPError::LaunchError(e.to_string())
        }
    })?;

    // Read stderr in a background task to prevent pipe blocking
    let stderr = child.stderr.take().ok_or_else(|| {
        MCPError::CommunicationError("Failed to open stderr".to_string())
    })?;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::debug!("[MCP stderr] {}", line);
        }
    });

    // Send request to stdin
    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            MCPError::CommunicationError("Failed to open stdin".to_string())
        })?;

        stdin
            .write_all((request_json + "\n").as_bytes())
            .await
            .map_err(|e| MCPError::CommunicationError(format!("Failed to write to stdin: {}", e)))?;
    }

    // Read response from stdout with timeout
    let stdout = child.stdout.take().ok_or_else(|| {
        MCPError::CommunicationError("Failed to open stdout".to_string())
    })?;

    let mut lines = tokio::io::BufReader::new(stdout).lines();

    // Read first line (should be the JSON response)
    let response_line = tokio::time::timeout(MCP_TOOL_CALL_TIMEOUT, lines.next_line())
        .await
        .map_err(|_| MCPError::CommunicationError("Tool execution timeout".to_string()))?
        .map_err(|e| MCPError::CommunicationError(format!("Failed to read response: {}", e)))?
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    log::debug!("MCP Response: {}", response_line);

    // Parse JSON-RPC response
    let response: JsonRpcResponse = serde_json::from_str(&response_line)
        .map_err(|e| MCPError::JsonError(e))?;

    // Ensure the child process is terminated
    let _ = child.kill().await;
    let _ = child.wait().await;

    if let Some(error) = response.error {
        return Err(MCPError::CommunicationError(format!(
            "MCP error ({}): {}",
            error.code, error.message
        )));
    }

    Ok(response
        .result
        .unwrap_or(serde_json::json!({"status": "success"})))
}

/// Call MCP tool via HTTP/SSE
#[allow(dead_code)]
async fn call_mcp_tool_http(
    server: &MCPServer,
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    log::info!("Calling MCP tool via HTTP: {}", tool_name);

    let url = server.url.as_ref().ok_or_else(|| {
        MCPError::InvalidConfig("HTTP server requires URL".to_string())
    })?;

    // Build JSON-RPC request
    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: ToolCallParams {
            name: tool_name.to_string(),
            arguments: input,
        },
        id: Uuid::new_v4().to_string(),
    };

    // Create HTTP client
    let client = reqwest::Client::new();
    let mut req_builder = client.post(url);

    // Add auth header if API key provided
    if let Some(api_key) = &server.api_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    // Send request with timeout
    let response = tokio::time::timeout(
        MCP_HTTP_TIMEOUT,
        req_builder
            .header("Content-Type", "application/json")
            .json(&request)
            .send(),
    )
    .await
    .map_err(|_| MCPError::CommunicationError("HTTP request timeout".to_string()))?
    .map_err(|e| MCPError::CommunicationError(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(MCPError::CommunicationError(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    // Parse JSON-RPC response
    let resp_json = response
        .json::<JsonRpcResponse>()
        .await
        .map_err(|e| MCPError::CommunicationError(format!("Failed to parse HTTP response: {}", e)))?;

    if let Some(error) = resp_json.error {
        return Err(MCPError::CommunicationError(format!(
            "MCP error ({}): {}",
            error.code, error.message
        )));
    }

    Ok(resp_json
        .result
        .unwrap_or(serde_json::json!({"status": "success"})))
}

/// Result of a connection test -- carries the real failure reason (e.g. "需要
/// 先安装 uv...") alongside the pass/fail flag, instead of collapsing it down
/// to a bare boolean and leaving the user to go dig through the log file.
#[derive(Debug, Clone, Serialize)]
pub struct MCPConnectionTestResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Test MCP server connection
#[tauri::command]
pub async fn test_mcp_connection(
    server_type: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
) -> Result<MCPConnectionTestResult, MCPError> {
    match server_type.as_str() {
        "stdio" => {
            if let Some(cmd) = command {
                let executable = cmd.trim();
                if executable.is_empty() {
                    return Err(MCPError::InvalidConfig("stdio requires a non-empty command".to_string()));
                }
                let args = args.unwrap_or_default();

                // Validate against command whitelist and dangerous patterns
                validate_mcp_command(executable, &args)?;

                // A stdio MCP server is a long-running process that never exits on
                // its own, so waiting for it to terminate (as `.output()` does)
                // would hang forever. Instead, probe it with a real tools/list
                // JSON-RPC request, the same way `call_mcp_tools_stdio` does.
                //
                // `command`/`args` are kept as two separate fields here -- matching
                // exactly how `call_mcp_tools_stdio` spawns the real saved server
                // (`Command::new(&server.command).args(&server.args)`, no shell
                // parsing) -- so a passing test here means the real launch will
                // behave the same way.
                let probe_server = MCPServer {
                    id: String::new(),
                    name: String::new(),
                    description: String::new(),
                    server_type: MCPServerType::Stdio,
                    command: executable.to_string(),
                    args,
                    env: HashMap::new(),
                    port: None,
                    url: None,
                    api_key: None,
                    enabled: true,
                    created_at: 0,
                    updated_at: 0,
                };

                match call_mcp_tools_stdio(&probe_server).await {
                    Ok(tools) => {
                        log::info!("MCP test connection succeeded for '{} {}': {} tools", executable, probe_server.args.join(" "), tools.len());
                        Ok(MCPConnectionTestResult { success: true, error: None })
                    }
                    Err(e) => {
                        log::warn!("MCP test connection failed for '{} {}': {}", executable, probe_server.args.join(" "), e);
                        Ok(MCPConnectionTestResult { success: false, error: Some(e.to_string()) })
                    }
                }
            } else {
                Err(MCPError::InvalidConfig("stdio requires command".to_string()))
            }
        }
        "sse" | "http" => {
            if let Some(url) = url {
                // Try HTTP request to server
                match reqwest::Client::new().get(&url).send().await {
                    Ok(resp) => {
                        log::info!("MCP test connection to '{}' returned status {}", url, resp.status());
                        let status = resp.status();
                        if status.is_success() {
                            Ok(MCPConnectionTestResult { success: true, error: None })
                        } else {
                            Ok(MCPConnectionTestResult { success: false, error: Some(format!("HTTP {}", status)) })
                        }
                    }
                    Err(e) => {
                        log::warn!("MCP test connection failed for '{}': {}", url, e);
                        Ok(MCPConnectionTestResult { success: false, error: Some(e.to_string()) })
                    }
                }
            } else {
                Err(MCPError::InvalidConfig("HTTP/SSE requires URL".to_string()))
            }
        }
        _ => Err(MCPError::InvalidConfig("Invalid server type".to_string())),
    }
}

impl std::fmt::Display for MCPServerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MCPServerType::Stdio => write!(f, "stdio"),
            MCPServerType::SSE => write!(f, "sse"),
            MCPServerType::HTTP => write!(f, "http"),
        }
    }
}
