// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[allow(dead_code)]
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

/// 知识库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub embedding_api_config_id: String,  // 指向全局 embedding 配置的引用
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_base_url: String,
    pub chunk_size: i32,
    pub chunk_overlap: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub document_count: i32,
}

/// 文档元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub kb_id: String,
    pub filename: String,
    pub file_type: String,  // pdf、docx、xlsx、md、html、txt
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

/// 带元数据的文本块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub kb_id: String,
    pub content: String,
    pub chunk_index: i32,
    pub token_count: i32,
}

/// 检索请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalRequest {
    pub kb_id: String,
    pub query: String,
    pub top_k: i32,
    pub retrieval_mode: RetrievalMode,
    pub similarity_threshold: f32,
    /// 句子窗口大小：为命中的 chunk 左右各取这么多个相邻 chunk 并拼接起来，
    /// 作为提供给 LLM 的扩展上下文。
    /// 0 = 禁用（默认值）。向后兼容：字段缺失时反序列化为 0。
    #[serde(default)]
    pub window_size: i32,
    /// Reranker 配置 ID（用于从 keyring 查找）。设置后，初次检索的结果会经过一个
    /// 兼容 Cohere 接口的 reranker API 重新排序。
    #[serde(default)]
    pub reranker_config_id: Option<String>,
    /// Reranker 的 base URL（例如 "https://api.cohere.com"）
    #[serde(default)]
    pub reranker_base_url: Option<String>,
    /// Reranker 模型名称（例如 "rerank-multilingual-v3.0"）
    #[serde(default)]
    pub reranker_model: Option<String>,
    /// 精排后保留的 chunk 数量。缺省时默认为 top_k。
    #[serde(default)]
    pub rerank_top_n: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    Vector,      // 纯向量相似度
    Keyword,     // 纯关键词检索
    Hybrid,      // 向量 + 关键词（默认）
}

/// 带分数的检索结果块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub chunk: Chunk,
    pub score: f32,
    pub vector_score: Option<f32>,
    pub keyword_score: Option<f32>,
    pub document_filename: String,
}

/// 检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub query: String,
    pub chunks: Vec<RetrievedChunk>,
    pub total_chunks: i32,
}

/// 创建知识库的请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKnowledgeBaseRequest {
    pub name: String,
    pub description: String,
    pub embedding_api_config_id: String,
    pub embedding_provider: String,
    pub embedding_model: String,
    pub embedding_base_url: String,
    pub chunk_size: Option<i32>,     // 默认：1000
    pub chunk_overlap: Option<i32>,  // 默认：200
}

impl Default for RetrievalMode {
    fn default() -> Self {
        RetrievalMode::Hybrid
    }
}
