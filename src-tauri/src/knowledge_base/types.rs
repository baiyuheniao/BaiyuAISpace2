// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KnowledgeBaseError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Embedding error: {0}")]
    EmbeddingError(String),
    #[error("Document parse error: {0}")]
    DocumentParseError(String),
    #[error("Retrieval error: {0}")]
    RetrievalError(String),
    #[error("Knowledge base not found: {0}")]
    NotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl Serialize for KnowledgeBaseError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// Knowledge base configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub embedding_api_config_id: String,  // Reference to global embedding config
    pub chunk_size: i32,
    pub chunk_overlap: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub document_count: i32,
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub kb_id: String,
    pub filename: String,
    pub file_type: String,  // pdf, docx, xlsx, md, html, txt
    pub file_size: i64,
    pub file_hash: String,
    pub content_preview: String,
    pub chunk_count: i32,
    pub status: DocumentStatus,
    pub error_message: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    Processing,
    Completed,
    Error,
}

/// Text chunk with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub kb_id: String,
    pub content: String,
    pub chunk_index: i32,
    pub token_count: i32,
}

/// Retrieval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalRequest {
    pub kb_id: String,
    pub query: String,
    pub top_k: i32,
    pub retrieval_mode: RetrievalMode,
    pub similarity_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Vector,      // Pure vector similarity
    Keyword,     // Pure keyword search
    Hybrid,      // Vector + keyword (default)
}

/// Retrieved chunk with score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub chunk: Chunk,
    pub score: f32,
    pub vector_score: Option<f32>,
    pub keyword_score: Option<f32>,
    pub document_filename: String,
}

/// Retrieval result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub query: String,
    pub chunks: Vec<RetrievedChunk>,
    pub total_chunks: i32,
}

/// Create knowledge base request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKnowledgeBaseRequest {
    pub name: String,
    pub description: String,
    pub embedding_api_config_id: String,
    pub chunk_size: Option<i32>,     // default: 1000
    pub chunk_overlap: Option<i32>,  // default: 200
}

impl Default for RetrievalMode {
    fn default() -> Self {
        RetrievalMode::Hybrid
    }
}
