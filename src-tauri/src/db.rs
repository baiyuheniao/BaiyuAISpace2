// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 数据库模块
 * 
 * 功能说明:
 * - SQLite 数据库初始化和表结构创建
 * - 会话 (Session) 管理 (CRUD)
 * - 消息 (Message) 管理
 * - MCP 服务器配置存储
 * - 数据库迁移支持
 * 
 * 数据库表:
 * - sessions: 聊天会话表
 * - messages: 消息表 (关联 sessions)
 * - mcp_servers: MCP 服务器配置表
 */

use crate::commands::llm::{ImageAttachment, VideoAttachment};
use base64::Engine as _;
use crate::types::{ChatMessage, ChatSession, ContextCompaction, MCPServer, MCPServerType, Skill};
use keyring::Entry;
use std::sync::Arc;
use tauri::Manager;

const MCP_KEYRING_SERVICE: &str = "mcp_api_key";

pub struct Database {
    pub path: String,
    pub conn: rusqlite::Connection,
    /// 消息附件（图片/视频）的落盘目录，每条消息一个子目录。数据库里只存文件清单，
    /// 不存 base64，避免 SQLite 文件随附件迅速膨胀。
    pub attachments_dir: std::path::PathBuf,
}

/// `messages.attachments` 列里保存的单个附件清单项。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAttachment {
    /// "image" 或 "video"
    kind: String,
    media_type: String,
    /// 相对该消息附件目录的文件名
    file: String,
}

fn extension_for(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        _ => "bin",
    }
}

impl Database {
    /**
     * 创建新的数据库实例
     * 
     * @param app_handle: Tauri 应用句柄
     * @return Database 实例
     */
    pub fn new(app_handle: &tauri::AppHandle) -> Self {
        let app_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir");
        std::fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
        let db_path = app_dir.join("app.db");
        
        let conn = rusqlite::Connection::open(&db_path).expect("Failed to open database");
        
        Self {
            path: db_path.to_str().unwrap().to_string(),
            conn,
            attachments_dir: app_dir.join("attachments"),
        }
    }

    /// 某条消息的附件目录。消息 ID 来自前端，只保留字母数字和 `-`/`_`，防止路径穿越。
    fn message_attachment_dir(&self, message_id: &str) -> std::path::PathBuf {
        let safe: String = message_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.attachments_dir.join(safe)
    }

