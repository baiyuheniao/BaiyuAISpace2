// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::commands::llm::{ChatMessage, ChatSession};
use crate::commands::mcp::{MCPServer, MCPServerType};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

// Database instance wrapper
pub struct Database {
    pub path: String,
}

impl Database {
    pub fn new(app_handle: &tauri::AppHandle) -> Self {
        let app_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir");
        std::fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        let db_path = app_dir.join("app.db");
        Self {
            path: db_path.to_str().unwrap().to_string(),
        }
    }

    pub async fn init(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        // Create sessions table
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                api_config_id TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        // Add api_config_id column if it doesn't exist (for database migration)
        let has_column: Result<i32, _> = conn.query_row(
            "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'api_config_id'",
            [],
            |_| Ok(1),
        );
        if has_column.is_err() {
            conn.execute(
                "ALTER TABLE sessions ADD COLUMN api_config_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            log::info!("Database migration: added api_config_id column");
        }

        // Create messages table
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                error TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
            [],
        )?;

        // Create MCP servers table
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                server_type TEXT NOT NULL,
                command TEXT,
                args TEXT, -- JSON array of strings
                env TEXT, -- JSON object of key-value pairs
                port INTEGER,
                url TEXT,
                api_key TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled)",
            [],
        )?;

        log::info!("Database initialized at: {}", self.path);
        Ok(())
    }

    pub fn save_session(&self, session: &ChatSession) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            r#"
            INSERT OR REPLACE INTO sessions (id, title, provider, model, api_config_id, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            [
                &session.id,
                &session.title,
                &session.provider,
                &session.model,
                &session.api_config_id,
                &session.created_at.to_string(),
                &session.updated_at.to_string(),
            ],
        )?;

        log::info!("Session saved: {}", session.id);
        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            [session_id],
        )?;

        log::info!("Session deleted: {}", session_id);
        Ok(())
    }

    pub fn get_sessions(&self) -> Result<Vec<ChatSession>, Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
            r#"
            SELECT id, title, provider, model, api_config_id, created_at, updated_at 
            FROM sessions 
            ORDER BY updated_at DESC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, title, provider, model, api_config_id, created_at, updated_at) = row?;
            let messages = self.get_messages(&id)?;
            
            sessions.push(ChatSession {
                id,
                title,
                provider,
                model,
                api_config_id,
                created_at,
                updated_at,
                messages,
            });
        }

        log::info!("Loaded {} sessions", sessions.len());
        Ok(sessions)
    }

    pub fn save_message(
        &self,
        session_id: &str,
        message: &ChatMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            r#"
            INSERT OR REPLACE INTO messages (id, session_id, role, content, timestamp, error)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            [
                &message.id,
                session_id,
                &message.role,
                &message.content,
                &message.timestamp.to_string(),
                &message.error.as_deref().unwrap_or(""),
            ],
        )?;

        // Update session's updated_at
        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            [&chrono::Utc::now().timestamp_millis().to_string(), session_id],
        )?;

        Ok(())
    }

    pub fn get_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, timestamp, error 
            FROM messages 
            WHERE session_id = ?1 
            ORDER BY timestamp ASC
            "#,
        )?;

        let rows = stmt.query_map([session_id], |row| {
            let error: Option<String> = row.get(4)?;
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                timestamp: row.get(3)?,
                error: if error.as_deref() == Some("") { None } else { error },
            })
        })?;

        let messages: Result<Vec<_>, _> = rows.collect();
        Ok(messages?)
    }

    pub fn save_mcp_server(&self, server: &MCPServer) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let args_json = serde_json::to_string(&server.args)?;
        let env_json = serde_json::to_string(&server.env)?;
        let server_type = match server.server_type {
            MCPServerType::Stdio => "stdio",
            MCPServerType::SSE => "sse",
            MCPServerType::HTTP => "http",
        }
        .to_string();
        
        let now = chrono::Utc::now().timestamp_millis();
        let port = server.port.map(|p| p as i64);

        conn.execute(
            r#"
            INSERT OR REPLACE INTO mcp_servers 
            (id, name, description, server_type, command, args, env, port, url, api_key, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            rusqlite::params![
                &server.id,
                &server.name,
                &server.description,
                &server_type,
                &server.command,
                &args_json,
                &env_json,
                &port,
                &server.url,
                &server.api_key,
                &server.enabled,
                &now,
                &now,
            ],
        )?;

        log::info!("MCP server saved: {}", server.id);
        Ok(())
    }

    pub fn get_mcp_servers(&self) -> Result<Vec<MCPServer>, Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        let mut stmt = conn.prepare(
            r#"
            SELECT id, name, description, server_type, command, args, env, port, url, api_key, enabled, created_at, updated_at
            FROM mcp_servers 
            ORDER BY name ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let server_type_str: String = row.get(3)?;
            let server_type = match server_type_str.as_str() {
                "stdio" => MCPServerType::Stdio,
                "sse" => MCPServerType::SSE,
                "http" => MCPServerType::HTTP,
                _ => MCPServerType::Stdio, // default
            };
            
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            
            Ok(MCPServer {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                server_type,
                command: row.get(4)?,
                args: serde_json::from_str(&args_json).unwrap_or_default(),
                env: serde_json::from_str(&env_json).unwrap_or_default(),
                port: row.get::<_, Option<i64>>(7)?.map(|p| p as u16),
                url: row.get(8)?,
                api_key: row.get(9)?,
                enabled: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;

        let servers: Result<Vec<_>, _> = rows.collect();
        Ok(servers?)
    }

    pub fn delete_mcp_server(&self, server_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            "DELETE FROM mcp_servers WHERE id = ?1",
            [server_id],
        )?;

        log::info!("MCP server deleted: {}", server_id);
        Ok(())
    }
}

// State wrapper for Tauri
pub struct DbState(pub Arc<Mutex<Database>>);
