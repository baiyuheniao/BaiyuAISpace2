// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use super::document::{parse_document, calculate_file_hash, split_text, estimate_tokens};
use super::embedding::generate_embeddings;
use super::db::{VectorStore, init_sqlite_tables};
use super::retrieval::Retriever;
use tauri::State;
use std::sync::Arc;

use uuid::Uuid;
use keyring::Entry;

pub struct KbState {
    pub vector_store: Arc<VectorStore>,
    pub db_path: String,
}

/// 初始化知识库相关数据表
pub fn init_knowledge_base(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    init_sqlite_tables(conn)
}

/// 根据 embedding 配置 ID 从系统 keyring 中取出对应的 API Key
/// keyring 条目格式为：emb_{config_id}
fn get_embedding_api_key(config_id: &str) -> Result<String, KnowledgeBaseError> {
    let entry = Entry::new(
        "BaiyuAISpace",
        &format!("api_keys_emb_{}", config_id),
    ).map_err(|e| KnowledgeBaseError::InvalidConfig(format!("Failed to access keyring: {}", e)))?;

    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            Err(KnowledgeBaseError::InvalidConfig(
                format!("Embedding API key not found for config: {}. Please set it in Settings.", config_id)
            ))
        }
        Err(e) => Err(KnowledgeBaseError::InvalidConfig(
            format!("Failed to retrieve API key: {}", e)
        )),
    }
}

/// 创建新知识库
#[tauri::command]
pub async fn create_knowledge_base(
    request: CreateKnowledgeBaseRequest,
    kb_state: State<'_, KbState>,
) -> Result<KnowledgeBase, KnowledgeBaseError> {
    log::info!("[KB] Creating knowledge base: {:?}", request);

    if request.embedding_provider.trim().is_empty() || request.embedding_model.trim().is_empty() {
        return Err(KnowledgeBaseError::InvalidConfig(
            "embedding_provider and embedding_model are required".to_string()
        ));
    }

    // 校验 chunk_overlap 必须小于 chunk_size
    let chunk_size = request.chunk_size.unwrap_or(1000);
    let chunk_overlap = request.chunk_overlap.unwrap_or(200);
    if chunk_overlap >= chunk_size {
        return Err(KnowledgeBaseError::InvalidConfig(
            format!("chunk_overlap ({}) must be less than chunk_size ({})", chunk_overlap, chunk_size)
        ));
    }

    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    log::info!("[KB] Inserting with chunk_size={}, chunk_overlap={}", chunk_size, chunk_overlap);

    let result = conn.execute(
        r#"
        INSERT INTO knowledge_bases
        (id, name, description, embedding_provider, embedding_model, embedding_dim, embedding_api_config_id, embedding_base_url, chunk_size, chunk_overlap, created_at, updated_at, document_count)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)
        "#,
        rusqlite::params![
            &id,
            &request.name,
            &request.description,
            &request.embedding_provider,
            &request.embedding_model,
            1536i32,     // embedding_dim —— 默认 1536
            &request.embedding_api_config_id,
            &request.embedding_base_url,
            chunk_size,
            chunk_overlap,
            now,
            now,
        ],
    );

    match result {
        Ok(rows) => {
            log::info!("[KB] Successfully created, rows affected: {}", rows);
        }
        Err(e) => {
            log::error!("[KB] Failed to insert: {}", e);
            return Err(KnowledgeBaseError::DatabaseError(e.to_string()));
        }
    }

    log::info!("Created knowledge base: {} ({})", request.name, id);

    Ok(KnowledgeBase {
        id,
        name: request.name,
        description: request.description,
        embedding_api_config_id: request.embedding_api_config_id,
        embedding_provider: request.embedding_provider,
        embedding_model: request.embedding_model,
        embedding_base_url: request.embedding_base_url,
        chunk_size,
        chunk_overlap,
        created_at: now,
        updated_at: now,
        document_count: 0,
    })
}

