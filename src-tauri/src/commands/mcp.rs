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

/// `stream_message` 在每一次聊天轮次都会调用 `get_all_mcp_tools`（即使 MCP 总开关
/// 关闭也要调，因为手动激活的 Skill 仍然可以绑定某个服务器）——没有缓存的话，
/// 意味着每次发送 LLM 请求前都要对全部已启用服务器重新跑一遍 `tools/list`。
/// 对 stdio 类型服务器来说，这是每条消息都要新起一个子进程（比如 `npx ...`）再
/// 销毁，如果服务器启动慢或者短暂不可达，耗时可以从几百毫秒一路涨到
/// `MCP_STDIO_TIMEOUT`/`MCP_HTTP_TIMEOUT` 设的上限——直接拖慢 TTFT（首字节时间）。
/// 因此对成功的查询结果做短 TTL 缓存；失败的结果绝不缓存，这样出问题的服务器
/// 会持续暴露出来，而不是被悄悄掩盖掉。
static MCP_TOOLS_CACHE: Lazy<Mutex<HashMap<String, (Vec<MCPTool>, Instant)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
const MCP_TOOLS_CACHE_TTL: Duration = Duration::from_secs(300);

/// MCP 错误类型
#[derive(Error, Debug)]
pub enum MCPError {
    /// 服务器未找到
    #[error("找不到 MCP 服务器 \"{0}\"，请检查是否已在设置中配置")]
    ServerNotFound(String),
    /// 启动服务器失败
    #[error("启动 MCP 服务器失败：{0}")]
    LaunchError(String),
    /// 通信错误
    #[error("与 MCP 服务器通信失败：{0}")]
    CommunicationError(String),
    /// 配置无效
    #[error("MCP 服务器配置有误：{0}")]
    InvalidConfig(String),
    /// JSON 解析错误
    #[error("解析 MCP 服务器返回的数据失败，请确认服务器版本是否兼容")]
    JsonError(#[from] serde_json::Error),
    /// HTTP 请求错误
    #[error("连接 MCP 服务器失败，请检查网络连接和服务器地址")]
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

/// MCP 工具调用结果
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolResult {
    pub tool_name: String,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

/// MCP tools/call 方法对应的 JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolCallRequest {
    jsonrpc: String,
    method: String,
    params: ToolCallParams,
    id: String,
}

/// tools/call 方法的参数
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallParams {
    name: String,
    arguments: serde_json::Value,
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: Option<String>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcErrorData>,
    id: Option<String>,
}

/// JSON-RPC 2.0 错误结构
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcErrorData {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

// Tauri 命令

/// 创建或更新 MCP 服务器配置
#[tauri::command]
pub async fn create_mcp_server(
    state: tauri::State<'_, DbState>,
    server: MCPServer,
) -> Result<MCPServer, MCPError> {
    // 校验配置
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
                    "HTTP/SSE 类型的服务器必须填写 URL".to_string(),
                ));
            }
        }
    }

    // 未提供 ID 时自动生成
    let mut config = server;
    if config.id.is_empty() {
        config.id = Uuid::new_v4().to_string();
    }
    config.created_at = chrono::Utc::now().timestamp_millis();
    config.updated_at = chrono::Utc::now().timestamp_millis();

    // 保存到数据库
    let db = state.0.lock().await;
    db.save_mcp_server(&config)
        .map_err(|e| { log::error!("保存 MCP 服务器配置失败（详情：{}）", e); MCPError::CommunicationError("保存 MCP 服务器配置失败，请重试".to_string()) })?;
    drop(db);

    // 服务器的 command/args/url 刚刚可能发生了变化 -- 清掉对应的工具列表缓存，
    // 让下一次查询重新发现，而不是继续返回过期数据。
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

/// 获取 MCP 服务器列表
#[tauri::command]
pub async fn list_mcp_servers(state: tauri::State<'_, DbState>) -> Result<Vec<MCPServer>, MCPError> {
    let db = state.0.lock().await;
    let servers = db
        .get_mcp_servers()
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    log::info!("Retrieved {} MCP servers", servers.len());
    Ok(servers)
}