    fn remove_message_attachments(&self, message_id: &str) {
        let dir = self.message_attachment_dir(message_id);
        if dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                log::warn!("删除消息附件目录失败 {}: {}", dir.display(), e);
            }
        }
    }

    /// 把消息的图片/视频写成文件，返回要存进数据库的清单 JSON；没有附件返回 None。
    /// 单个附件解码或写入失败只记日志并跳过，不让整条消息保存失败。
    ///
    /// 新文件的名字里带一段本次保存独有的随机串，所以永远不会覆盖或删除旧文件：
    /// 在数据库里的清单更新成功之前，旧清单指向的旧文件原封不动，任何一步失败或
    /// 应用中途退出，都只会留下多余的新文件，不会让旧附件丢失。多余的文件由
    /// `prune_message_attachments` 在清单更新成功之后清理。
    fn store_message_attachments(&self, message: &ChatMessage) -> Result<Option<String>, Box<dyn std::error::Error>> {
        if message.images.is_empty() && message.videos.is_empty() {
            return Ok(None);
        }
        let dir = self.message_attachment_dir(&message.id);
        std::fs::create_dir_all(&dir)?;
        let generation = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();

        let mut stored: Vec<StoredAttachment> = Vec::new();
        let items = message
            .images
            .iter()
            .map(|a| ("image", "img", &a.data, &a.media_type))
            .chain(message.videos.iter().map(|a| ("video", "vid", &a.data, &a.media_type)));
        for (index, (kind, prefix, data, media_type)) in items.enumerate() {
            let file = format!("{}{}-{}.{}", prefix, index, generation, extension_for(media_type));
            let bytes = match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("附件 base64 解码失败，已跳过 (message {}): {}", message.id, e);
                    continue;
                }
            };
            if let Err(e) = std::fs::write(dir.join(&file), bytes) {
                log::warn!("附件写入失败，已跳过 (message {}): {}", message.id, e);
                let _ = std::fs::remove_file(dir.join(&file));
                continue;
            }
            stored.push(StoredAttachment { kind: kind.to_string(), media_type: media_type.clone(), file });
        }
        if stored.is_empty() {
            // 一个都没存成：返回 None，SQL 会保留旧清单，旧文件也没动过。
            return Ok(None);
        }
        Ok(Some(serde_json::to_string(&stored)?))
    }

    /// 清单已经写进数据库之后，删掉该消息目录里清单没有引用的文件（上一版附件、
    /// 以及此前中断留下的多余文件）。
    fn prune_message_attachments(&self, message_id: &str, manifest: &str) {
        let Ok(items) = serde_json::from_str::<Vec<StoredAttachment>>(manifest) else { return };
        let keep: std::collections::HashSet<String> = items.into_iter().map(|i| i.file).collect();
        let dir = self.message_attachment_dir(message_id);
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !keep.contains(&name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// 按清单把附件文件读回前端使用的 base64 结构；缺失的文件直接略过。
    fn load_message_attachments(&self, message_id: &str, manifest: &str) -> (Vec<ImageAttachment>, Vec<VideoAttachment>) {
        let mut images = Vec::new();
        let mut videos = Vec::new();
        let Ok(items) = serde_json::from_str::<Vec<StoredAttachment>>(manifest) else {
            return (images, videos);
        };
        let dir = self.message_attachment_dir(message_id);
        for item in items {
            // 清单里的文件名只会是我们自己生成的，这里仍然只认单层文件名。
            if item.file.contains('/') || item.file.contains('\\') || item.file.contains("..") {
                continue;
            }
            let Ok(bytes) = std::fs::read(dir.join(&item.file)) else {
                log::warn!("附件文件缺失 (message {}): {}", message_id, item.file);
                continue;
            };
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            match item.kind.as_str() {
                "image" => images.push(ImageAttachment { data, media_type: item.media_type }),
                "video" => videos.push(VideoAttachment { data, media_type: item.media_type }),
                _ => {}
            }
        }
        (images, videos)
    }

    /**
     * 初始化数据库表结构
     * 
     * 创建以下表:
     * - sessions: 聊天会话
     * - messages: 消息 (外键关联 sessions)
     * - mcp_servers: MCP 服务器配置
     * 
     * 同时创建索引以优化查询性能
     */
    pub fn init(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                api_config_id TEXT NOT NULL DEFAULT '',
                working_directory TEXT,
                file_access_mode TEXT NOT NULL DEFAULT 'none',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        let has_column = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'api_config_id'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
        if !has_column {
            self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN api_config_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            log::info!("Database migration: added api_config_id column");
        }
        let has_working_directory = self.conn.query_row("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'working_directory'", [], |_| Ok(true)).unwrap_or(false);
        if !has_working_directory { self.conn.execute("ALTER TABLE sessions ADD COLUMN working_directory TEXT", [])?; }
        let has_file_access_mode = self.conn.query_row("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'file_access_mode'", [], |_| Ok(true)).unwrap_or(false);
        if !has_file_access_mode { self.conn.execute("ALTER TABLE sessions ADD COLUMN file_access_mode TEXT NOT NULL DEFAULT 'none'", [])?; }

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                error TEXT,
                token_usage TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
            [],
        )?;

        let has_token_usage = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'token_usage'",
            [],
            |_| Ok(true),
        ).unwrap_or(false);
        if !has_token_usage {
            self.conn.execute("ALTER TABLE messages ADD COLUMN token_usage TEXT", [])?;
        }

        // 助手回复触发的工具调用记录（JSON 文本）。旧库没有这一列，按需补上；
        // 补列前已存在的消息该列为 NULL，读出来就是"没有工具调用记录"。
        let has_tool_calls = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'tool_calls'",
            [],
            |_| Ok(true),
        ).unwrap_or(false);
        if !has_tool_calls {
            self.conn.execute("ALTER TABLE messages ADD COLUMN tool_calls TEXT", [])?;
        }

        // 图片/视频附件的文件清单（JSON 文本），文件本身在 attachments 目录下。
        let has_attachments = self.conn.query_row(
            "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'attachments'",
            [],
            |_| Ok(true),
        ).unwrap_or(false);
        if !has_attachments {
            self.conn.execute("ALTER TABLE messages ADD COLUMN attachments TEXT", [])?;
        }

        // 压缩记录与原消息分表保存：UI/导出继续读取完整消息，模型请求才按这个
        // 记录把旧历史替换成摘要。这避免“为省 token 而丢聊天记录”。
        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS context_compactions (
                session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
                summary TEXT NOT NULL,
                preserve_from_timestamp INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                server_type TEXT NOT NULL,
                command TEXT,
                args TEXT,
                env TEXT,
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

        self.conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                instructions TEXT NOT NULL DEFAULT '',
                bound_mcp_server_ids TEXT NOT NULL DEFAULT '[]',
                enabled BOOLEAN NOT NULL DEFAULT 1,
                resource_files TEXT NOT NULL DEFAULT '[]',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_skills_enabled ON skills(enabled)",
            [],
        )?;

        log::info!("Database initialized at: {}", self.path);
        Ok(())
    }

    /**
     * 保存会话到数据库
     * 
     * @param session: 要保存的会话对象
     */
    pub fn save_session(&self, session: &ChatSession) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            r#"
            INSERT INTO sessions (id, title, provider, model, api_config_id, working_directory, file_access_mode, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                provider = excluded.provider,
                model = excluded.model,
                api_config_id = excluded.api_config_id,
                working_directory = excluded.working_directory,
                file_access_mode = excluded.file_access_mode,
                updated_at = excluded.updated_at
            "#,
            rusqlite::params![
                &session.id,
                &session.title,
                &session.provider,
                &session.model,
                &session.api_config_id,
                &session.working_directory,
                &session.file_access_mode,
                session.created_at,
                session.updated_at,
            ],
        )?;

        log::info!("Session saved: {}", session.id);
        Ok(())
    }

    /**
     * 删除会话
     * 
     * @param session_id: 要删除的会话 ID
     */
    pub fn delete_session(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // 级联删除消息行之前先记下消息 ID，之后才能清理它们的附件文件。
        let mut stmt = self.conn.prepare("SELECT id FROM messages WHERE session_id = ?1")?;
        let message_ids: Vec<String> = stmt
            .query_map([session_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            [session_id],
        )?;
        for message_id in &message_ids {
            self.remove_message_attachments(message_id);
        }

        log::info!("Session deleted: {}", session_id);
        Ok(())
    }

    /**
     * 获取所有会话
     * 按最后更新时间倒序排列
     * 
     * @return 会话列表 (包含消息)
     */
    pub fn get_sessions(&self) -> Result<Vec<ChatSession>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, title, provider, model, api_config_id, working_directory, file_access_mode, created_at, updated_at
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
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, title, provider, model, api_config_id, working_directory, file_access_mode, created_at, updated_at) = row?;
            // 列表不带附件内容；需要附件时用 get_session 单独取某个会话。
            let messages = self.get_messages_with(&id, false)?;
            
            sessions.push(ChatSession {
                id,
                title,
                provider,
                model,
                api_config_id,
                working_directory,
                file_access_mode,
                created_at,
                updated_at,
                messages,
            });
        }

        log::info!("Loaded {} sessions", sessions.len());
        for s in &sessions {
            log::debug!("Session {} has {} messages", s.id, s.messages.len());
        }
        Ok(sessions)
    }

    /// 读取单个会话（含消息及其图片/视频附件）。打开某个会话时用它，而不是把所有
    /// 会话的附件都读一遍。
    pub fn get_session(&self, session_id: &str) -> Result<Option<ChatSession>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, title, provider, model, api_config_id, working_directory, file_access_mode, created_at, updated_at
            FROM sessions
            WHERE id = ?1
            "#,
        )?;
        let mut rows = stmt.query([session_id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let id: String = row.get(0)?;
        let messages = self.get_messages(&id)?;
        Ok(Some(ChatSession {
            id,
            title: row.get(1)?,
            provider: row.get(2)?,
            model: row.get(3)?,
            api_config_id: row.get(4)?,
            working_directory: row.get(5)?,
            file_access_mode: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            messages,
        }))
    }

    /**
     * 保存消息到数据库
     * 同时更新会话的 updated_at 时间戳
     * 
     * @param session_id: 所属会话 ID
     * @param message: 要保存的消息
     */
    pub fn save_message(
        &self,
        session_id: &str,
        message: &ChatMessage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let token_usage = message.token_usage.as_ref().map(serde_json::to_string).transpose()?;
        // 空数组等同于没有记录，统一存 NULL，避免给每条普通回复都落一个 "[]"。
        let tool_calls = message
            .tool_calls
            .as_ref()
            .filter(|value| !value.is_null() && value.as_array().map_or(true, |items| !items.is_empty()))
            .map(serde_json::to_string)
            .transpose()?;
        let attachments = self.store_message_attachments(message)?;
        self.conn.execute(
            r#"
            INSERT INTO messages (id, session_id, role, content, timestamp, error, token_usage, tool_calls, attachments)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                content = excluded.content,
                error = excluded.error,
                token_usage = excluded.token_usage,
                tool_calls = excluded.tool_calls,
                -- 这次保存没带附件（比如只是回写助手回复）时保留原有附件，不当作"清空"
                attachments = COALESCE(excluded.attachments, messages.attachments)
            "#,
            rusqlite::params![
                message.id,
                session_id,
                message.role,
                message.content,
                message.timestamp,
                message.error.as_deref().unwrap_or(""),
                token_usage,
                tool_calls,
                attachments,
            ],
        )?;

        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            [&chrono::Utc::now().timestamp_millis().to_string(), session_id],
        )?;

        // 清单已经安全落库，这时才清理上一版附件文件。
        if let Some(manifest) = attachments.as_deref() {
            self.prune_message_attachments(&message.id, manifest);
        }

        log::info!("[save_message] saved message {} for session {}", message.id, session_id);
        Ok(())
    }

    /**
     * 删除单条消息
     * 用于消息编辑（截断编辑点之后的旧消息）和重新生成（删除待重生成的回复）
     *
     * @param message_id: 要删除的消息 ID
     */
    pub fn delete_message(&self, message_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "DELETE FROM messages WHERE id = ?1",
            [message_id],
        )?;
        self.remove_message_attachments(message_id);

        log::info!("Message deleted: {}", message_id);
        Ok(())
    }

    /**
     * 获取指定会话的所有消息
     * 按时间戳升序排列
     *
     * @param session_id: 会话 ID
     * @return 消息列表
     */
    pub fn get_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
        self.get_messages_with(session_id, true)
    }

    /// `include_attachments` 为 false 时不读附件文件（图片/视频留空），用于一次返回
    /// 所有会话的列表——不能为了列表把整个媒体库都读成 base64 传给前端。
    pub fn get_messages_with(
        &self,
        session_id: &str,
        include_attachments: bool,
    ) -> Result<Vec<ChatMessage>, Box<dyn std::error::Error>> {
        log::info!("[get_messages] Querying messages for session_id: {}, db_path: {}", session_id, self.path);
        
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, role, content, timestamp, error, token_usage, tool_calls, attachments
            FROM messages
            WHERE session_id = ?
            ORDER BY timestamp ASC
            "#,
        )?;

        let rows = stmt.query_map([session_id], |row| {
            let error: Option<String> = row.get(4)?;
            let token_usage_json: Option<String> = row.get(5)?;
            let tool_calls_json: Option<String> = row.get(6)?;
            let attachments_json: Option<String> = row.get(7)?;
            let id: String = row.get(0)?;
            let (images, videos) = match attachments_json.as_deref() {
                Some(manifest) if include_attachments => self.load_message_attachments(&id, manifest),
                _ => (vec![], vec![]),
            };
            Ok(ChatMessage {
                id,
                role: row.get(1)?,
                content: row.get(2)?,
                timestamp: row.get(3)?,
                error: if error.as_deref() == Some("") { None } else { error },
                images,
                videos,
                token_usage: token_usage_json.as_deref().and_then(|value| serde_json::from_str(value).ok()),
                tool_calls: tool_calls_json.as_deref().and_then(|value| serde_json::from_str(value).ok()),
            })
        })?;

        let messages: Result<Vec<_>, _> = rows.collect();
        log::info!("get_messages for session {}: {} messages", session_id, messages.as_ref().map(|m| m.len()).unwrap_or(0));
        Ok(messages?)
    }

    /// 读取某个会话当前生效的压缩快照。缺失代表该会话仍按完整历史发送。
    pub fn get_context_compaction(
        &self,
        session_id: &str,
    ) -> Result<Option<ContextCompaction>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            "SELECT summary, preserve_from_timestamp, created_at FROM context_compactions WHERE session_id = ?1",
        )?;
        let mut rows = stmt.query([session_id])?;
        Ok(rows.next()?.map(|row| -> rusqlite::Result<ContextCompaction> {
            Ok(ContextCompaction {
                summary: row.get(0)?,
                preserve_from_timestamp: row.get(1)?,
                created_at: row.get(2)?,
            })
        }).transpose()?)
    }

    /// 新摘要覆盖旧摘要，但永远不覆盖 `messages` 表中的原始对话。
    pub fn save_context_compaction(
        &self,
        session_id: &str,
        compaction: &ContextCompaction,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            r#"
            INSERT INTO context_compactions (session_id, summary, preserve_from_timestamp, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(session_id) DO UPDATE SET
                summary = excluded.summary,
                preserve_from_timestamp = excluded.preserve_from_timestamp,
                created_at = excluded.created_at
            "#,
            rusqlite::params![session_id, compaction.summary, compaction.preserve_from_timestamp, compaction.created_at],
        )?;
        Ok(())
    }

    /**
     * 保存 MCP 服务器配置
     * 
     * @param server: MCP 服务器配置
     */
    pub fn save_mcp_server(&self, server: &MCPServer) -> Result<(), Box<dyn std::error::Error>> {
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
        
        if let Some(ref api_key) = server.api_key {
            if !api_key.is_empty() {
                let entry = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server.id, "api_key"))
                    .map_err(|e| { log::error!("创建密钥链条目失败（详情：{}）", e); "保存 API 密钥失败，请检查系统密钥链权限".to_string() })?;
                entry.set_password(api_key)
                    .map_err(|e| { log::error!("写入密钥链失败（详情：{}）", e); "保存 API 密钥失败，请检查系统密钥链权限".to_string() })?;
            }
        }
        
        let has_api_key_in_keyring = server.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false);
        
        self.conn.execute(
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
                if has_api_key_in_keyring { Some("__KEYRING__") } else { None },
                &server.enabled,
                &now,
                &now,
            ],
        )?;

        log::info!("MCP server saved: {} (API key stored in keyring)", server.id);
        Ok(())
    }

    fn get_mcp_api_key_from_keyring(server_id: &str) -> Option<String> {
        let entry = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")).ok()?;
        entry.get_password().ok()
    }

    pub fn get_mcp_servers(&self) -> Result<Vec<MCPServer>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
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
                _ => MCPServerType::Stdio,
            };
            
            let args_json: String = row.get(5)?;
            let env_json: String = row.get(6)?;
            let api_key_placeholder: Option<String> = row.get(9)?;
            
            let api_key = api_key_placeholder
                .filter(|k| k == "__KEYRING__")
                .and_then(|_| {
                    let id: Result<String, _> = row.get(0);
                    id.ok().and_then(|id_str| Self::get_mcp_api_key_from_keyring(&id_str))
                });
            
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
                api_key,
                enabled: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;

        let servers: Result<Vec<_>, _> = rows.collect();
        Ok(servers?)
    }

    /**
     * 删除 MCP 服务器配置
     * 
     * @param server_id: 要删除的服务器 ID
     */
    pub fn delete_mcp_server(&self, server_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Ok(entry) = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")) {
            let _ = entry.delete_credential();
        }
        
        self.conn.execute(
            "DELETE FROM mcp_servers WHERE id = ?1",
            [server_id],
        )?;

        log::info!("MCP server deleted: {} (including keyring entry)", server_id);
        Ok(())
    }

    /**
     * 保存 Skill 配置 (新建或更新)
     */
    pub fn save_skill(&self, skill: &Skill) -> Result<(), Box<dyn std::error::Error>> {
        let bound_servers_json = serde_json::to_string(&skill.bound_mcp_server_ids)?;
        let resource_files_json = serde_json::to_string(&skill.resource_files)?;

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO skills
            (id, name, description, instructions, bound_mcp_server_ids, enabled, resource_files, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            rusqlite::params![
                &skill.id,
                &skill.name,
                &skill.description,
                &skill.instructions,
                &bound_servers_json,
                &skill.enabled,
                &resource_files_json,
                &skill.created_at,
                &skill.updated_at,
            ],
        )?;

        log::info!("Skill saved: {}", skill.id);
        Ok(())
    }

    /**
     * 获取所有 Skill
     */
    pub fn get_skills(&self) -> Result<Vec<Skill>, Box<dyn std::error::Error>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, description, instructions, bound_mcp_server_ids, enabled, resource_files, created_at, updated_at
            FROM skills
            ORDER BY name ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let bound_servers_json: String = row.get(4)?;
            let resource_files_json: String = row.get(6)?;

            Ok(Skill {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                instructions: row.get(3)?,
                bound_mcp_server_ids: serde_json::from_str(&bound_servers_json).unwrap_or_default(),
                enabled: row.get(5)?,
                resource_files: serde_json::from_str(&resource_files_json).unwrap_or_default(),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let skills: Result<Vec<_>, _> = rows.collect();
        Ok(skills?)
    }

    /**
     * 删除 Skill
     */
    pub fn delete_skill(&self, skill_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.execute(
            "DELETE FROM skills WHERE id = ?1",
            [skill_id],
        )?;

        log::info!("Skill deleted: {}", skill_id);
        Ok(())
    }

    /**
     * 清空数据库：删除所有会话、消息、MCP 服务器配置、Skill。
     * 不涉及知识库 / 协作团队 / 定时任务，那些是各自独立的 SQLite 文件。
     */
    pub fn clear_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        // MCP 服务器的 API Key 存在系统密钥链里，delete 前先逐个清理，
        // 避免只删表行、密钥链里留下孤儿凭据。
        let mut stmt = self.conn.prepare("SELECT id FROM mcp_servers")?;
        let server_ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for server_id in &server_ids {
            if let Ok(entry) = Entry::new(MCP_KEYRING_SERVICE, &format!("{}_{}", server_id, "api_key")) {
                let _ = entry.delete_credential();
            }
        }

        self.conn.execute("DELETE FROM messages", [])?;
        self.conn.execute("DELETE FROM sessions", [])?;
        if self.attachments_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&self.attachments_dir) {
                log::warn!("清空附件目录失败 {}: {}", self.attachments_dir.display(), e);
            }
        }
        self.conn.execute("DELETE FROM mcp_servers", [])?;
        self.conn.execute("DELETE FROM skills", [])?;
        self.conn.execute_batch("VACUUM")?;
        log::info!("Database cleared: all sessions, messages, mcp_servers, skills removed");
        Ok(())
    }
}

