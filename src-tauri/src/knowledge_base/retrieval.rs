// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use super::db::VectorStore;
use super::embedding::generate_single_embedding;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Retriever {
    vector_store: Arc<VectorStore>,
}

impl Retriever {
    pub fn new(vector_store: Arc<VectorStore>) -> Self {
        Self { vector_store }
    }

    /// Retrieve relevant chunks
    pub async fn retrieve(
        &self,
        request: RetrievalRequest,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        let query = request.query.clone();
        let kb_id = request.kb_id.clone();
        
        match request.retrieval_mode {
            RetrievalMode::Vector => {
                self.vector_search(&request, api_key).await
            }
            RetrievalMode::Keyword => {
                self.keyword_search(&request).await
            }
            RetrievalMode::Hybrid => {
                self.hybrid_search(&request, api_key).await
            }
        }
    }

    /// Pure vector similarity search
    async fn vector_search(
        &self,
        request: &RetrievalRequest,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // Get knowledge base config from SQLite to get provider/model
        // For now, we'll need to pass these in or fetch them
        // This is a simplified implementation
        
        // TODO: Get embedding config from knowledge base
        let provider = "openai"; // Should be fetched from kb config
        let model = "text-embedding-3-small";
        
        // Generate query embedding
        let query_vector = generate_single_embedding(&request.query, provider, api_key, model).await?;
        
        // Search vector store
        let results = self.vector_store
            .search(&request.kb_id, query_vector, request.top_k)
            .await?;
        
        // Convert to RetrievedChunk
        let chunks: Vec<RetrievedChunk> = results.into_iter()
            .map(|(chunk_id, doc_id, content, score)| {
                RetrievedChunk {
                    chunk: Chunk {
                        id: chunk_id.clone(),
                        document_id: doc_id.clone(),
                        kb_id: request.kb_id.clone(),
                        content,
                        chunk_index: 0, // Would be fetched from SQLite
                        token_count: 0,
                    },
                    score,
                    vector_score: Some(score),
                    keyword_score: None,
                    document_filename: String::new(), // Would be fetched from SQLite
                }
            })
            .collect();
        
        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: chunks.len() as i32,
            chunks,
        })
    }

    /// Pure keyword search (BM25)
    async fn keyword_search(
        &self,
        request: &RetrievalRequest,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // For keyword search, we need to query SQLite FTS or do basic LIKE queries
        // LanceDB also supports full-text search
        
        // TODO: Implement FTS using SQLite FTS5 or LanceDB FTS
        // For now, return empty results
        
        log::warn!("Keyword search not yet fully implemented");
        
        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: 0,
            chunks: vec![],
        })
    }

    /// Hybrid search: combine vector and keyword
    async fn hybrid_search(
        &self,
        request: &RetrievalRequest,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // Get results from both methods
        let vector_result = self.vector_search(request, api_key).await?;
        let keyword_result = self.keyword_search(request).await?;
        
        // Merge and rerank
        let merged = self.merge_results(
            vector_result.chunks,
            keyword_result.chunks,
            request.top_k,
        );
        
        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: merged.len() as i32,
            chunks: merged,
        })
    }

    /// Merge vector and keyword results using RRF (Reciprocal Rank Fusion)
    fn merge_results(
        &self,
        vector_chunks: Vec<RetrievedChunk>,
        keyword_chunks: Vec<RetrievedChunk>,
        top_k: i32,
    ) -> Vec<RetrievedChunk> {
        use std::collections::HashMap;
        
        let k = 60.0; // RRF constant
        let mut scores: HashMap<String, (RetrievedChunk, f32)> = HashMap::new();
        
        // Add vector scores
        for (rank, chunk) in vector_chunks.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32);
            scores.entry(chunk.chunk.id.clone())
                .and_modify(|(_, score)| *score += rrf_score)
                .or_insert_with(|| (chunk.clone(), rrf_score));
        }
        
        // Add keyword scores
        for (rank, chunk) in keyword_chunks.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32);
            scores.entry(chunk.chunk.id.clone())
                .and_modify(|(c, score)| {
                    *score += rrf_score;
                    c.keyword_score = chunk.keyword_score;
                })
                .or_insert_with(|| (chunk.clone(), rrf_score));
        }
        
        // Sort by RRF score and take top_k
        let mut results: Vec<_> = scores.into_iter()
            .map(|(_, (chunk, score))| {
                let mut c = chunk;
                c.score = score;
                c
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
