// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;

/// 基于 SQLite、用余弦相似度做检索的向量存储
pub struct VectorStore {
    db_path: String,
}

impl VectorStore {
    pub async fn new(db_path: &str) -> Result<Self, KnowledgeBaseError> {
        // 确保目录存在
        std::fs::create_dir_all(db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        Ok(Self {
            db_path: db_path.to_string(),
        })
    }

    /// 为某个知识库创建向量表
    #[allow(dead_code)]
    pub async fn create_kb_table(&self, kb_id: &str, dim: i32) -> Result<(), KnowledgeBaseError> {
        let conn = self.get_conn()?;

        // 若不存在则创建 vectors 表
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS vectors (
                chunk_id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                kb_id TEXT NOT NULL,
                vector BLOB NOT NULL,
                FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            )
            "#,
            [],
        )
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // 创建索引以加快查询
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_vectors_kb ON vectors(kb_id)",
            [],
        )
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_vectors_doc ON vectors(document_id)",
            [],
        )
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        log::info!("Created vector table for knowledge base: {} (dim: {})", kb_id, dim);
        Ok(())
    }

    /// 插入向量 —— 包了一层 `spawn_blocking`，避免文档导入时批量 SQLite 写入
    /// 阻塞异步执行器。
    pub async fn insert_vectors(
        &self,
        kb_id: &str,
        vectors: Vec<(String, String, String, Vec<f32>)>, // (chunk_id, document_id, content, vector)
    ) -> Result<(), KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();

        tokio::task::spawn_blocking(move || {
            let main_db_path = std::path::Path::new(&db_path)
                .parent()
                .map(|p| p.join("app.db"))
                .ok_or_else(|| KnowledgeBaseError::DatabaseError("Invalid db path".to_string()))?;

            let conn = rusqlite::Connection::open(&main_db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            let count = vectors.len();
            for (chunk_id, document_id, _content, vector) in vectors {
                let vector_bytes = vector_to_bytes(&vector);
                conn.execute(
                    r#"
                    INSERT OR REPLACE INTO vectors (chunk_id, document_id, kb_id, vector)
                    VALUES (?1, ?2, ?3, ?4)
                    "#,
                    rusqlite::params![chunk_id, document_id, kb_id, vector_bytes],
                )
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            }

            log::info!("Inserted {} vectors for knowledge base: {}", count, kb_id);
            Ok(())
        })
        .await
        .map_err(|e| KnowledgeBaseError::DatabaseError(format!("spawn_blocking failed: {}", e)))?
    }

    /// 单个知识库的向量数超过这个值时，记一条提示日志说明本次是精确全量扫描。
    /// 这个阈值绝不会排除任何数据 —— 它只是提示：如果查询延迟在这个规模下已经
    /// 让用户能感知到，或许值得考虑上 ANN 索引。
    const LARGE_KB_SCAN_HINT: u64 = 200_000;

    /// 在知识库的全部向量上做精确余弦相似度检索（不会有任何文档被预先排除在候选之外）。
    ///
    /// 包了一层 `spawn_blocking`，避免阻塞式的 SQLite I/O 卡住异步执行器。内存占用
    /// 通过固定大小的最小堆流式处理每一行，而不是把所有打分结果都物化进一个 Vec，
    /// 把峰值内存限制在 O(top_k) —— 不再随知识库规模增长而增长。
    pub async fn search(
        &self,
        kb_id: &str,
        query_vector: Vec<f32>,
        top_k: i32,
    ) -> Result<Vec<(String, String, String, f32)>, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();

        tokio::task::spawn_blocking(move || {
            // top_k 非正数意味着"不需要任何结果"。
            if top_k <= 0 {
                return Ok(Vec::new());
            }
            let top_k = top_k as usize;

            let main_db_path = std::path::Path::new(&db_path)
                .parent()
                .map(|p| p.join("app.db"))
                .ok_or_else(|| KnowledgeBaseError::DatabaseError("Invalid db path".to_string()))?;

            let conn = rusqlite::Connection::open(&main_db_path)
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT v.chunk_id, v.document_id, c.content, v.vector
                    FROM vectors v
                    JOIN chunks c ON v.chunk_id = c.id
                    WHERE v.kb_id = ?1
                    "#,
                )
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            // `query_map` 是惰性游标 —— 每次只取到一行，不会把所有行都物化进内存。
            // 我们对每一行算出分数后只在最小堆里保留当前的 top_k，因此峰值内存维持在 O(top_k)。
            let rows = stmt
                .query_map([&kb_id], |row| {
                    let chunk_id: String = row.get(0)?;
                    let document_id: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let vector_bytes: Vec<u8> = row.get(3)?;
                    Ok((chunk_id, document_id, content, vector_bytes))
                })
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            // 保存当前最好的 top_k 个结果的最小堆。`Reverse` 让 BinaryHeap（本身是
            // 最大堆）表现得像最小堆，最小的分数会在堆顶，一旦超过 top_k 就淘汰它。
            let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<ScoredChunk>> =
                std::collections::BinaryHeap::with_capacity(top_k + 1);

            let mut scanned: u64 = 0;
            for row in rows {
                let (chunk_id, document_id, content, vector_bytes) =
                    row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
                scanned += 1;

                let vector = bytes_to_vector(&vector_bytes);
                let score = cosine_similarity(&query_vector, &vector);
                // `vector`（每行里占用内存最大的分配）在此处被释放。

                push_capped(
                    &mut heap,
                    ScoredChunk { score, chunk_id, document_id, content },
                    top_k,
                );
            }

            if scanned > Self::LARGE_KB_SCAN_HINT {
                log::info!(
                    "[KB] Exact full scan over {} vectors in '{}' (complete, no exclusion). \
                     Consider an ANN index if query latency becomes noticeable at this scale.",
                    scanned, kb_id
                );
            }

            let results: Vec<(String, String, String, f32)> = drain_sorted_desc(heap)
                .into_iter()
                .map(|s| (s.chunk_id, s.document_id, s.content, s.score))
                .collect();

            log::info!(
                "Vector search for {} scanned {} vectors, returned {} results",
                kb_id,
                scanned,
                results.len()
            );
            Ok(results)
        })
        .await
        .map_err(|e| KnowledgeBaseError::DatabaseError(format!("spawn_blocking failed: {}", e)))?
    }

    /// 按 document_id 删除向量
    pub async fn delete_document_vectors(
        &self,
        kb_id: &str,
        document_id: &str,
    ) -> Result<(), KnowledgeBaseError> {
        let conn = self.get_conn()?;
        conn.execute(
            "DELETE FROM vectors WHERE kb_id = ?1 AND document_id = ?2",
            [kb_id, document_id],
        )
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        log::info!("Deleted vectors for document: {} in {}", document_id, kb_id);
        Ok(())
    }

    /// 清空某个知识库的向量数据
    pub async fn drop_kb_table(&self, kb_id: &str) -> Result<(), KnowledgeBaseError> {
        let conn = self.get_conn()?;
        conn.execute("DELETE FROM vectors WHERE kb_id = ?1", [kb_id])
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        log::info!("Dropped vectors for knowledge base: {}", kb_id);
        Ok(())
    }

    fn get_conn(&self) -> Result<rusqlite::Connection, KnowledgeBaseError> {
        let main_db_path = std::path::Path::new(&self.db_path)
            .parent()
            .map(|p| p.join("app.db"))
            .ok_or_else(|| KnowledgeBaseError::DatabaseError("Invalid db path".to_string()))?;
        rusqlite::Connection::open(&main_db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))
    }
}

