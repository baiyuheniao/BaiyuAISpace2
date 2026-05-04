// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::db::DbState;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum MCPError {
    #[error("MCP server not found: {0}")]
    ServerNotFound(String),
    #[error("Failed to launch MCP server: {0}")]
    LaunchError(String),
    #[error("MCP communication error: {0}")]
    CommunicationError(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
}

impl Serialize for MCPError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// MCP Server Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServer {
    pub id: String,
    pub name: String,
    pub description: String,
    pub server_type: MCPServerType, // "stdio", "sse", "custom"
    pub command: String, // For stdio: path to executable or script
    pub args: Vec<String>, // Command arguments
    pub env: HashMap<String, String>, // Environment variables
    pub port: Option<u16>, // For SSE/HTTP servers
    pub url: Option<String>, // For HTTP servers
    pub api_key: Option<String>, // For API authentication
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MCPServerType {
    #[serde(rename = "stdio")]
    Stdio,
    #[serde(rename = "sse")]
    SSE,
    #[serde(rename = "http")]
    HTTP,
}

/// Tool exposed by MCP Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPTool {
    pub server_id: String,
    pub server_name: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value, // JSON Schema
}

/// MCP Tool Call Request
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
    log::info!("MCP server deleted: {}", server_id);
    Ok(())
}

const ALLOWED_MCP_COMMANDS: &[&str] = &[
    "npx", "npm", "node", "python", "python3", "pip", "uvx", "uv",
    "bun", "deno", "go", "cargo", "ruby", "perl", "php",
];

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

fn parse_mcp_tools_from_result(result: &serde_json::Value, server: &MCPServer) -> Result<Vec<MCPTool>, MCPError> {
    let array = result
        .as_array()
        .ok_or_else(|| MCPError::CommunicationError("tools/list response is not array".to_string()))?;

    let mut tools = Vec::new();
    for item in array {
        let name = item["name"]
            .as_str()
            .ok_or_else(|| MCPError::CommunicationError("missing tool name".to_string()))?;
        let description = item["description"].as_str().unwrap_or("No description").to_string();
        let input_schema = item["input_schema"].clone();

        tools.push(MCPTool {
            server_id: server.id.clone(),
            server_name: server.name.clone(),
            name: name.to_string(),
            description,
            input_schema,
        });
    }
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

    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&server.env)
        .spawn()
        .map_err(|e| MCPError::LaunchError(format!("Failed to launch MCP server: {}", e)))?;

    {
        let mut stdin = child.stdin.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stdin".to_string()))?;
        stdin
            .write_all((request_json + "\n").as_bytes())
            .map_err(|e| MCPError::CommunicationError(format!("Failed to write to stdin: {}", e)))?;
    }

    let stdout = child.stdout.take().ok_or_else(|| MCPError::CommunicationError("Failed to open stdout".to_string()))?;
    let mut reader = BufReader::new(stdout).lines();

    let response_line = tokio::time::timeout(std::time::Duration::from_secs(30), async { reader.next().transpose() })
        .await
        .map_err(|_| MCPError::CommunicationError("Tool list timeout".to_string()))?
        .map_err(|e| MCPError::CommunicationError(format!("Failed to read response: {}", e)))?
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(MCPError::JsonError)?;

    // Ensure the child process is terminated
    let _ = child.kill();
    let _ = child.wait();

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
        std::time::Duration::from_secs(60),
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

    let db = state.0.lock().await;
    let servers = db
        .get_mcp_servers()
        .map_err(|e| MCPError::CommunicationError(e.to_string()))?;
    let enabled_servers: Vec<_> = servers.iter().filter(|s| s.enabled).cloned().collect();

    let mut all_tools = Vec::new();
    for server in enabled_servers {
        match get_mcp_tools(state.clone(), server.id.clone()).await {
            Ok(tools) => all_tools.extend(tools),
            Err(e) => log::warn!("Failed to get tools from server {}: {}", server.id, e),
        }
    }

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
    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(&server.env)
        .spawn()
        .map_err(|e| MCPError::LaunchError(format!("Failed to launch MCP server: {}", e)))?;

    // Send request to stdin
    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            MCPError::CommunicationError("Failed to open stdin".to_string())
        })?;

        stdin
            .write_all((request_json + "\n").as_bytes())
            .map_err(|e| MCPError::CommunicationError(format!("Failed to write to stdin: {}", e)))?;
    }

    // Read response from stdout with timeout
    let stdout = child.stdout.take().ok_or_else(|| {
        MCPError::CommunicationError("Failed to open stdout".to_string())
    })?;

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Read first line (should be the JSON response)
    let response_inner = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        async { lines.next().transpose() },
    )
    .await
    .map_err(|_| MCPError::CommunicationError("Tool execution timeout".to_string()))?;

    let response_option = response_inner
        .map_err(|e| MCPError::CommunicationError(format!("Failed to read response: {}", e)))?;

    let response_line = response_option
        .ok_or_else(|| MCPError::CommunicationError("No response from MCP server".to_string()))?;

    log::debug!("MCP Response: {}", response_line);

    // Parse JSON-RPC response
    let response: JsonRpcResponse = serde_json::from_str(&response_line)
        .map_err(|e| MCPError::JsonError(e))?;

    // Ensure the child process is terminated
    let _ = child.kill();
    let _ = child.wait();

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
        std::time::Duration::from_secs(60),
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

/// Test MCP server connection
#[tauri::command]
pub async fn test_mcp_connection(
    server_type: String,
    command: Option<String>,
    url: Option<String>,
) -> Result<bool, MCPError> {
    match server_type.as_str() {
        "stdio" => {
            if let Some(cmd) = command {
                // Try to execute command and verify it responds
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(|e| MCPError::LaunchError(e.to_string()))?;

                Ok(output.status.success())
            } else {
                Err(MCPError::InvalidConfig("stdio requires command".to_string()))
            }
        }
        "sse" | "http" => {
            if let Some(url) = url {
                // Try HTTP request to server
                match reqwest::Client::new().get(&url).send().await {
                    Ok(resp) => Ok(resp.status().is_success()),
                    Err(e) => Err(MCPError::CommunicationError(e.to_string())),
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
