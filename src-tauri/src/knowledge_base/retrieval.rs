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

    /// 检索出相关 chunk，然后可选地为每条结果扩展句子窗口上下文
    /// （来自同一文档的相邻 chunk）。
    pub async fn retrieve(
        &self,
        request: RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        embedding_base_url: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        let window_size = request.window_size;

        let mut result = match request.retrieval_mode {
            RetrievalMode::Vector => {
                self.vector_search(&request, embedding_provider, embedding_model, embedding_base_url, api_key).await
            }
            RetrievalMode::Keyword => {
                self.keyword_search(&request).await
            }
            RetrievalMode::Hybrid => {
                self.hybrid_search(&request, embedding_provider, embedding_model, embedding_base_url, api_key).await
            }
        }?;

        if window_size > 0 && !result.chunks.is_empty() {
            result.chunks = self.expand_windows(result.chunks, window_size).await?;
        }

        Ok(result)
    }

    /// 为每个检索到的 chunk 扩展最多 `window` 个相邻 chunk（左右各取，同一文档内，
    /// 按 chunk_index 排序）。命中 chunk 的内容会被替换为拼接后的窗口内容，
    /// 让 LLM 获得更丰富的上下文，同时不影响任何分数或排名。
    async fn expand_windows(
        &self,
        chunks: Vec<RetrievedChunk>,
        window: i32,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        let db_path = self.db_path.clone();

        // 在移动 `chunks` 之前先收集好各项标识
        let targets: Vec<(String, String, i32)> = chunks
            .iter()
            .map(|c| (c.chunk.id.clone(), c.chunk.document_id.clone(), c.chunk.chunk_index))
            .collect();

        let expanded = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT content FROM chunks \
                     WHERE document_id = ?1 AND chunk_index BETWEEN ?2 AND ?3 \
                     ORDER BY chunk_index ASC",
                )
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            let mut map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();

            for (chunk_id, doc_id, chunk_index) in &targets {
                let contents: Vec<String> = stmt
                    .query_map(
                        rusqlite::params![doc_id, chunk_index - window, chunk_index + window],
                        |row| row.get(0),
                    )
                    .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
                    .filter_map(|r| r.ok())
                    .collect();

                map.insert(chunk_id.clone(), contents.join("\n"));
            }

            Ok::<_, KnowledgeBaseError>(map)
        })
        .await
        .map_err(|e| KnowledgeBaseError::DatabaseError(format!("spawn_blocking: {}", e)))??;

        let result = chunks
            .into_iter()
            .map(|mut c| {
                if let Some(content) = expanded.get(&c.chunk.id) {
                    c.chunk.content = content.clone();
                }
                c
            })
            .collect();

        Ok(result)
    }

    /// 纯向量相似度检索
    async fn vector_search(
        &self,
        request: &RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        embedding_base_url: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // 使用传入的 embedding 配置生成查询向量
        let query_vector = generate_single_embedding(
            &request.query,
            embedding_provider,
            api_key,
            embedding_model,
            embedding_base_url,
        ).await?;

        // 在向量存储中检索
        let results = self.vector_store
            .search(&request.kb_id, query_vector, request.top_k)
            .await?;

        // 转换为带完整元数据的 RetrievedChunk
        let chunks = self.enrich_chunks(results, &request.kb_id).await?;

        // 按相似度阈值过滤
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

    /// 纯关键词检索，使用 SQLite FTS 或 LIKE
    async fn keyword_search(
        &self,
        request: &RetrievalRequest,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = request.kb_id.clone();
        let query = request.query.clone();
        let top_k = request.top_k;
        
        // 在阻塞任务中执行 SQLite 操作
        let chunks = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            // 优先尝试 FTS5，失败则回退到 LIKE 查询
            Self::search_with_fts_blocking(&conn, &kb_id, &query, top_k)
                .or_else(|_| Self::search_with_like_blocking(&conn, &kb_id, &query, top_k))
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))??;

        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: chunks.len() as i32,
            chunks,
        })
    }

    /// 混合检索：结合向量与关键词
    async fn hybrid_search(
        &self,
        request: &RetrievalRequest,
        embedding_provider: &str,
        embedding_model: &str,
        embedding_base_url: &str,
        api_key: &str,
    ) -> Result<RetrievalResult, KnowledgeBaseError> {
        // 两种方式都用更大的 top_k 取结果，便于后续融合。
        // 把 similarity_threshold 清零，这样 vector_search 就不会在候选项还没机会被
        // RRF 中的关键词排名"捞回来"之前就被预先过滤掉 —— 向量相似度低但关键词精确
        // 命中的 chunk 应该能在合并阶段存活下来。
        // RRF 分数（约 0.001–0.033）和余弦相似度（0–1）不是同一量纲，无法直接比较，
        // 所以我们在合并后的输出上也跳过阈值过滤。
        let mut vector_request = request.clone();
        vector_request.top_k = request.top_k * 2;
        vector_request.similarity_threshold = 0.0;

        let mut keyword_request = request.clone();
        keyword_request.top_k = request.top_k * 2;

        let vector_result = self.vector_search(&vector_request, embedding_provider, embedding_model, embedding_base_url, api_key).await?;
        let keyword_result = self.keyword_search(&keyword_request).await?;

        // 使用 RRF 合并并重新排序
        let merged = self.merge_results(
            vector_result.chunks,
            keyword_result.chunks,
            request.top_k,
        );

        // 阈值要作用在原始的余弦分数（vector_score）上，而不是 RRF 分数 —— RRF
        // 的值（约 0.001–0.033）和 similarity_threshold（0–1）不可比较。一个 chunk
        // 只要满足以下任一条件即算通过：向量相似度高于阈值，或者它命中了关键词
        // （关键词命中本身就是一种相关性信号）。
        let filtered: Vec<_> = merged
            .into_iter()
            .filter(|c| {
                c.vector_score.map_or(false, |vs| vs >= request.similarity_threshold)
                    || c.keyword_score.is_some()
            })
            .collect();

        Ok(RetrievalResult {
            query: request.query.clone(),
            total_chunks: filtered.len() as i32,
            chunks: filtered,
        })
    }

    /// 从数据库获取知识库配置
    #[allow(dead_code)]
    async fn get_knowledge_base(&self, kb_id: &str) -> Result<KnowledgeBase, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            
            conn.query_row(
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
            ).map_err(|e| KnowledgeBaseError::NotFound(format!("Knowledge base not found: {}", e)))
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
    }

    /// 用 SQLite 中的元数据丰富 chunk 结果
    /// 对应 #38 的修复：改用 JOIN 而不是 N+1 次查询
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

            if results.is_empty() {
                return Ok(Vec::new());
            }

            // 构建一条带 JOIN 的查询，一次性拿到所有元数据
            let placeholders: String = results.iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");

            let chunk_ids: Vec<&str> = results.iter()
                .map(|(id, _, _, _)| id.as_str())
                .collect();

            let query = format!(
                r#"
                SELECT c.id, c.chunk_index, c.token_count,
                       COALESCE(d.filename, 'Unknown') as filename
                FROM chunks c
                LEFT JOIN documents d ON c.document_id = d.id
                WHERE c.id IN ({})
                "#,
                placeholders
            );

            let mut stmt = conn.prepare(&query)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            let metadata_rows: std::collections::HashMap<String, (i32, i32, String)> = stmt
                .query_map(rusqlite::params_from_iter(chunk_ids), |row| {
                    let id: String = row.get(0)?;
                    let chunk_index: i32 = row.get(1)?;
                    let token_count: i32 = row.get(2)?;
                    let filename: String = row.get(3)?;
                    Ok((id, (chunk_index, token_count, filename)))
                })
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();

            let chunks: Vec<RetrievedChunk> = results
                .into_iter()
                .map(|(chunk_id, doc_id, content, score)| {
                    let (chunk_index, token_count, filename) = metadata_rows
                        .get(&chunk_id)
                        .cloned()
                        .unwrap_or((0, 0, "Unknown".to_string()));

                    RetrievedChunk {
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
                    }
                })
                .collect();

            Ok(chunks)
        }).await.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?
    }

    /// 使用 FTS5（全文检索）进行搜索 —— 阻塞版本
    /// 对应 #37 的修复：对用户查询中的 FTS5 特殊字符做转义
    fn search_with_fts_blocking(
        conn: &rusqlite::Connection,
        kb_id: &str,
        query: &str,
        top_k: i32,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        // 检查 FTS 表是否存在
        let fts_exists: bool = conn.query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
            [],
            |_| Ok(true)
        ).unwrap_or(false);

        if !fts_exists {
            return Err(KnowledgeBaseError::RetrievalError("FTS5 not available".to_string()));
        }

        // 构建 FTS 查询：转义特殊字符，并把每个词用双引号包起来
        // FTS5 的特殊字符包括：" * ( ) : ^ [ ] { } + - AND OR NOT NEAR
        let fts_query: String = query
            .split_whitespace()
            .map(|term| {
                // 转义词内部的双引号
                let escaped = term.replace('"', "\"\"");
                format!("\"{}\"", escaped)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.token_count, d.filename,
                   rank
            FROM chunks_fts fts
            JOIN chunks c ON fts.rowid = c.rowid
            JOIN documents d ON c.document_id = d.id
            WHERE fts.kb_id = ?1 AND fts MATCH ?2
            ORDER BY rank
            LIMIT ?3
            "#
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let rows = stmt.query_map(
            rusqlite::params![kb_id, &fts_query, top_k],
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
                    score: 1.0, // FTS 不会直接给出 0-1 范围的分数
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

    /// 使用 LIKE 查询进行搜索（FTS 不可用时的回退方案）—— 阻塞版本
    /// 对应 #37 的修复：对用户查询中的 LIKE 通配符做转义
    fn search_with_like_blocking(
        conn: &rusqlite::Connection,
        kb_id: &str,
        query: &str,
        top_k: i32,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
        // 构建带通配符的 LIKE 模式，同时转义 LIKE 的特殊字符
        let escaped_terms: Vec<String> = query
            .split_whitespace()
            .map(|term| {
                // 转义 % 和 _ 字符
                let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
                escaped
            })
            .collect();

        let pattern = format!("%{}%", escaped_terms.join("%"));

        let mut stmt = conn.prepare(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.token_count, d.filename
            FROM chunks c
            JOIN documents d ON c.document_id = d.id
            WHERE c.kb_id = ?1 AND c.content LIKE ?2 ESCAPE '\'
            LIMIT ?3
            "#
        ).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        let rows = stmt.query_map(
            rusqlite::params![kb_id, &pattern, top_k],
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
                    score: 0.5, // LIKE 查询无法给出有意义的分数
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

    /// 使用 RRF（Reciprocal Rank Fusion，倒数排名融合）合并向量与关键词检索结果
    fn merge_results(
        &self,
        vector_chunks: Vec<RetrievedChunk>,
        keyword_chunks: Vec<RetrievedChunk>,
        top_k: i32,
    ) -> Vec<RetrievedChunk> {
        let k = 60.0; // RRF 常数
        let mut scores: std::collections::HashMap<String, (RetrievedChunk, f32)> = std::collections::HashMap::new();
        
        // 加入向量分数
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
        
        // 加入关键词分数
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
        
        // 按 RRF 分数排序并取前 top_k 个
        let mut results: Vec<_> = scores.into_iter()
            .map(|(_, (mut chunk, score))| {
                chunk.score = score;
                chunk
            })
            .collect();
        
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k as usize);
        
        results
    }
}

/// 用检索到的 chunk 为 LLM 构建上下文
#[allow(dead_code)]
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
