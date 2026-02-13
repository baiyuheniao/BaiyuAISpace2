// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::io::Write;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCall {
    pub server_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// MCP Tool Call Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolResult {
    pub tool_name: String,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

// Tauri Commands

/// Create or update MCP server configuration
#[tauri::command]
pub async fn create_mcp_server(
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
    config.updated_at = chrono::Local::now().timestamp_millis();

    // TODO: Test connection to MCP server
    // TODO: Fetch available tools from server
    
    log::info!(
        "MCP server configured: {} ({})",
        config.name,
        config.server_type.to_string()
    );

    Ok(config)
}

/// Get list of MCP servers
#[tauri::command]
pub async fn list_mcp_servers() -> Result<Vec<MCPServer>, MCPError> {
    // TODO: Load from database/storage
    Ok(vec![])
}

/// Delete MCP server configuration
#[tauri::command]
pub async fn delete_mcp_server(server_id: String) -> Result<(), MCPError> {
    // TODO: Delete from database/storage
    log::info!("MCP server deleted: {}", server_id);
    Ok(())
}

/// Get available tools from a MCP server
#[tauri::command]
pub async fn get_mcp_tools(server_id: String) -> Result<Vec<MCPTool>, MCPError> {
    // TODO: Query MCP server for available tools
    // For now, return empty list
    Ok(vec![])
}

/// Get all available tools from all enabled MCP servers
#[tauri::command]
pub async fn get_all_mcp_tools() -> Result<Vec<MCPTool>, MCPError> {
    // TODO: Aggregate tools from all enabled servers
    Ok(vec![])
}

/// Call a MCP tool
#[tauri::command]
pub async fn call_mcp_tool(
    server_id: String,
    tool_name: String,
    input: serde_json::Value,
) -> Result<MCPToolResult, MCPError> {
    // TODO: Implement actual MCP tool invocation
    // This will communicate with the MCP server based on its type (stdio/SSE/HTTP)
    
    let result = MCPToolResult {
        tool_name: tool_name.clone(),
        result: serde_json::json!({
            "status": "success",
            "message": format!("Tool {} executed", tool_name)
        }),
        error: None,
    };

    Ok(result)
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