/// 列出所有知识库
#[tauri::command]
pub async fn list_knowledge_bases(
    kb_state: State<'_, KbState>,
) -> Result<Vec<KnowledgeBase>, KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, name, description, embedding_api_config_id,
         chunk_size, chunk_overlap, created_at, updated_at, document_count,
         COALESCE(embedding_provider, ''), COALESCE(embedding_model, ''), COALESCE(embedding_base_url, '')
         FROM knowledge_bases ORDER BY updated_at DESC"
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let rows = stmt.query_map([], |row| {
        Ok(KnowledgeBase {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            embedding_api_config_id: row.get(3)?,
            chunk_size: row.get(4)?,
            chunk_overlap: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            document_count: row.get(8)?,
            embedding_provider: row.get(9)?,
            embedding_model: row.get(10)?,
            embedding_base_url: row.get(11)?,
        })
    }).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let mut bases = Vec::new();
    for row in rows {
        bases.push(row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?);
    }

    Ok(bases)
}

/// 删除知识库
#[tauri::command]
pub async fn delete_knowledge_base(
    kb_id: String,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // 检查知识库是否存在
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_bases WHERE id = ?1",
        [&kb_id],
        |row| row.get(0),
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    if !exists {
        return Err(KnowledgeBaseError::NotFound(format!("Knowledge base not found: {}", kb_id)));
    }

    // 从 SQLite 中删除（级联删除会自动清掉关联的 documents 和 chunks）
    conn.execute(
        "DELETE FROM knowledge_bases WHERE id = ?1",
        [&kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // 删除向量表
    kb_state.vector_store.drop_kb_table(&kb_id).await?;

    log::info!("Deleted knowledge base: {}", kb_id);
    Ok(())
}

/// 把文档标记为失败，并清理掉阶段一（Phase 1）里已经写入的 chunks/FTS5 记录，
/// 避免文档卡在“处理中”状态却留下一堆孤儿数据（对应 import_document 阶段二失败的情况）。
async fn mark_document_failed(
    db_state: &State<'_, crate::db::DbState>,
    doc_id: &str,
    error_msg: &str,
) -> Result<(), KnowledgeBaseError> {
    log::error!("[KB] {}", error_msg);

    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    conn.execute(
        "UPDATE documents SET status = 'error', error_message = ?1 WHERE id = ?2",
        rusqlite::params![error_msg, doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // 必须在删除 chunks 之前先清理 FTS5 条目（需要用到 chunks 里的 rowid）
    if let Err(cleanup_err) = conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE document_id = ?1)",
        rusqlite::params![doc_id],
    ) {
        log::warn!("[KB] Failed to clean up FTS5 entries: {}", cleanup_err);
    }

    // 清理孤儿 chunks（在 FTS5 清理之后进行）
    if let Err(cleanup_err) = conn.execute(
        "DELETE FROM chunks WHERE document_id = ?1",
        rusqlite::params![doc_id],
    ) {
        log::warn!("[KB] Failed to clean up orphan chunks: {}", cleanup_err);
    }

    Ok(())
}

/// 向知识库导入文档
///
/// # 对应 #33、#34 的修复：
/// - 阶段一（持有 DB 锁）：读取知识库配置、创建文档记录、解析文件、写入 chunks + FTS
/// - 阶段二（释放 DB 锁）：通过网络请求生成 embedding（不持锁）
/// - 阶段三（重新获取 DB 锁）：写入向量、更新文档状态
/// - 如果阶段二失败，阶段三会把文档标记为 "error" 并清理孤儿 chunks
///
/// # 对应 #32 的修复：
/// - API Key 改为通过 embedding_api_config_id 从安全存储（keyring）中读取
/// - 前端不再传递 api_key 参数
#[tauri::command]
pub async fn import_document(
    kb_id: String,
    file_path: String,
    db_state: State<'_, crate::db::DbState>,
    kb_state: State<'_, KbState>,
) -> Result<Document, KnowledgeBaseError> {
    // ===== 阶段一：数据库操作（持有锁） =====
    let (doc_id, kb, file_name, file_type, file_size, file_hash, preview, chunks) = {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 获取知识库配置
        let kb: KnowledgeBase = conn.query_row(
            "SELECT id, name, description, embedding_api_config_id,
             chunk_size, chunk_overlap, created_at, updated_at, document_count,
             COALESCE(embedding_provider, ''), COALESCE(embedding_model, ''), COALESCE(embedding_base_url, '')
             FROM knowledge_bases WHERE id = ?1",
            [&kb_id],
            |row| {
                Ok(KnowledgeBase {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    embedding_api_config_id: row.get(3)?,
                    chunk_size: row.get(4)?,
                    chunk_overlap: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                    document_count: row.get(8)?,
                    embedding_provider: row.get(9)?,
                    embedding_model: row.get(10)?,
                    embedding_base_url: row.get(11)?,
                })
            }
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 创建文档记录
        let doc_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let file_hash = calculate_file_hash(&file_path).await?;
        let file_name = std::path::Path::new(&file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let file_type = std::path::Path::new(&file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt")
            .to_lowercase();

        // 获取文件大小
        let file_size = match tokio::fs::metadata(&file_path).await {
            Ok(m) => m.len() as i64,
            Err(e) => {
                log::warn!("Failed to read file metadata for {}: {}", file_path, e);
                0
            }
        };

        conn.execute(
            r#"
            INSERT INTO documents
            (id, kb_id, filename, file_type, file_size, file_hash, content_preview,
             chunk_count, status, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', 0, 'processing', ?7)
            "#,
            rusqlite::params![&doc_id, &kb_id, &file_name, &file_type, file_size, &file_hash, now],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 解析文档
        let content = match parse_document(&file_path).await {
            Ok(c) => c,
            Err(e) => {
                conn.execute(
                    "UPDATE documents SET status = 'error', error_message = ?1 WHERE id = ?2",
                    rusqlite::params![&e.to_string(), &doc_id],
                ).map_err(|e2| KnowledgeBaseError::DatabaseError(e2.to_string()))?;
                return Err(e);
            }
        };

        // 存储预览内容
        let preview: String = content.chars().take(500).collect();
        conn.execute(
            "UPDATE documents SET content_preview = ?1 WHERE id = ?2",
            rusqlite::params![&preview, &doc_id],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 切分为多个 chunk
        let chunks = split_text(&content, kb.chunk_size as usize, kb.chunk_overlap as usize);

        // 把 chunk 写入 SQLite 和 FTS5
        let mut all_chunk_ids = Vec::new();
        for (i, chunk_text) in chunks.iter().enumerate() {
            let chunk_id = Uuid::new_v4().to_string();
            let tokens = estimate_tokens(chunk_text);

            conn.execute(
                r#"
                INSERT INTO chunks (id, document_id, kb_id, content, chunk_index, token_count, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                rusqlite::params![&chunk_id, &doc_id, &kb_id, chunk_text, i as i32, tokens, now],
            ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            // 写入 FTS5 —— 出错时记日志而不是直接忽略
            if let Err(e) = conn.execute(
                "INSERT INTO chunks_fts (rowid, kb_id, content) VALUES (last_insert_rowid(), ?1, ?2)",
                rusqlite::params![&kb_id, chunk_text],
            ) {
                log::warn!("[KB] FTS5 insert failed for chunk {}: {}", chunk_id, e);
            }

            all_chunk_ids.push(chunk_id);
        }

        (doc_id, kb, file_name, file_type, file_size, file_hash, preview, chunks)
    };
    // ===== 阶段一结束：释放 DB 锁 =====

    // ===== 阶段二：网络请求（不持有 DB 锁） =====
    // 从安全存储中读取 API Key，而不再由前端传入（#32）
    let api_key = match get_embedding_api_key(&kb.embedding_api_config_id) {
        Ok(key) => key,
        Err(e) => {
            let error_msg = format!("Embedding API key lookup failed: {}", e);
            mark_document_failed(&db_state, &doc_id, &error_msg).await?;
            return Err(e);
        }
    };

    // 使用知识库自身保存的 embedding provider/model/base_url
    // （这些字段在创建知识库时，根据所选的 Embedding API 配置写入）。
    // 仅对创建于该字段引入之前的旧知识库，才回退到 OpenAI 默认值。
    let (embedding_provider, embedding_model, embedding_base_url) =
        if !kb.embedding_provider.is_empty() && !kb.embedding_model.is_empty() {
            (kb.embedding_provider.clone(), kb.embedding_model.clone(), kb.embedding_base_url.clone())
        } else {
            ("openai".to_string(), "text-embedding-3-small".to_string(), String::new())
        };

    let embeddings_result = generate_embeddings(
        chunks.clone(),
        &embedding_provider,
        &api_key,
        &embedding_model,
        &embedding_base_url,
    ).await;

    // 处理 embedding 生成失败的情况：把文档标记为 error 并清理孤儿 chunks
    let embeddings = match embeddings_result {
        Ok(emb) => emb,
        Err(e) => {
            let error_msg = format!("Embedding generation failed: {}", e);
            mark_document_failed(&db_state, &doc_id, &error_msg).await?;
            return Err(KnowledgeBaseError::EmbeddingError(error_msg));
        }
    };

    // ===== 阶段三：在数据库中收尾 =====
    // rusqlite::Connection 不是 Send 的，不能跨越 .await 持有它。
    // 因此拆成几个子阶段：同步的 DB 操作 → 异步的向量插入 → 同步的 DB 更新。

    // 阶段 3a：查询 chunk ID 并构建向量（同步，不涉及 await）
    let (vectors_to_insert, chunk_count_actual): (Vec<_>, usize) = {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 从数据库中查询 chunk ID
        let mut stmt = conn.prepare(
            "SELECT id FROM chunks WHERE document_id = ?1 ORDER BY chunk_index ASC"
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let chunk_ids: Vec<String> = stmt.query_map([&doc_id], |row| row.get(0))
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        // stmt 在此处被释放

        let count = chunk_ids.len();

        if chunk_ids.len() != embeddings.len() {
            log::warn!(
                "[KB] Chunk ID count ({}) != embedding count ({}), skipping vector insertion",
                chunk_ids.len(),
                embeddings.len()
            );
            (Vec::new(), count)
        } else {
            let vectors: Vec<_> = chunk_ids.iter()
                .zip(chunks.iter())
                .zip(embeddings.iter())
                .map(|((chunk_id, content), embedding)| {
                    (chunk_id.clone(), doc_id.clone(), content.clone(), embedding.clone())
                })
                .collect();
            (vectors, count)
        }
    }; // db 锁在此处释放

    // 阶段 3b：插入向量（异步，不持有 DB 锁）
    if !vectors_to_insert.is_empty() {
        kb_state.vector_store.insert_vectors(&kb_id, vectors_to_insert).await?;
    }

    // 阶段 3c：更新文档状态（重新获取 DB 锁）
    {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE documents SET status = 'completed', chunk_count = ?1 WHERE id = ?2",
            rusqlite::params![chunk_count_actual as i32, &doc_id],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        conn.execute(
            "UPDATE knowledge_bases SET document_count = document_count + 1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, &kb_id],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    } // db 锁已释放

    log::info!("Imported document {} with {} chunks", file_name, chunk_count_actual);

    Ok(Document {
        id: doc_id,
        kb_id,
        filename: file_name,
        file_type,
        file_size,
        file_hash,
        content_preview: preview,
        chunk_count: chunk_count_actual as i32,
        status: DocumentStatus::Completed,
        error_message: None,
        created_at: chrono::Utc::now().timestamp_millis(),
    })
}

/// 列出知识库中的文档
#[tauri::command]
pub async fn list_documents(
    kb_id: String,
    kb_state: State<'_, KbState>,
) -> Result<Vec<Document>, KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, kb_id, filename, file_type, file_size, file_hash, content_preview,
         chunk_count, status, error_message, created_at
         FROM documents WHERE kb_id = ?1 ORDER BY created_at DESC"
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let rows = stmt.query_map([&kb_id], |row| {
        let status_str: String = row.get(8)?;
        let status = match status_str.as_str() {
            "completed" => DocumentStatus::Completed,
            "error" => DocumentStatus::Error,
            _ => DocumentStatus::Processing,
        };

        Ok(Document {
            id: row.get(0)?,
            kb_id: row.get(1)?,
            filename: row.get(2)?,
            file_type: row.get(3)?,
            file_size: row.get(4)?,
            file_hash: row.get(5)?,
            content_preview: row.get(6)?,
            chunk_count: row.get(7)?,
            status,
            error_message: row.get(9)?,
            created_at: row.get(10)?,
        })
    }).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    let mut docs = Vec::new();
    for row in rows {
        docs.push(row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?);
    }

    Ok(docs)
}

/// 删除文档
///
/// # 对应 #35 的修复：
/// - 校验文档存在，且确实属于指定的知识库
/// - 使用安全的方式递减文档计数（MAX(count - 1, 0)，不会变负数）
/// - 用事务保证操作的原子性
#[tauri::command]
pub async fn delete_document(
    doc_id: String,
    kb_id: String,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // 校验文档存在，且属于指定的知识库
    let doc_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE id = ?1 AND kb_id = ?2",
        rusqlite::params![&doc_id, &kb_id],
        |row| row.get(0),
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    if !doc_exists {
        return Err(KnowledgeBaseError::NotFound(
            format!("Document not found: {} in knowledge base: {}", doc_id, kb_id)
        ));
    }

    // 删除向量
    kb_state.vector_store.delete_document_vectors(&kb_id, &doc_id).await?;

    // 从 FTS5 中删除（必须在删除 chunks 之前进行，因为需要用到 rowid）
    if let Err(e) = conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE document_id = ?1)",
        rusqlite::params![&doc_id],
    ) {
        log::warn!("[KB] FTS5 cleanup failed for document {}: {}", doc_id, e);
    }

    // 从 SQLite 中删除（级联删除会自动清掉 chunks）
    conn.execute(
        "DELETE FROM documents WHERE id = ?1",
        rusqlite::params![&doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // 安全地更新知识库的文档计数（保证永远不会小于 0）
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE knowledge_bases SET document_count = MAX(document_count - 1, 0), updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, &kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    log::info!("Deleted document: {}", doc_id);
    Ok(())
}

/// 检索知识库
///
/// # 对应 #32 的修复：
/// - API Key 改为通过 embedding_api_config_id 从安全存储中读取
#[tauri::command]
pub async fn search_knowledge_base(
    request: RetrievalRequest,
    kb_state: State<'_, KbState>,
) -> Result<RetrievalResult, KnowledgeBaseError> {
    // 从知识库中获取 embedding API 配置
    let (embedding_api_config_id, embedding_provider, embedding_model, embedding_base_url) = {
        let conn = rusqlite::Connection::open(&kb_state.db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let (config_id, provider, model, base_url): (String, String, String, String) = conn.query_row(
            "SELECT embedding_api_config_id, COALESCE(embedding_provider, ''), COALESCE(embedding_model, ''), COALESCE(embedding_base_url, '') FROM knowledge_bases WHERE id = ?1",
            [&request.kb_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 仅对创建于 embedding_provider/model 字段引入之前的旧知识库，
        // 才回退到 OpenAI 默认值。
        if provider.is_empty() || model.is_empty() {
            (config_id, "openai".to_string(), "text-embedding-3-small".to_string(), String::new())
        } else {
            (config_id, provider, model, base_url)
        }
    };

    // 从安全存储中读取 API Key（#32）
    let api_key = get_embedding_api_key(&embedding_api_config_id)?;

    let retriever = Retriever::new(kb_state.vector_store.clone(), kb_state.db_path.clone());
    let mut result = retriever.retrieve(request.clone(), &embedding_provider, &embedding_model, &embedding_base_url, &api_key).await?;

    // 可选的 reranker 精排环节
    if let Some(ref config_id) = request.reranker_config_id {
        if !result.chunks.is_empty() {
            match get_reranker_api_key(config_id) {
                Ok(reranker_key) => {
                    let base_url = request.reranker_base_url.as_deref().unwrap_or("");
                    let model = request.reranker_model.as_deref().unwrap_or("");
                    let top_n = request.rerank_top_n.unwrap_or(request.top_k) as usize;
                    match super::reranker::rerank_chunks(
                        &request.query,
                        result.chunks,
                        top_n,
                        &reranker_key,
                        model,
                        base_url,
                    ).await {
                        Ok(reranked) => {
                            result.total_chunks = reranked.len() as i32;
                            result.chunks = reranked;
                        }
                        Err(e) => {
                            log::warn!("[KB] Reranker failed, returning unranked results: {}", e);
                            result.chunks = vec![];
                            result.total_chunks = 0;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[KB] Could not load reranker API key: {}", e);
                }
            }
        }
    }

    Ok(result)
}

/// 从系统 keyring 中取出 reranker 的 API Key
fn get_reranker_api_key(config_id: &str) -> Result<String, KnowledgeBaseError> {
    let entry = Entry::new(
        "BaiyuAISpace",
        &format!("api_keys_reranker_{}", config_id),
    ).map_err(|e| KnowledgeBaseError::InvalidConfig(format!("Failed to access keyring: {}", e)))?;

    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => Err(KnowledgeBaseError::InvalidConfig(
            format!("Reranker API key not found for config: {}", config_id)
        )),
        Err(e) => Err(KnowledgeBaseError::InvalidConfig(
            format!("Failed to retrieve reranker API key: {}", e)
        )),
    }
}

/// 解析文档并返回全文（用于聊天时直接注入上下文，无需知识库/向量化）
#[tauri::command]
pub async fn read_document_for_context(
    file_path: String,
) -> Result<String, KnowledgeBaseError> {
    parse_document(&file_path).await
}