/// 向量检索过程中，top-k 最小堆里保存的一个打分候选项。
/// 排序只依据 `score`；NaN 分数（来自格式异常的 embedding）会被视为最小值，
/// 因此总是最先被淘汰，不会挤占正常结果的位置。
struct ScoredChunk {
    score: f32,
    chunk_id: String,
    document_id: String,
    content: String,
}

impl PartialEq for ScoredChunk {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for ScoredChunk {}
impl PartialOrd for ScoredChunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ScoredChunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        // 把 NaN 严格当作最小值处理，这样 NaN 候选项总会先于任何有效打分的候选项
        // 被淘汰，绝不会顶替掉一个真实的结果。
        match self.score.partial_cmp(&other.score) {
            Some(ord) => ord,
            None => match (self.score.is_nan(), other.score.is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => Ordering::Equal, // 对有限的 f32 而言这个分支不可能走到
            },
        }
    }
}

/// 把一个候选项推入有界的 top-k 最小堆，一旦堆的大小超过 `top_k` 就淘汰当前最低分的那个。
/// 使峰值内存维持在 O(top_k)。
fn push_capped(
    heap: &mut std::collections::BinaryHeap<std::cmp::Reverse<ScoredChunk>>,
    item: ScoredChunk,
    top_k: usize,
) {
    heap.push(std::cmp::Reverse(item));
    if heap.len() > top_k {
        heap.pop();
    }
}

