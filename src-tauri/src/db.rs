// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::commands::llm::{ChatMessage, ChatSession};
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
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

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

        log::info!("Database initialized at: {}", self.path);
        Ok(())
    }

    pub fn save_session(&self, session: &ChatSession) -> Result<(), Box<dyn std::error::Error>> {
        let conn = rusqlite::Connection::open(&self.path)?;
        
        conn.execute(
            r#"
            INSERT OR REPLACE INTO sessions (id, title, provider, model, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            [
                &session.id,
                &session.title,
                &session.provider,
                &session.model,
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
            SELECT id, title, provider, model, created_at, updated_at 
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
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, title, provider, model, created_at, updated_at) = row?;
            let messages = self.get_messages(&id)?;
            
            sessions.push(ChatSession {
                id,
                title,
                provider,
                model,
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
}

// State wrapper for Tauri
pub struct DbState(pub Arc<Mutex<Database>>);
