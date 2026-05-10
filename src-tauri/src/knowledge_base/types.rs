// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 知识库类型定义模块
 * 
 * 包含知识库、文档、文本块等核心数据结构的定义
 */

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 知识库错误类型
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum KnowledgeBaseError {
    /// 数据库错误
    #[error("Database error: {0}")]
    DatabaseError(String),
    /// 嵌入错误
    #[error("Embedding error: {0}")]
    EmbeddingError(String),
    /// 文档解析错误
    #[error("Document parse error: {0}")]
    DocumentParseError(String),
    /// 检索错误
    #[error("Retrieval error: {0}")]
    RetrievalError(String),
    /// 未找到
    #[error("Knowledge base not found: {0}")]
    NotFound(String),
    /// 配置无效
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// 实现 Serialize trait 用于 Tauri 命令返回
impl Serialize for KnowledgeBaseError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// 知识库配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBase {
    /// 知识库 ID
    pub id: String,
    /// 知识库名称
    pub name: String,
    /// 知识库描述
    pub description: String,
    /// 关联的 Embedding API 配置 ID
    pub embedding_api_config_id: String,
    /// 文本分块大小 (token 数)
    pub chunk_size: i32,
    /// 分块重叠大小
    pub chunk_overlap: i32,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
    /// 文档数量
    pub document_count: i32,
}

/// 文档元数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// 文档 ID
    pub id: String,
    /// 所属知识库 ID
    pub kb_id: String,
    /// 文件名
    pub filename: String,
    /// 文件类型 (pdf, docx, xlsx, md, html, txt)
    pub file_type: String,
    /// 文件大小 (字节)
    pub file_size: i64,
    /// 文件内容哈希 (用于去重)
    pub file_hash: String,
    /// 内容预览 (前 200 字符)
    pub content_preview: String,
    /// 分块数量
    pub chunk_count: i32,
    /// 处理状态
    pub status: DocumentStatus,
    /// 错误信息 (如果有)
    pub error_message: Option<String>,
    /// 创建时间戳
    pub created_at: i64,
}

/// 文档处理状态枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentStatus {
    /// 处理中
    Processing,
    /// 完成
    Completed,
    /// 错误
    Error,
}

/// 文本块结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// 分块 ID
    pub id: String,
    /// 所属文档 ID
    pub document_id: String,
    /// 所属知识库 ID
    pub kb_id: String,
    /// 分块内容
    pub content: String,
    /// 分块索引
    pub chunk_index: i32,
    /// Token 数量
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
