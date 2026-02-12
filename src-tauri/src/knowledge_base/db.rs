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

    /// Insert vectors
    pub async fn insert_vectors(
        &self,
        kb_id: &str,
        vectors: Vec<(String, String, String, Vec<f32>)>, // (chunk_id, document_id, content, vector)
    ) -> Result<(), KnowledgeBaseError> {
        let conn = self.get_conn()?;

        let count = vectors.len();
        for (chunk_id, document_id, _content, vector) in vectors {
            // Serialize vector to bytes (f32 array)
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
    }

    /// Vector search using cosine similarity
    pub async fn search(
        &self,
        kb_id: &str,
        query_vector: Vec<f32>,
        top_k: i32,
    ) -> Result<Vec<(String, String, String, f32)>, KnowledgeBaseError> {
        let conn = self.get_conn()?;

        // Query all vectors for this knowledge base
        // For better performance with large datasets, consider using approximate methods
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

        let rows = stmt
            .query_map([kb_id], |row| {
                let chunk_id: String = row.get(0)?;
                let document_id: String = row.get(1)?;
                let content: String = row.get(2)?;
                let vector_bytes: Vec<u8> = row.get(3)?;
                let vector = bytes_to_vector(&vector_bytes);
                Ok((chunk_id, document_id, content, vector))
            })
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        // Calculate cosine similarity for all vectors
        let mut scored_results: Vec<(String, String, String, f32)> = Vec::new();
        for row in rows {
            let (chunk_id, document_id, content, vector) =
                row.map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
            let similarity = cosine_similarity(&query_vector, &vector);
            scored_results.push((chunk_id, document_id, content, similarity));
        }

        // Sort by similarity (descending) and take top_k
        scored_results.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
        scored_results.truncate(top_k as usize);

        log::info!(
            "Vector search for {} returned {} results",
            kb_id,
            scored_results.len()
        );
        Ok(scored_results)
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

        conn.execute(
            "DELETE FROM vectors WHERE kb_id = ?1",
            [kb_id],
        )
        .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;

        log::info!("Dropped vectors for knowledge base: {}", kb_id);
        Ok(())
    }

    /// Get SQLite connection
    fn get_conn(&self) -> Result<rusqlite::Connection, KnowledgeBaseError> {
        // The vectors are stored in the main SQLite database
        // We need to get the main db path from the app
        // For now, we'll derive it from the vector db path
        let main_db_path = std::path::Path::new(&self.db_path)
            .parent()
            .map(|p| p.join("app.db"))
            .ok_or_else(|| KnowledgeBaseError::DatabaseError("Invalid db path".to_string()))?;

        rusqlite::Connection::open(&main_db_path)
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))
    }
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
            embedding_provider TEXT NOT NULL,
            embedding_model TEXT NOT NULL,
            embedding_dim INTEGER NOT NULL,
            chunk_size INTEGER NOT NULL DEFAULT 1000,
            chunk_overlap INTEGER NOT NULL DEFAULT 200,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            document_count INTEGER DEFAULT 0
        )
        "#,
        [],
    )?;

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
    let _ = conn.execute(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
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