/// 把 top-k 最小堆导出为一个按分数降序排列（最好的排在最前）的 Vec。
fn drain_sorted_desc(
    heap: std::collections::BinaryHeap<std::cmp::Reverse<ScoredChunk>>,
) -> Vec<ScoredChunk> {
    let mut scored: Vec<ScoredChunk> = heap.into_iter().map(|r| r.0).collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// 把向量（f32 数组）转换为字节序列
fn vector_to_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|&f| f.to_le_bytes())
        .collect()
}

/// 把字节序列转换回向量（f32 数组）
fn bytes_to_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(chunk);
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// 计算两个向量之间的余弦相似度
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

/// 元数据用的 SQLite schema
pub fn init_sqlite_tables(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    // 知识库表
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS knowledge_bases (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            embedding_api_config_id TEXT NOT NULL,
            chunk_size INTEGER NOT NULL DEFAULT 1000,
            chunk_overlap INTEGER NOT NULL DEFAULT 200,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            document_count INTEGER DEFAULT 0
        )
        "#,
        [],
    )?;

    // 迁移：若 chunk_size 和 chunk_overlap 列不存在则补上
    let table_info: Vec<String> = conn
        .prepare("PRAGMA table_info(knowledge_bases)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    
    // 若不存在则添加 embedding_provider（旧版遗留列）
    if !table_info.contains(&"embedding_provider".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_provider TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // 若不存在则添加 embedding_model（旧版遗留列）
    if !table_info.contains(&"embedding_model".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_model TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // 若不存在则添加 embedding_dim（旧版遗留列）
    if !table_info.contains(&"embedding_dim".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_dim INTEGER NOT NULL DEFAULT 1536",
            [],
        );
    }
    
    // 若不存在则添加 embedding_api_config_id
    if !table_info.contains(&"embedding_api_config_id".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_api_config_id TEXT NOT NULL DEFAULT ''",
            [],
        );
    }

    // 若不存在则添加 embedding_base_url
    if !table_info.contains(&"embedding_base_url".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_base_url TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // 若不存在则添加 chunk_size 和 chunk_overlap
    if !table_info.contains(&"chunk_size".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN chunk_size INTEGER NOT NULL DEFAULT 1000",
            [],
        );
    }
    if !table_info.contains(&"chunk_overlap".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN chunk_overlap INTEGER NOT NULL DEFAULT 200",
            [],
        );
    }

    // 文档表
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS documents (
            id TEXT PRIMARY KEY,
            kb_id TEXT NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
            filename TEXT NOT NULL,
            file_type TEXT NOT NULL,
            file_size INTEGER,
            file_hash TEXT,
            content_preview TEXT,
            chunk_count INTEGER DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'processing',
            error_message TEXT,
            created_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    // chunks 表 —— 存放供关键词检索使用的实际文本内容
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            kb_id TEXT NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
            content TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            token_count INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
        [],
    )?;

    // vectors 表 —— 存放 embedding 向量
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS vectors (
            chunk_id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL,
            kb_id TEXT NOT NULL,
            vector BLOB NOT NULL,
            FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
        )
        "#,
        [],
    )?;

    // 用于全文检索的 FTS5 虚拟表（可选，取决于 FTS5 是否可用）
    // 对应 #29、#30 的修复：加入 kb_id 列以实现知识库之间的隔离
    let _ = conn.execute(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            kb_id,
            content,
            content_rowid=rowid,
            tokenize='porter'
        )
        "#,
        [],
    );

    // 索引
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_kb_updated ON knowledge_bases(updated_at DESC)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_doc_kb ON documents(kb_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chunk_doc ON chunks(document_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chunk_kb ON chunks(kb_id)",
        [],
    )?;
    // 关键词检索用的索引
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chunk_content ON chunks(content)",
        [],
    )?;
    // vectors 相关索引
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_vectors_kb ON vectors(kb_id)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_vectors_doc ON vectors(document_id)",
        [],
    )?;

    log::info!("Knowledge base SQLite tables initialized");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 对一组分数运行真正的有界堆筛选逻辑（push_capped + drain_sorted_desc），
    /// 按顺序返回结果对应的 chunk_id。
    fn heap_top_k(scores: &[f32], top_k: usize) -> Vec<String> {
        let mut heap = std::collections::BinaryHeap::new();
        for (i, &score) in scores.iter().enumerate() {
            push_capped(
                &mut heap,
                ScoredChunk {
                    score,
                    chunk_id: i.to_string(),
                    document_id: "d".to_string(),
                    content: String::new(),
                },
                top_k,
            );
        }
        drain_sorted_desc(heap).into_iter().map(|s| s.chunk_id).collect()
    }

    /// 朴素对照实现：给每一项打分，降序排序，取前 top_k 个。
    fn naive_top_k(scores: &[f32], top_k: usize) -> Vec<String> {
        let mut idx: Vec<usize> = (0..scores.len()).collect();
        idx.sort_by(|&a, &b| {
            scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal)
        });
        idx.into_iter().take(top_k).map(|i| i.to_string()).collect()
    }

    #[test]
    fn heap_matches_naive_full_sort() {
        // 确定性的伪随机分数；各不相同，保证排序结果没有歧义。
        let n = 1000usize;
        let scores: Vec<f32> = (0..n)
            .map(|i| {
                let x = ((i as u64).wrapping_mul(2654435761) % 1_000_003) as f32;
                x / 1_000_003.0
            })
            .collect();

        for &top_k in &[1usize, 5, 20, 100] {
            let heap_ids = heap_top_k(&scores, top_k);
            let naive_ids = naive_top_k(&scores, top_k);
            assert_eq!(heap_ids, naive_ids, "mismatch at top_k={}", top_k);
        }
    }

    #[test]
    fn nan_scores_are_evicted_before_real_results() {
        // 三个正常结果加若干 NaN 候选项；top_k=3 时所有 NaN 都应被淘汰，
        // 只留下正常分数的结果，且顺序正确。
        let scores = vec![f32::NAN, 0.9, f32::NAN, 0.1, 0.5, f32::NAN];
        let ids = heap_top_k(&scores, 3);
        assert_eq!(ids, vec!["1".to_string(), "4".to_string(), "3".to_string()]);
        // 各索引对应关系：1 -> 0.9, 4 -> 0.5, 3 -> 0.1
    }

    #[test]
    fn top_k_larger_than_input_returns_all_sorted() {
        let scores = vec![0.2f32, 0.8, 0.5];
        let ids = heap_top_k(&scores, 10);
        assert_eq!(ids, vec!["1".to_string(), "2".to_string(), "0".to_string()]);
    }
}