/// 数据库状态封装结构
/// 用于在 Tauri 应用中共享数据库实例
pub struct DbState(pub Arc<tokio::sync::Mutex<Database>>);

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        let db = Database {
            path: ":memory:".to_string(),
            conn: rusqlite::Connection::open_in_memory().expect("open in-memory db"),
            attachments_dir: std::env::temp_dir().join(format!("baiyu-attachments-test-{}", uuid::Uuid::new_v4())),
        };
        db.init().expect("init schema");
        db
    }

    fn session(id: &str) -> ChatSession {
        ChatSession {
            id: id.into(),
            title: "测试会话".into(),
            messages: vec![],
            created_at: 1,
            updated_at: 1,
            provider: "openai".into(),
            model: "test-model".into(),
            api_config_id: "cfg".into(),
            working_directory: None,
            file_access_mode: "none".into(),
        }
    }

    fn assistant(id: &str, timestamp: i64, content: &str, tool_calls: Option<serde_json::Value>) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: "assistant".into(),
            content: content.into(),
            timestamp,
            error: None,
            images: vec![],
            videos: vec![],
            token_usage: None,
            tool_calls,
        }
    }

    fn sample_tool_calls() -> serde_json::Value {
        serde_json::json!([
            { "callId": "call_1", "toolName": "baiyu_file_search", "arguments": "{\"query\":\"x\"}",
              "status": "done", "result": "{\"display_type\":\"preview\",\"preview\":\"ok\"}", "contentOffset": 14 },
            { "callId": "call_2", "toolName": "baiyu_file_read", "arguments": "{}",
              "status": "error", "result": "{\"error\":\"boom\"}", "contentOffset": 14 }
        ])
    }

    #[test]
    fn tool_calls_round_trip_through_save_and_get() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &assistant("m1", 10, "我先搜一下。找到了。", Some(sample_tool_calls()))).unwrap();

        let messages = db.get_messages("s1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].tool_calls, Some(sample_tool_calls()), "工具调用记录必须原样取回，包括 contentOffset");
    }

    #[test]
    fn messages_without_tool_calls_read_back_as_none() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &assistant("plain", 10, "没有调用工具", None)).unwrap();
        db.save_message("s1", &assistant("empty", 20, "空数组", Some(serde_json::json!([])))).unwrap();
        db.save_message("s1", &assistant("null", 30, "null 值", Some(serde_json::Value::Null))).unwrap();

        for message in db.get_messages("s1").unwrap() {
            assert_eq!(message.tool_calls, None, "{} 不应落库任何工具调用记录", message.id);
        }
        let stored: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM messages WHERE tool_calls IS NOT NULL", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, 0, "空记录统一存 NULL，不写 \"[]\"");
    }

    #[test]
    fn saving_a_message_again_updates_its_tool_calls() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &assistant("m1", 10, "草稿", None)).unwrap();
        db.save_message("s1", &assistant("m1", 10, "终稿", Some(sample_tool_calls()))).unwrap();

        let messages = db.get_messages("s1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "终稿");
        assert_eq!(messages[0].tool_calls, Some(sample_tool_calls()));
    }

    #[test]
    fn init_adds_tool_calls_column_to_a_legacy_messages_table() {
        // 模拟升级前的旧库：messages 表没有 tool_calls 列，里面已有历史消息。
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                error TEXT,
                token_usage TEXT
            );
            INSERT INTO messages (id, session_id, role, content, timestamp, error, token_usage)
            VALUES ('old', 's1', 'assistant', '升级前的回复', 5, '', NULL);
            "#,
        )
        .unwrap();
        let db = Database {
            path: ":memory:".to_string(),
            conn,
            attachments_dir: std::env::temp_dir().join(format!("baiyu-attachments-test-{}", uuid::Uuid::new_v4())),
        };

        db.init().expect("迁移旧库不应失败");
        db.init().expect("迁移必须可重复执行");

        let messages = db.get_messages("s1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "升级前的回复");
        assert_eq!(messages[0].tool_calls, None);
    }

    // ---- 图片/视频附件落盘 ----

    /// 某条消息附件目录下现有的文件名（排序后）。
    fn attachment_files(db: &Database, message_id: &str) -> Vec<String> {
        let mut names: Vec<String> = match std::fs::read_dir(db.message_attachment_dir(message_id)) {
            Ok(entries) => entries.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect(),
            Err(_) => vec![],
        };
        names.sort();
        names
    }

    fn user_with_attachments(id: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            role: "user".into(),
            content: "看看这张图和这段视频".into(),
            timestamp: 1,
            error: None,
            images: vec![
                ImageAttachment { data: "iVBORw0KGgo=".into(), media_type: "image/png".into() },
                ImageAttachment { data: "/9j/4AAQ".into(), media_type: "image/jpeg".into() },
            ],
            videos: vec![VideoAttachment { data: "AAAAGGZ0eXA=".into(), media_type: "video/mp4".into() }],
            token_usage: None,
            tool_calls: None,
        }
    }

    #[test]
    fn attachments_round_trip_as_files_and_not_as_base64_in_the_database() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 2);
        assert_eq!(loaded.images[0].data, "iVBORw0KGgo=");
        assert_eq!(loaded.images[0].media_type, "image/png");
        assert_eq!(loaded.images[1].media_type, "image/jpeg");
        assert_eq!(loaded.videos.len(), 1);
        assert_eq!(loaded.videos[0].data, "AAAAGGZ0eXA=");

        let manifest: String = db.conn.query_row("SELECT attachments FROM messages WHERE id = 'm1'", [], |r| r.get(0)).unwrap();
        assert!(!manifest.contains("iVBORw0KGgo"), "数据库里只能有文件清单，不能有 base64 内容");
        assert_eq!(attachment_files(&db, "m1").len(), 3, "两张图一段视频，各一个文件");
    }

    #[test]
    fn messages_without_attachments_have_no_files_and_read_back_empty() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &assistant("m1", 1, "纯文字", None)).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert!(loaded.images.is_empty() && loaded.videos.is_empty());
        assert!(!db.message_attachment_dir("m1").exists());
    }

    #[test]
    fn resaving_a_message_without_attachments_keeps_the_existing_ones() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();

        let mut resaved = user_with_attachments("m1");
        resaved.content = "改过的文字".into();
        resaved.images.clear();
        resaved.videos.clear();
        db.save_message("s1", &resaved).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.content, "改过的文字");
        assert_eq!(loaded.images.len(), 2, "没带附件的回写不能把已有附件清空");
        assert_eq!(loaded.videos.len(), 1);
    }

    #[test]
    fn deleting_a_message_or_a_session_removes_its_attachment_files() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();
        db.save_message("s1", &user_with_attachments("m2")).unwrap();

        db.delete_message("m1").unwrap();
        assert!(!db.message_attachment_dir("m1").exists());
        assert!(db.message_attachment_dir("m2").exists());

        db.delete_session("s1").unwrap();
        assert!(!db.message_attachment_dir("m2").exists(), "删除会话时它所有消息的附件也要清掉");
    }

    #[test]
    fn a_missing_attachment_file_is_skipped_instead_of_failing_the_load() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();
        let png = attachment_files(&db, "m1").into_iter().find(|f| f.starts_with("img0-") && f.ends_with(".png")).expect("png file");
        std::fs::remove_file(db.message_attachment_dir("m1").join(png)).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 1);
        assert_eq!(loaded.images[0].media_type, "image/jpeg");
        assert_eq!(loaded.videos.len(), 1);
    }

    #[test]
    fn hostile_message_ids_cannot_escape_the_attachments_directory() {
        let db = test_db();
        let dir = db.message_attachment_dir("../../etc/passwd");
        assert!(dir.starts_with(&db.attachments_dir), "{}", dir.display());
        assert!(!dir.to_string_lossy().contains(".."));
    }

    #[test]
    fn clear_all_removes_every_attachment_file() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();
        db.clear_all().unwrap();
        assert!(!db.attachments_dir.exists());
    }

    #[test]
    fn a_failed_replacement_keeps_the_previous_attachments_and_manifest() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();
        let before = attachment_files(&db, "m1");

        // 新附件全部无法解码 => 没有任何东西可存，旧附件必须原样保留
        let mut broken = user_with_attachments("m1");
        broken.images = vec![ImageAttachment { data: "不是base64!!".into(), media_type: "image/png".into() }];
        broken.videos.clear();
        db.save_message("s1", &broken).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 2, "替换失败不能丢掉原有附件");
        assert_eq!(loaded.videos.len(), 1);
        assert_eq!(attachment_files(&db, "m1"), before);
    }

    #[test]
    fn replacing_attachments_installs_the_new_set_and_prunes_the_old_files() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();

        let mut replaced = user_with_attachments("m1");
        replaced.images = vec![ImageAttachment { data: "R0lGODlh".into(), media_type: "image/gif".into() }];
        replaced.videos.clear();
        db.save_message("s1", &replaced).unwrap();

        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 1);
        assert_eq!(loaded.images[0].media_type, "image/gif");
        assert!(loaded.videos.is_empty());
        let files = attachment_files(&db, "m1");
        assert_eq!(files.len(), 1, "旧文件不能残留: {:?}", files);
        assert!(files[0].ends_with(".gif"));
    }

    #[test]
    fn old_attachments_survive_until_the_new_manifest_is_committed() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();

        // 模拟"新文件已写好，但数据库里的清单还没更新就退出/失败"：只做写文件这一步
        let mut replaced = user_with_attachments("m1");
        replaced.images = vec![ImageAttachment { data: "R0lGODlh".into(), media_type: "image/gif".into() }];
        replaced.videos.clear();
        let new_manifest = db.store_message_attachments(&replaced).unwrap();
        assert!(new_manifest.is_some());

        // 数据库仍是旧清单，旧文件一个都不能少，读出来还是原来的两张图和一段视频
        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 2);
        assert_eq!(loaded.images[0].media_type, "image/png");
        assert_eq!(loaded.videos.len(), 1);

        // 之后正常保存一次：新清单落库，多余的文件（包括上面遗留的）被清掉
        db.save_message("s1", &replaced).unwrap();
        let loaded = &db.get_messages("s1").unwrap()[0];
        assert_eq!(loaded.images.len(), 1);
        assert_eq!(attachment_files(&db, "m1").len(), 1);
    }

    #[test]
    fn session_list_skips_attachment_bytes_but_single_session_includes_them() {
        let db = test_db();
        db.save_session(&session("s1")).unwrap();
        db.save_message("s1", &user_with_attachments("m1")).unwrap();

        let listed = &db.get_sessions().unwrap()[0].messages[0];
        assert_eq!(listed.content, "看看这张图和这段视频", "列表照常带文字");
        assert!(listed.images.is_empty() && listed.videos.is_empty(), "列表不能把所有附件读成 base64");

        let full = db.get_session("s1").unwrap().expect("session exists");
        assert_eq!(full.messages[0].images.len(), 2);
        assert_eq!(full.messages[0].videos.len(), 1);
        assert!(db.get_session("missing").unwrap().is_none());
    }
}