/// 删除 MCP 服务器配置
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

/// 把裸运行时（如 npx/python）"program not found" 这种启动失败，翻译成真正
/// 能告诉用户该装什么的提示，而不是原始的操作系统 `NotFound` 文本 —— stdio 类型
/// 的 MCP 服务器本质上就是"用这些参数运行这个运行时"，所以运行时缺失是迄今为止
/// 最常见的启动失败原因，对非开发者用户来说也是最难自己看懂的一种。
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

/// 在 Windows 上把裸命令名（比如 "npx"）解析成一个可直接 spawn 的路径。
///
/// `std::process::Command::new` 是直接调用 `CreateProcessW`，*不会*像 shell
/// 那样做基于 PATHEXT 的扩展名搜索 —— 所以那些以 `.cmd`/`.bat` shim 形式安装的
/// 命令（npm 在 Windows 上安装 `npx`/`npm` 本身就是这种方式，不同于 `node`、
/// `cargo` 这类单文件 `.exe` 工具）即使明明在 PATH 里，也会因为 spawn 不到而
/// 报"program not found"。已直接验证过：`Command::new("npx")` 在 Windows 上
/// 会报 `NotFound`，而 `Command::new("node")` 则能正常工作。这里改为在 PATH 中
/// 搜索第一个匹配扩展名的文件，直接 spawn 那个具体文件。这个解析只用在真正
/// spawn 的那一刻 -- `validate_mcp_command` 仍然是拿原始的裸命令名去比对白名单。
#[cfg(target_os = "windows")]
fn resolve_windows_command(command: &str) -> String {
    let path = Path::new(command);
    // 已经带扩展名，或者本身就是带分隔符的路径：原样返回。
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

    // 在后台任务里读 stderr，防止管道被写满而阻塞
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
            .map_err(|e| { log::error!("MCP 写入 stdin 失败（详情：{}）", e); MCPError::CommunicationError("向 MCP 服务器发送请求失败".to_string()) })?;
    }

    let stdout = child.stdout.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stdout".to_string()))?;
    let mut lines = tokio::io::BufReader::new(stdout).lines();

    let response_line = tokio::time::timeout(MCP_STDIO_TIMEOUT, lines.next_line())
        .await
        .map_err(|_| MCPError::CommunicationError("Tool list timeout".to_string()))?
        .map_err(|e| { log::error!("MCP 读取响应失败（详情：{}）", e); MCPError::CommunicationError("读取 MCP 服务器响应失败，请确认服务器是否正常运行".to_string()) })?
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(MCPError::JsonError)?;

    // 确保子进程被终止
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
        .map_err(|e| { log::error!("MCP 解析 HTTP 响应失败（详情：{}）", e); MCPError::CommunicationError("解析 MCP 服务器返回的数据失败，请确认服务器版本是否兼容".to_string()) })?;

    if let Some(error) = resp_json.error {
        return Err(MCPError::CommunicationError(format!("MCP error ({}): {}", error.code, error.message)));
    }

    let result = resp_json
        .result
        .ok_or_else(|| MCPError::CommunicationError("tools/list response missing result".to_string()))?;

    parse_mcp_tools_from_result(&result, server)
}

/// 获取某个 MCP 服务器可用的工具（发起一次 tools/list 调用）
#[tauri::command]
pub async fn get_mcp_tools(
    state: tauri::State<'_, DbState>,
    server_id: String
) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Fetching tools from MCP server: {}", server_id);

    // 从数据库加载服务器配置
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

