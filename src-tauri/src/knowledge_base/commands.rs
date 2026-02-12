// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use super::document::{parse_document, calculate_file_hash, split_text, estimate_tokens};
use super::embedding::{generate_embeddings, get_embedding_dimension};
use super::db::{VectorStore, init_sqlite_tables};
use super::retrieval::Retriever;
use tauri::State;
use std::sync::Arc;

use uuid::Uuid;

pub struct KbState {
    pub vector_store: Arc<VectorStore>,
    pub db_path: String,
}

/// Initialize knowledge base tables
pub fn init_knowledge_base(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    init_sqlite_tables(conn)
}

/// Create a new knowledge base
#[tauri::command]
pub async fn create_knowledge_base(
    request: CreateKnowledgeBaseRequest,
    db_state: State<'_, crate::db::DbState>,
) -> Result<KnowledgeBase, KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let chunk_size = request.chunk_size.unwrap_or(1000);
    let chunk_overlap = request.chunk_overlap.unwrap_or(200);
    let embedding_dim = get_embedding_dimension(&request.embedding_provider, &request.embedding_model);
    
    conn.execute(
        r#"
        INSERT INTO knowledge_bases 
        (id, name, description, embedding_provider, embedding_model, embedding_dim, 
         chunk_size, chunk_overlap, created_at, updated_at, document_count)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)
        "#,
        [
            &id,
            &request.name,
            &request.description,
            &request.embedding_provider,
            &request.embedding_model,
            &embedding_dim.to_string(),
            &chunk_size.to_string(),
            &chunk_overlap.to_string(),
            &now.to_string(),
            &now.to_string(),
        ],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    log::info!("Created knowledge base: {} ({})", request.name, id);
    
    Ok(KnowledgeBase {
        id,
        name: request.name,
        description: request.description,
        embedding_provider: request.embedding_provider,
        embedding_model: request.embedding_model,
        embedding_dim,
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
    db_state: State<'_, crate::db::DbState>,
) -> Result<Vec<KnowledgeBase>, KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    let mut stmt = conn.prepare(
        "SELECT id, name, description, embedding_provider, embedding_model, embedding_dim, 
         chunk_size, chunk_overlap, created_at, updated_at, document_count 
         FROM knowledge_bases ORDER BY updated_at DESC"
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    let rows = stmt.query_map([], |row| {
        Ok(KnowledgeBase {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            embedding_provider: row.get(3)?,
            embedding_model: row.get(4)?,
            embedding_dim: row.get(5)?,
            chunk_size: row.get(6)?,
            chunk_overlap: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            document_count: row.get(10)?,
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
    db_state: State<'_, crate::db::DbState>,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
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
#[tauri::command]
pub async fn import_document(
    kb_id: String,
    file_path: String,
    api_key: String,
    db_state: State<'_, crate::db::DbState>,
    kb_state: State<'_, KbState>,
) -> Result<Document, KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Get knowledge base config
    let kb: KnowledgeBase = conn.query_row(
        "SELECT id, name, description, embedding_provider, embedding_model, embedding_dim, 
         chunk_size, chunk_overlap, created_at, updated_at, document_count 
         FROM knowledge_bases WHERE id = ?1",
        [&kb_id],
        |row| {
            Ok(KnowledgeBase {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                embedding_provider: row.get(3)?,
                embedding_model: row.get(4)?,
                embedding_dim: row.get(5)?,
                chunk_size: row.get(6)?,
                chunk_overlap: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                document_count: row.get(10)?,
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
    let file_size = tokio::fs::metadata(&file_path)
        .await
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    
    conn.execute(
        r#"
        INSERT INTO documents 
        (id, kb_id, filename, file_type, file_size, file_hash, content_preview, 
         chunk_count, status, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', 0, 'processing', ?7)
        "#,
        [
            &doc_id,
            &kb_id,
            &file_name,
            &file_type,
            &file_size.to_string(),
            &file_hash,
            &now.to_string(),
        ],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Parse document
    let content = match parse_document(&file_path).await {
        Ok(c) => c,
        Err(e) => {
            // Update status to error
            conn.execute(
                "UPDATE documents SET status = 'error', error_message = ?1 WHERE id = ?2",
                [&e.to_string(), &doc_id],
            ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            return Err(e);
        }
    };
    
    // Store preview
    let preview: String = content.chars().take(500).collect();
    conn.execute(
        "UPDATE documents SET content_preview = ?1 WHERE id = ?2",
        [&preview, &doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Split into chunks
    let chunks = split_text(&content, kb.chunk_size as usize, kb.chunk_overlap as usize);
    let chunk_count = chunks.len() as i32;
    
    // Generate embeddings in batches
    let mut all_chunk_ids = Vec::new();
    let _all_embeddings: Vec<Vec<f32>> = Vec::new();
    
    for (i, chunk_text) in chunks.iter().enumerate() {
        let chunk_id = Uuid::new_v4().to_string();
        let tokens = estimate_tokens(chunk_text);
        
        // Store chunk in SQLite
        conn.execute(
            r#"
            INSERT INTO chunks (id, document_id, kb_id, content, chunk_index, token_count, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            [
                &chunk_id,
                &doc_id,
                &kb_id,
                chunk_text,
                &(i as i32).to_string(),
                &tokens.to_string(),
                &now.to_string(),
            ],
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        // Also insert into FTS5 for keyword search (ignore errors if FTS5 not available)
        let _ = conn.execute(
            "INSERT INTO chunks_fts (rowid, content) VALUES (last_insert_rowid(), ?1)",
            [chunk_text],
        );
        
        all_chunk_ids.push(chunk_id);
    }
    
    // Generate embeddings for all chunks
    let embeddings = generate_embeddings(
        chunks.clone(),
        &kb.embedding_provider,
        &api_key,
        &kb.embedding_model,
    ).await?;
    
    // Prepare vectors for insertion
    let vectors: Vec<_> = all_chunk_ids.iter()
        .zip(chunks.iter())
        .zip(embeddings.iter())
        .map(|((chunk_id, content), embedding)| {
            (chunk_id.clone(), doc_id.clone(), content.clone(), embedding.clone())
        })
        .collect();
    
    // Insert into vector store
    kb_state.vector_store.insert_vectors(&kb_id, vectors).await?;
    
    // Update document status
    conn.execute(
        "UPDATE documents SET status = 'completed', chunk_count = ?1 WHERE id = ?2",
        [&chunk_count.to_string(), &doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Update knowledge base document count
    conn.execute(
        "UPDATE knowledge_bases SET document_count = document_count + 1, updated_at = ?1 WHERE id = ?2",
        [&now.to_string(), &kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    log::info!("Imported document {} with {} chunks", file_name, chunk_count);
    
    Ok(Document {
        id: doc_id,
        kb_id,
        filename: file_name,
        file_type,
        file_size,
        file_hash,
        content_preview: preview,
        chunk_count,
        status: DocumentStatus::Completed,
        error_message: None,
        created_at: now,
    })
}

/// List documents in knowledge base
#[tauri::command]
pub async fn list_documents(
    kb_id: String,
    db_state: State<'_, crate::db::DbState>,
) -> Result<Vec<Document>, KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
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
#[tauri::command]
pub async fn delete_document(
    doc_id: String,
    kb_id: String,
    db_state: State<'_, crate::db::DbState>,
    kb_state: State<'_, KbState>,
) -> Result<(), KnowledgeBaseError> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path)
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Delete vectors
    kb_state.vector_store.delete_document_vectors(&kb_id, &doc_id).await?;
    
    // Delete from FTS5 (must delete before deleting chunks since we need rowid)
    let _ = conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT rowid FROM chunks WHERE document_id = ?1)",
        [&doc_id],
    );
    
    // Delete from SQLite (cascade will delete chunks)
    conn.execute(
        "DELETE FROM documents WHERE id = ?1",
        [&doc_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    // Update knowledge base document count
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE knowledge_bases SET document_count = document_count - 1, updated_at = ?1 WHERE id = ?2",
        [&now.to_string(), &kb_id],
    ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
    
    log::info!("Deleted document: {}", doc_id);
    Ok(())
}

/// Search knowledge base
#[tauri::command]
pub async fn search_knowledge_base(
    request: RetrievalRequest,
    api_key: String,
    kb_state: State<'_, KbState>,
) -> Result<RetrievalResult, KnowledgeBaseError> {
    let retriever = Retriever::new(kb_state.vector_store.clone(), kb_state.db_path.clone());
    retriever.retrieve(request, &api_key).await
}

/// Get available embedding models
#[tauri::command]
pub fn get_embedding_models() -> Vec<(String, String, i32)> {
    super::embedding::get_available_embedding_models()
}
