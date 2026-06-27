// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;

/// Vector store using SQLite with cosine similarity search
pub struct VectorStore {
    db_path: String,
}

impl VectorStore {
    pub async fn new(db_path: &str) -> Result<Self, KnowledgeBaseError> {
        // Ensure directory exists
        std::fs::create_dir_all(db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        Ok(Self {
            db_path: db_path.to_string(),
        })
    }

    /// Create vector table for a knowledge base
    #[allow(dead_code)]
    pub async fn create_kb_table(&self, kb_id: &str, dim: i32) -> Result<(), KnowledgeBaseError> {
        let conn = self.get_conn()?;

        // Create vectors table if not exists
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

        // Create index for faster lookups
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

    /// Insert vectors — wrapped in `spawn_blocking` so batch SQLite writes
    /// don't stall the async executor during document import.
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

    /// Above this many vectors in a single knowledge base, log an informational
    /// note that an exact full scan is being performed. This NEVER excludes any
    /// data — it only hints that an ANN index might be worthwhile if query
    /// latency ever becomes user-visible at this scale.
    const LARGE_KB_SCAN_HINT: u64 = 200_000;

    /// Vector search using exact cosine similarity over ALL vectors in the
    /// knowledge base (no document is ever excluded from candidacy).
    ///
    /// Wrapped in `spawn_blocking` so blocking SQLite I/O doesn't stall the
    /// async executor. Memory is bounded to O(top_k) by streaming rows through
    /// a fixed-size min-heap instead of materializing every scored row into a
    /// Vec — peak memory no longer scales with the size of the knowledge base.
    pub async fn search(
        &self,
        kb_id: &str,
        query_vector: Vec<f32>,
        top_k: i32,
    ) -> Result<Vec<(String, String, String, f32)>, KnowledgeBaseError> {
        let db_path = self.db_path.clone();
        let kb_id = kb_id.to_string();

        tokio::task::spawn_blocking(move || {
            // A non-positive top_k means "no results requested".
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

            // `query_map` is a lazy cursor — rows arrive one at a time and are
            // NOT all materialized in memory. We compute each score and keep
            // only the running top_k in a min-heap, so peak memory stays O(top_k).
            let rows = stmt
                .query_map([&kb_id], |row| {
                    let chunk_id: String = row.get(0)?;
                    let document_id: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let vector_bytes: Vec<u8> = row.get(3)?;
                    Ok((chunk_id, document_id, content, vector_bytes))
                })
                .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

            // Min-heap of the best top_k so far. `Reverse` makes BinaryHeap (a
            // max-heap) behave as a min-heap, so the smallest score sits on top
            // and is the one evicted once we exceed top_k.
            let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<ScoredChunk>> =
                std::collections::BinaryHeap::with_capacity(top_k + 1);

            let mut scanned: u64 = 0;
            for row in rows {
                let (chunk_id, document_id, content, vector_bytes) =
                    row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
                scanned += 1;

                let vector = bytes_to_vector(&vector_bytes);
                let score = cosine_similarity(&query_vector, &vector);
                // `vector` (the largest per-row allocation) is dropped here.

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

    /// Delete vectors by document_id
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

    /// Drop knowledge base table
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

/// One scored candidate held in the top-k min-heap during a vector search.
/// Ordered solely by `score`; NaN scores (from malformed embeddings) sort as
/// the smallest so they are evicted first and never crowd out real results.
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
        // Treat NaN as strictly the smallest value so a NaN candidate is always
        // evicted before any real-scored one and never displaces a real result.
        match self.score.partial_cmp(&other.score) {
            Some(ord) => ord,
            None => match (self.score.is_nan(), other.score.is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => Ordering::Equal, // unreachable for finite f32
            },
        }
    }
}

/// Push a candidate into a bounded top-k min-heap, evicting the current lowest
/// score once the heap exceeds `top_k`. Keeps peak memory at O(top_k).
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

/// Drain a top-k min-heap into a Vec sorted by score descending (best first).
fn drain_sorted_desc(
    heap: std::collections::BinaryHeap<std::cmp::Reverse<ScoredChunk>>,
) -> Vec<ScoredChunk> {
    let mut scored: Vec<ScoredChunk> = heap.into_iter().map(|r| r.0).collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// Convert vector (f32 array) to bytes
fn vector_to_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|&f| f.to_le_bytes())
        .collect()
}

/// Convert bytes back to vector (f32 array)
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

/// Calculate cosine similarity between two vectors
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

/// SQLite schema for metadata
pub fn init_sqlite_tables(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    // Knowledge bases table
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

    // Migrate: add chunk_size and chunk_overlap columns if they don't exist
    let table_info: Vec<String> = conn
        .prepare("PRAGMA table_info(knowledge_bases)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    
    // Add embedding_provider if it doesn't exist (old column)
    if !table_info.contains(&"embedding_provider".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_provider TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // Add embedding_model if it doesn't exist (old column)
    if !table_info.contains(&"embedding_model".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_model TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // Add embedding_dim if it doesn't exist (old column)
    if !table_info.contains(&"embedding_dim".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_dim INTEGER NOT NULL DEFAULT 1536",
            [],
        );
    }
    
    // Add embedding_api_config_id if it doesn't exist
    if !table_info.contains(&"embedding_api_config_id".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_api_config_id TEXT NOT NULL DEFAULT ''",
            [],
        );
    }

    // Add embedding_base_url if it doesn't exist
    if !table_info.contains(&"embedding_base_url".to_string()) {
        let _ = conn.execute(
            "ALTER TABLE knowledge_bases ADD COLUMN embedding_base_url TEXT NOT NULL DEFAULT ''",
            [],
        );
    }
    
    // Add chunk_size and chunk_overlap if they don't exist
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

    // Documents table
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

    // Chunks table - stores actual content for keyword search
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

    // Vectors table - stores embedding vectors
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

    // FTS5 virtual table for full-text search (optional, if FTS5 is available)
    // Fix for #29 and #30: Include kb_id column for knowledge base isolation
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

    // Indexes
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
    // Index for keyword search
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_chunk_content ON chunks(content)",
        [],
    )?;
    // Indexes for vectors
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

    /// Run the real bounded-heap selection (push_capped + drain_sorted_desc)
    /// over a set of scores and return the resulting chunk_ids in order.
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

    /// Naive reference: score every item, sort descending, take top_k.
    fn naive_top_k(scores: &[f32], top_k: usize) -> Vec<String> {
        let mut idx: Vec<usize> = (0..scores.len()).collect();
        idx.sort_by(|&a, &b| {
            scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal)
        });
        idx.into_iter().take(top_k).map(|i| i.to_string()).collect()
    }

    #[test]
    fn heap_matches_naive_full_sort() {
        // Deterministic pseudo-random scores; distinct so ordering is unambiguous.
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
        // Three real results and several NaN candidates; with top_k=3 the NaNs
        // must all be evicted and only the real scores survive, in order.
        let scores = vec![f32::NAN, 0.9, f32::NAN, 0.1, 0.5, f32::NAN];
        let ids = heap_top_k(&scores, 3);
        assert_eq!(ids, vec!["1".to_string(), "4".to_string(), "3".to_string()]);
        // indices: 1 -> 0.9, 4 -> 0.5, 3 -> 0.1
    }

    #[test]
    fn top_k_larger_than_input_returns_all_sorted() {
        let scores = vec![0.2f32, 0.8, 0.5];
        let ids = heap_top_k(&scores, 10);
        assert_eq!(ids, vec!["1".to_string(), "2".to_string(), "0".to_string()]);
    }
}