/// 获取所有已启用 MCP 服务器的全部可用工具
#[tauri::command]
pub async fn get_all_mcp_tools(state: tauri::State<'_, DbState>) -> Result<Vec<MCPTool>, MCPError> {
    log::info!("Fetching all available MCP tools");

    // 用作用域限定，确保下面并发拉取之前先释放数据库锁。
    let enabled_servers: Vec<_> = {
        let db = state.0.lock().await;
        let servers = db
            .get_mcp_servers()
            .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
        servers.into_iter().filter(|s| s.enabled).collect()
    };

    // 并发拉取所有服务器，而不是一个个串行处理 -- 串行循环意味着 N 个服务器
    // 各自的最坏延迟会依次叠加，服务器一多耗时就会迅速累积。
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

    // 内置工具（网页搜索、抓取网页）是随应用本体一起打包的 -- 不需要任何外部
    // 运行时/进程 -- 所以无论用户配置或安装了什么，这些工具永远可用。
    all_tools.extend(builtin_tool_defs());

    log::info!("Total MCP tools available: {}", all_tools.len());
    Ok(all_tools)
}

/// 调用一个 MCP 工具，完整支持 JSON-RPC 2.0
#[tauri::command]
pub async fn call_mcp_tool(
    state: tauri::State<'_, DbState>,
    server_id: Option<String>,
    tool_name: String,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    log::info!("MCP tool call requested: server_id={:?}, tool={} input={:?}", server_id, tool_name, input);

    // 优先处理内置的测试/演示工具
    if tool_name.starts_with("demo_") || tool_name.starts_with("test_") {
        let request_id = Uuid::new_v4().to_string();
        return handle_demo_tool_call(&tool_name, input, &request_id).await;
    }

    // 内置的网页搜索/抓取网页在数据库里没有对应的服务器行 --
    // 直接分发处理，而不是尝试（并且失败）去查一个不存在的行。
    if tool_name.starts_with("builtin__") {
        return execute_builtin_tool(&tool_name, input).await;
    }

    // 从数据库加载服务器配置
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
        // 优先方案：找出提供了所请求工具的那个已启用服务器
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
        Err(err) => Err(MCPError::CommunicationError(format!("工具 \"{}\" 调用失败：{}", tool_name, err))),
    }
}

/// 处理演示/测试用工具调用（供开发/测试使用）
pub(crate) async fn handle_demo_tool_call(
    tool_name: &str,
    input: serde_json::Value,
    request_id: &str,
) -> Result<serde_json::Value, MCPError> {
    log::info!("Executing demo tool: {}", tool_name);

    let response = match tool_name {
        "demo_echo" => {
            // 原样回显输入内容
            serde_json::json!({
                "success": true,
                "tool_name": tool_name,
                "echo": input,
                "timestamp": chrono::Local::now().to_rfc3339()
            })
        }
        "demo_calculator" => {
            // 简单的计算器演示
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
            // 测试连通性
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

/// 应用本体直接内置的工具定义（网页搜索、抓取网页）-- 这些工具通过 `reqwest`
/// 在进程内直接运行，不是外部的 stdio/HTTP MCP 服务器，所以不需要 `MCPServer`
/// 数据库行，也不受用户机器上安装了什么的影响。`server_id: "builtin"` 就是
/// `execute_tool_calls`（llm.rs）和 `call_mcp_tool` 用来识别它们的标记。
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

/// 直接执行某个内置工具 -- 不起子进程，不走外部 MCP 传输协议，
/// 就是进程内一次普通的 HTTP 调用。
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
        // DuckDuckGo 的 HTML 搜索结果不是直接链接到目标网址，而是把真实目标 URL
        // 包在一个 `uddg=` 编码的跳转链接里（`//duckduckgo.com/l/?uddg=...`）。
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
    // 只从块级内容标签里取文本，而不是整个文档 -- 真正的正文内容几乎总是在
    // 这些标签里，而导航栏/脚本/样式/页脚这些界面元素不会用它们，这样就不用
    // 再显式地把它们过滤掉。
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
        // 兜底方案：如果页面没有用语义化的块级标签，就整个 body 一起取。
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

/// 通过 Stdio 调用 MCP 工具（JSON-RPC 通过 stdin/stdout 传输）
#[allow(dead_code)]
async fn call_mcp_tool_stdio(
    server: &MCPServer,
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    log::info!("Calling MCP tool via stdio: {}", tool_name);

    // 构建 JSON-RPC 请求
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

    // 启动服务器进程
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

    // 在后台任务里读 stderr，防止管道被写满而阻塞
    let stderr = child.stderr.take().ok_or_else(|| {
        MCPError::CommunicationError("Failed to open stderr".to_string())
    })?;
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::debug!("[MCP stderr] {}", line);
        }
    });

    // 把请求写入 stdin
    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            MCPError::CommunicationError("Failed to open stdin".to_string())
        })?;

        stdin
            .write_all((request_json + "\n").as_bytes())
            .await
            .map_err(|e| { log::error!("MCP 写入 stdin 失败（详情：{}）", e); MCPError::CommunicationError("向 MCP 服务器发送请求失败".to_string()) })?;
    }

    // 带超时地从 stdout 读取响应
    let stdout = child.stdout.take().ok_or_else(|| {
        MCPError::CommunicationError("Failed to open stdout".to_string())
    })?;

    let mut lines = tokio::io::BufReader::new(stdout).lines();

    // 读取第一行（应该就是 JSON 响应）
    let response_line = tokio::time::timeout(MCP_TOOL_CALL_TIMEOUT, lines.next_line())
        .await
        .map_err(|_| MCPError::CommunicationError("Tool execution timeout".to_string()))?
        .map_err(|e| { log::error!("MCP 读取响应失败（详情：{}）", e); MCPError::CommunicationError("读取 MCP 服务器响应失败，请确认服务器是否正常运行".to_string()) })?
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    log::debug!("MCP Response: {}", response_line);

    // 解析 JSON-RPC 响应
    let response: JsonRpcResponse = serde_json::from_str(&response_line)
        .map_err(|e| MCPError::JsonError(e))?;

    // 确保子进程被终止
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

