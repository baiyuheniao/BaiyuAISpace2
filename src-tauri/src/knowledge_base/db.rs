// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct VectorStore {
    db: Arc<Mutex<lancedb::Connection>>,
}

impl VectorStore {
    pub async fn new(db_path: &str) -> Result<Self, KnowledgeBaseError> {
        let conn = lancedb::connect(db_path)
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create vector table for a knowledge base
    pub async fn create_kb_table(&self, kb_id: &str, dim: i32) -> Result<(), KnowledgeBaseError> {
        let db = self.db.lock().await;
        
        // Check if table exists
        let table_name = format!("kb_{}", kb_id);
        match db.open_table(&table_name).execute().await {
            Ok(_) => {
                log::info!("Vector table {} already exists", table_name);
                Ok(())
            }
            Err(_) => {
                // Table doesn't exist, create it
                use arrow_array::{ArrayRef, Float32Array, StringArray, RecordBatch};
                use std::sync::Arc;
                
                // Create empty table with schema
                // Schema: vector (fixed size list), chunk_id, document_id, content
                let schema = Arc::new(arrow::datatypes::Schema::new(vec![
                    arrow::datatypes::Field::new(
                        "vector",
                        arrow::datatypes::DataType::FixedSizeList(
                            Arc::new(arrow::datatypes::Field::new(
                                "item",
                                arrow::datatypes::DataType::Float32,
                                true,
                            )),
                            dim,
                        ),
                        true,
                    ),
                    arrow::datatypes::Field::new("chunk_id", arrow::datatypes::DataType::Utf8, true),
                    arrow::datatypes::Field::new("document_id", arrow::datatypes::DataType::Utf8, true),
                    arrow::datatypes::Field::new("content", arrow::datatypes::DataType::Utf8, true),
                ]));
                
                let batch = RecordBatch::new_empty(schema);
                
                db.create_table(&table_name, batch.into())
                    .execute()
                    .await
                    .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
                
                log::info!("Created vector table: {}", table_name);
                Ok(())
            }
        }
    }

    /// Insert vectors
    pub async fn insert_vectors(
        &self,
        kb_id: &str,
        vectors: Vec<(String, String, String, Vec<f32>)>, // (chunk_id, document_id, content, vector)
    ) -> Result<(), KnowledgeBaseError> {
        let db = self.db.lock().await;
        let table_name = format!("kb_{}", kb_id);
        
        let table = db.open_table(&table_name)
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        // Build record batch
        use arrow_array::{ArrayRef, Float32Array, StringArray, FixedSizeListArray, RecordBatch};
        use arrow::datatypes::Int32Type;
        use std::sync::Arc;
        
        let chunk_ids: ArrayRef = Arc::new(StringArray::from(
            vectors.iter().map(|v| v.0.clone()).collect::<Vec<_>>()
        ));
        let document_ids: ArrayRef = Arc::new(StringArray::from(
            vectors.iter().map(|v| v.1.clone()).collect::<Vec<_>>()
        ));
        let contents: ArrayRef = Arc::new(StringArray::from(
            vectors.iter().map(|v| v.2.clone()).collect::<Vec<_>>()
        ));
        
        // Build vector array
        let dim = vectors.first().map(|v| v.3.len()).unwrap_or(0) as i32;
        let vector_values: Vec<f32> = vectors.iter().flat_map(|v| v.3.clone()).collect();
        let vector_array = Float32Array::from(vector_values);
        let vector_list = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            vec![Some(vector_array.iter().map(|v| v).collect::<Vec<_>>()); vectors.len()],
            dim,
        );
        
        let batch = RecordBatch::try_from_iter(vec![
            ("vector", Arc::new(vector_list) as ArrayRef),
            ("chunk_id", chunk_ids),
            ("document_id", document_ids),
            ("content", contents),
        ]).map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        table.add(batch.into())
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        log::info!("Inserted {} vectors into {}", vectors.len(), table_name);
        Ok(())
    }

    /// Vector search
    pub async fn search(
        &self,
        kb_id: &str,
        query_vector: Vec<f32>,
        top_k: i32,
    ) -> Result<Vec<(String, String, String, f32)>, KnowledgeBaseError> {
        let db = self.db.lock().await;
        let table_name = format!("kb_{}", kb_id);
        
        let table = db.open_table(&table_name)
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        let results = table
            .search(query_vector)
            .limit(top_k as usize)
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::RetrievalError(e.to_string()))?;
        
        // Parse results
        let mut chunks = Vec::new();
        // Note: LanceDB returns a RecordBatch, we need to extract columns
        // This is simplified - actual implementation needs proper column extraction
        
        log::info!("Vector search returned results from {}", table_name);
        Ok(chunks)
    }

    /// Delete vectors by document_id
    pub async fn delete_document_vectors(&self, kb_id: &str, document_id: &str) -> Result<(), KnowledgeBaseError> {
        let db = self.db.lock().await;
        let table_name = format!("kb_{}", kb_id);
        
        let table = db.open_table(&table_name)
            .execute()
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        table.delete(&format!("document_id = '{}'", document_id))
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        log::info!("Deleted vectors for document {} from {}", document_id, table_name);
        Ok(())
    }

    /// Drop knowledge base table
    pub async fn drop_kb_table(&self, kb_id: &str) -> Result<(), KnowledgeBaseError> {
        let db = self.db.lock().await;
        let table_name = format!("kb_{}", kb_id);
        
        db.drop_table(&table_name)
            .await
            .map_err(|e| KnowledgeBaseError::DatabaseError(e.to_string()))?;
        
        log::info!("Dropped vector table: {}", table_name);
        Ok(())
    }
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

    // Chunks table
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

    log::info!("Knowledge base SQLite tables initialized");
    Ok(())
}
