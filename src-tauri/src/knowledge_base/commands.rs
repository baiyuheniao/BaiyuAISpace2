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

/// Initialize knowledge base tables
pub fn init_knowledge_base(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    init_sqlite_tables(conn)
}

/// Retrieve API key from system keyring using embedding config ID
/// The keyring entry format is: emb_{config_id}
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

/// Create a new knowledge base
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

    // Validate chunk_overlap < chunk_size
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
            1536i32,     // embedding_dim - default 1536
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

/// List all knowledge bases
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

/// Delete knowledge base
#[tauri::command]
pub async fn delete_knowledge_base(
    kb_id: String,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // Check if knowledge base exists
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_bases WHERE id = ?1",
        [&kb_id],
        |row| row.get(0),
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    if !exists {
        return Err(KnowledgeBaseError::NotFound(format!("Knowledge base not found: {}", kb_id)));
    }

    // Delete from SQLite (cascade will delete documents and chunks)
    conn.execute(
        "DELETE FROM knowledge_bases WHERE id = ?1",
        [&kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // Delete vector table
    kb_state.vector_store.drop_kb_table(&kb_id).await?;

    log::info!("Deleted knowledge base: {}", kb_id);
    Ok(())
}

/// Import document to knowledge base
///
/// # Fix for #33 and #34:
/// - Phase 1 (DB lock held): Read KB config, create document record, parse file, write chunks + FTS
/// - Phase 2 (DB lock released): Generate embeddings via network (no lock held)
/// - Phase 3 (DB lock re-acquired): Write vectors, update document status
/// - If Phase 2 fails, Phase 3 marks document as "error" and cleans up orphan chunks
///
/// # Fix for #32:
/// - API key is retrieved from secure storage (keyring) using embedding_api_config_id
/// - Frontend no longer passes api_key parameter
#[tauri::command]
pub async fn import_document(
    kb_id: String,
    file_path: String,
    db_state: State<'_, crate::db::DbState>,
    kb_state: State<'_, KbState>,
) -> Result<Document, KnowledgeBaseError> {
    // ===== Phase 1: Database operations (lock held) =====
    let (doc_id, kb, file_name, file_type, file_size, file_hash, preview, chunks) = {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // Get knowledge base config
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

        // Create document record
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

        // Get file size
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

        // Parse document
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

        // Store preview
        let preview: String = content.chars().take(500).collect();
        conn.execute(
            "UPDATE documents SET content_preview = ?1 WHERE id = ?2",
            rusqlite::params![&preview, &doc_id],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // Split into chunks
        let chunks = split_text(&content, kb.chunk_size as usize, kb.chunk_overlap as usize);

        // Write chunks to SQLite and FTS5
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

            // Insert into FTS5 - log error instead of ignoring
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
    // ===== Phase 1 END: DB lock released =====

    // ===== Phase 2: Network operation (no DB lock held) =====
    // Retrieve API key from secure storage instead of receiving from frontend (#32)
    let api_key = get_embedding_api_key(&kb.embedding_api_config_id)?;

    // Use the embedding provider/model/base_url stored on the knowledge base
    // itself (set at creation time from the selected Embedding API config).
    // Fall back to OpenAI defaults only for knowledge bases created before
    // this field was populated.
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

    // Handle embedding failure: mark document as error and clean up orphan chunks
    let embeddings = match embeddings_result {
        Ok(emb) => emb,
        Err(e) => {
            let error_msg = format!("Embedding generation failed: {}", e);
            log::error!("[KB] {}", error_msg);

            let db = db_state.0.lock().await;
            let conn = rusqlite::Connection::open(&db.path)
                .map_err(|e2| KnowledgeBaseError::DatabaseError(e2.to_string()))?;

            conn.execute(
                "UPDATE documents SET status = 'error', error_message = ?1 WHERE id = ?2",
                rusqlite::params![&error_msg, &doc_id],
            ).map_err(|e2| KnowledgeBaseError::DatabaseError(e2.to_string()))?;

            // Clean up FTS5 entries BEFORE deleting chunks (need rowid from chunks)
            if let Err(cleanup_err) = conn.execute(
                "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE document_id = ?1)",
                rusqlite::params![&doc_id],
            ) {
                log::warn!("[KB] Failed to clean up FTS5 entries: {}", cleanup_err);
            }

            // Clean up orphan chunks (after FTS5 cleanup)
            if let Err(cleanup_err) = conn.execute(
                "DELETE FROM chunks WHERE document_id = ?1",
                rusqlite::params![&doc_id],
            ) {
                log::warn!("[KB] Failed to clean up orphan chunks: {}", cleanup_err);
            }

            return Err(KnowledgeBaseError::EmbeddingError(error_msg));
        }
    };

    // ===== Phase 3: Finalize in DB =====
    // rusqlite::Connection is not Send, so we cannot hold it across .await points.
    // Split into sub-phases: sync DB work → async vector insert → sync DB update.

    // Phase 3a: Query chunk IDs and build vectors (synchronous, no await)
    let (vectors_to_insert, chunk_count_actual): (Vec<_>, usize) = {
        let db = db_state.0.lock().await;
        let conn = rusqlite::Connection::open(&db.path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // Query chunk IDs from DB
        let mut stmt = conn.prepare(
            "SELECT id FROM chunks WHERE document_id = ?1 ORDER BY chunk_index ASC"
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let chunk_ids: Vec<String> = stmt.query_map([&doc_id], |row| row.get(0))
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        // stmt is dropped here

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
    }; // db lock released here

    // Phase 3b: Insert vectors (async, no DB lock held)
    if !vectors_to_insert.is_empty() {
        kb_state.vector_store.insert_vectors(&kb_id, vectors_to_insert).await?;
    }

    // Phase 3c: Update document status (re-acquire DB lock)
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
    } // db lock released

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

/// List documents in knowledge base
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

/// Delete document
///
/// # Fix for #35:
/// - Verify document exists and belongs to the specified knowledge base
/// - Use safe document count decrement (MAX(count - 1, 0))
/// - Use transaction for atomicity
#[tauri::command]
pub async fn delete_document(
    doc_id: String,
    kb_id: String,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let conn = rusqlite::Connection::open(&kb_state.db_path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // Verify document exists and belongs to the specified knowledge base
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

    // Delete vectors
    kb_state.vector_store.delete_document_vectors(&kb_id, &doc_id).await?;

    // Delete from FTS5 (must delete before deleting chunks since we need rowid)
    if let Err(e) = conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE document_id = ?1)",
        rusqlite::params![&doc_id],
    ) {
        log::warn!("[KB] FTS5 cleanup failed for document {}: {}", doc_id, e);
    }

    // Delete from SQLite (cascade will delete chunks)
    conn.execute(
        "DELETE FROM documents WHERE id = ?1",
        rusqlite::params![&doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    // Update knowledge base document count safely (never go below 0)
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE knowledge_bases SET document_count = MAX(document_count - 1, 0), updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, &kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

    log::info!("Deleted document: {}", doc_id);
    Ok(())
}

/// Search knowledge base
///
/// # Fix for #32:
/// - API key is retrieved from secure storage using embedding_api_config_id
#[tauri::command]
pub async fn search_knowledge_base(
    request: RetrievalRequest,
    kb_state: State<'_, KbState>,
) -> Result<RetrievalResult, KnowledgeBaseError> {
    // Get embedding API config from the knowledge base
    let (embedding_api_config_id, embedding_provider, embedding_model, embedding_base_url) = {
        let conn = rusqlite::Connection::open(&kb_state.db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let (config_id, provider, model, base_url): (String, String, String, String) = conn.query_row(
            "SELECT embedding_api_config_id, COALESCE(embedding_provider, ''), COALESCE(embedding_model, ''), COALESCE(embedding_base_url, '') FROM knowledge_bases WHERE id = ?1",
            [&request.kb_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // Fall back to OpenAI defaults only for knowledge bases created
        // before embedding_provider/model were populated.
        if provider.is_empty() || model.is_empty() {
            (config_id, "openai".to_string(), "text-embedding-3-small".to_string(), String::new())
        } else {
            (config_id, provider, model, base_url)
        }
    };

    // Retrieve API key from secure storage (#32)
    let api_key = get_embedding_api_key(&embedding_api_config_id)?;

    let retriever = Retriever::new(kb_state.vector_store.clone(), kb_state.db_path.clone());
    let mut result = retriever.retrieve(request.clone(), &embedding_provider, &embedding_model, &embedding_base_url, &api_key).await?;

    // Optional reranker pass
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

/// Retrieve reranker API key from system keyring
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

/// Get available embedding models
#[tauri::command]
pub fn get_embedding_models() -> Vec<(String, String, i32)> {
    super::embedding::get_available_embedding_models()
}

/// 解析文档并返回全文（用于聊天时直接注入上下文，无需知识库/向量化）
#[tauri::command]
pub async fn read_document_for_context(
    file_path: String,
) -> Result<String, KnowledgeBaseError> {
    parse_document(&file_path).await
}
