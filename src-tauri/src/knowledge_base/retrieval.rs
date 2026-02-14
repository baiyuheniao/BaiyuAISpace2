// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use super::db::VectorStore;
use super::embedding::generate_single_embedding;
use std::sync::Arc;

pub struct Retriever {
    vector_store: Arc<VectorStore>,
    db_path: String,
}

impl Retriever {
    pub fn new(vector_store: Arc<VectorStore>, db_path: String) -> Self {
        Self { vector_store, db_path }
    }

    /// Retrieve relevant chunks
    pub async fn retrieve(
        &self,
        request: RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        match request.retrieval_mode {
            RetrievalMode::Vector => {
                self.vector_search(&request, embedding_provider, embedding_model, api_key).await
            }
            RetrievalMode::Keyword => {
                self.keyword_search(&request).await
            }
            RetrievalMode::Hybrid => {
                self.hybrid_search(&request, embedding_provider, embedding_model, api_key).await
            }
        }
    }

    /// Pure vector similarity search
    async fn vector_search(
        &self,
        request: &RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // Generate query embedding using provided embedding config
        let query_vector = generate_single_embedding(
            &request.query, 
            embedding_provider, 
            api_key, 
            embedding_model
        ).await?;
        
        // Search vector store
        let results = self.vector_store
            .search(&request.kb_id, query_vector, request.top_k)
            .await?;
        
        // Convert to RetrievedChunk with full metadata
        let chunks = self.enrich_chunks(results, &request.kb_id).await?;
        
        // Filter by similarity threshold
        let filtered_chunks: Vec<_> = chunks
            .into_iter()
            .filter(|c| c.score >= request.similarity_threshold)
            .collect();

        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: filtered_chunks.len() as i32,
            chunks: filtered_chunks,
        })
    }

    /// Pure keyword search using SQLite FTS or LIKE
    async fn keyword_search(
        &self,
        request: &RetrievalRequest,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = request.kb_id.clone();
        let query = request.query.clone();
        let top_k = request.top_k;
        
        // Run SQLite operations in blocking task
        let chunks = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            
            // Try FTS5 first, fallback to LIKE query
            Self::search_with_fts_blocking(&conn, &kb_id, &query, top_k)
                .or_else(|_| Self::search_with_like_blocking(&conn, &kb_id, &query, top_k))
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))??;

        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: chunks.len() as i32,
            chunks,
        })
    }

    /// Hybrid search: combine vector and keyword
    async fn hybrid_search(
        &self,
        request: &RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // Get results from both methods with larger top_k for better fusion
        let mut vector_request = request.clone();
        vector_request.top_k = request.top_k * 2;
        
        let mut keyword_request = request.clone();
        keyword_request.top_k = request.top_k * 2;
        
        let vector_result = self.vector_search(&vector_request, embedding_provider, embedding_model, api_key).await?;
        let keyword_result = self.keyword_search(&keyword_request).await?;
        
        // Merge and rerank using RRF
        let merged = self.merge_results(
            vector_result.chunks,
            keyword_result.chunks,
            request.top_k,
        );
        
        // Filter by similarity threshold
        let filtered: Vec<_> = merged
            .into_iter()
            .filter(|c| c.score >= request.similarity_threshold)
            .collect();

        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: filtered.len() as i32,
            chunks: filtered,
        })
    }

    /// Get knowledge base configuration from database
    async fn get_knowledge_base(&self, kb_id: &str) -> Result<KnowledgeBase, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            
            conn.query_row(
                "SELECT id, name, description, embedding_api_config_id, 
                 chunk_size, chunk_overlap, created_at, updated_at, document_count 
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
                    })
                }
            ).map_err(|e| KnowledgeBaseError::NotFound(format!("Knowledge base not found: {}", e)))
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
    }

    /// Enrich chunk results with metadata from SQLite
    async fn enrich_chunks(
        &self,
        results: Vec<(String, String, String, f32)>, // (chunk_id, doc_id, content, score)
        kb_id: &str,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            
            let mut chunks = Vec::new();
            
            for (chunk_id, doc_id, content, score) in results {
                // Get chunk metadata
                let chunk_result: Result<(i32, i32), _> = conn.query_row(
                    "SELECT chunk_index, token_count FROM chunks WHERE id = ?1",
                    [&chunk_id],
                    |row| Ok((row.get(0)?, row.get(1)?))
                );
                
                let (chunk_index, token_count) = chunk_result.unwrap_or((0, 0));
                
                // Get document filename
                let filename: String = conn.query_row(
                    "SELECT filename FROM documents WHERE id = ?1",
                    [&doc_id],
                    |row| row.get(0)
                ).unwrap_or_else(|_| "Unknown".to_string());
                
                chunks.push(RetrievedChunk {
                    chunk: Chunk {
                        id: chunk_id,
                        document_id: doc_id.clone(),
                        kb_id: kb_id.clone(),
                        content,
                        chunk_index,
                        token_count,
                    },
                    score,
                    vector_score: Some(score),
                    keyword_score: None,
                    document_filename: filename,
                });
            }
            
            Ok(chunks)
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
    }

    /// Search using FTS5 (Full-Text Search) - blocking version
    fn search_with_fts_blocking(
        conn: &rusqlite::Connection,
        kb_id: &str,
        query: &str,
        top_k: i32,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        // Check if FTS table exists
        let fts_exists: bool = conn.query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
            [],
            |_| Ok(true)
        ).unwrap_or(false);
        
        if !fts_exists {
            return Err(KnowledgeBaseError::RetrievalError("FTS5 not available".to_string()));
        }
        
        // Build FTS query (convert query to OR-separated terms)
        let fts_query = query.split_whitespace().collect::<Vec<_>>().join(" OR ");
        
        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.token_count, d.filename,
                   rank
            FROM chunks_fts fts
            JOIN chunks c ON fts.rowid = c.rowid
            JOIN documents d ON c.document_id = d.id
            WHERE c.kb_id = ?1 AND fts MATCH ?2
            ORDER BY rank
            LIMIT ?3
            "#
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        let rows = stmt.query_map(
            [kb_id, &fts_query, &top_k.to_string()],
            |row| {
                Ok(RetrievedChunk {
                    chunk: Chunk {
                        id: row.get(0)?,
                        document_id: row.get(1)?,
                        kb_id: kb_id.to_string(),
                        content: row.get(2)?,
                        chunk_index: row.get(3)?,
                        token_count: row.get(4)?,
                    },
                    score: 1.0, // FTS doesn't give 0-1 scores directly
                    vector_score: None,
                    keyword_score: Some(1.0),
                    document_filename: row.get(5)?,
                })
            }
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?);
        }
        
        Ok(chunks)
    }

    /// Search using LIKE query (fallback for when FTS is not available) - blocking version
    fn search_with_like_blocking(
        conn: &rusqlite::Connection,
        kb_id: &str,
        query: &str,
        top_k: i32,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        // Build LIKE pattern with wildcards
        let pattern = format!("%{}%", query.split_whitespace().collect::<Vec<_>>().join("%"));
        
        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.token_count, d.filename
            FROM chunks c
            JOIN documents d ON c.document_id = d.id
            WHERE c.kb_id = ?1 AND c.content LIKE ?2
            LIMIT ?3
            "#
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        let rows = stmt.query_map(
            [kb_id, &pattern, &top_k.to_string()],
            |row| {
                Ok(RetrievedChunk {
                    chunk: Chunk {
                        id: row.get(0)?,
                        document_id: row.get(1)?,
                        kb_id: kb_id.to_string(),
                        content: row.get(2)?,
                        chunk_index: row.get(3)?,
                        token_count: row.get(4)?,
                    },
                    score: 0.5, // LIKE doesn't give proper scores
                    vector_score: None,
                    keyword_score: Some(0.5),
                    document_filename: row.get(5)?,
                })
            }
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?);
        }
        
        Ok(chunks)
    }

    /// Merge vector and keyword results using RRF (Reciprocal Rank Fusion)
    fn merge_results(
        &self,
        vector_chunks: Vec<RetrievedChunk>,
        keyword_chunks: Vec<RetrievedChunk>,
        top_k: i32,
    ) -> Vec<RetrievedChunk> {
        let k = 60.0; // RRF constant
        let mut scores: std::collections::HashMap<String, (RetrievedChunk, f32)> = std::collections::HashMap::new();
        
        // Add vector scores
        for (rank, chunk) in vector_chunks.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32);
            scores.entry(chunk.chunk.id.clone())
                .and_modify(|(_, score)| *score += rrf_score)
                .or_insert_with(|| {
                    let mut c = chunk.clone();
                    c.vector_score = chunk.vector_score.or(Some(chunk.score));
                    (c, rrf_score)
                });
        }
        
        // Add keyword scores
        for (rank, chunk) in keyword_chunks.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32);
            scores.entry(chunk.chunk.id.clone())
                .and_modify(|(c, score)| {
                    *score += rrf_score;
                    c.keyword_score = chunk.keyword_score.or(Some(chunk.score));
                })
                .or_insert_with(|| {
                    let mut c = chunk.clone();
                    c.keyword_score = chunk.keyword_score.or(Some(chunk.score));
                    (c, rrf_score)
                });
        }
        
        // Sort by RRF score and take top_k
        let mut results: Vec<_> = scores.into_iter()
            .map(|(_, (mut chunk, score))| {
                chunk.score = score;
                chunk
            })
            .collect();
        
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(top_k as usize);
        
        results
    }
}

/// Build context for LLM from retrieved chunks
pub fn build_context(chunks: &[RetrievedChunk], query: &str) -> String {
    if chunks.is_empty() {
        return query.to_string();
    }
    
    let mut context_parts = vec![
        "基于以下参考文档回答问题：".to_string(),
        String::new(),
    ];
    
    for (i, chunk) in chunks.iter().enumerate() {
        context_parts.push(format!(
            "[文档 {}: {}]\n{}",
            i + 1,
            chunk.document_filename,
            chunk.chunk.content
        ));
        context_parts.push(String::new());
    }
    
    context_parts.push("---".to_string());
    context_parts.push(String::new());
    context_parts.push(format!("问题：{}", query));
    
    context_parts.join("\n")
}