/// 通过 HTTP/SSE 调用 MCP 工具
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

    // 构建 JSON-RPC 请求
    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: ToolCallParams {
            name: tool_name.to_string(),
            arguments: input,
        },
        id: Uuid::new_v4().to_string(),
    };

    // 创建 HTTP 客户端
    let client = reqwest::Client::new();
    let mut req_builder = client.post(url);

    // 如果提供了 API 密钥，加上认证头
    if let Some(api_key) = &server.api_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    // 带超时地发送请求
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

    // 解析 JSON-RPC 响应
    let resp_json = response
        .json::<JsonRpcResponse>()
        .await
        .map_err(|e| { log::error!("MCP 解析 HTTP 响应失败（详情：{}）", e); MCPError::CommunicationError("解析 MCP 服务器返回的数据失败，请确认服务器版本是否兼容".to_string()) })?;

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

/// 连接测试的结果 -- 除了成功/失败标志外，还带上真正的失败原因（比如"需要
/// 先安装 uv..."），而不是简化成一个裸的布尔值，让用户自己去翻日志文件找原因。
#[derive(Debug, Clone, Serialize)]
pub struct MCPConnectionTestResult {
    pub success: bool,
    pub error: Option<String>,
}

/// 测试 MCP 服务器连接
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

                // 校验是否在命令白名单内，以及是否包含危险模式
                validate_mcp_command(executable, &args)?;

                // stdio 类型的 MCP 服务器是一个长期运行、不会自己退出的进程，
                // 所以像 `.output()` 那样等它终止会永远卡住。这里改为像
                // `call_mcp_tools_stdio` 一样，发一次真实的 tools/list
                // JSON-RPC 请求来探测。
                //
                // `command`/`args` 这里特意保留为两个独立字段 -- 与
                // `call_mcp_tools_stdio` 启动真实已保存服务器的方式完全一致
                // （`Command::new(&server.command).args(&server.args)`，不经过
                // shell 解析）-- 这样这里测试通过，就意味着真正启动时行为一致。
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
                // 尝试向服务器发起 HTTP 请求
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
