// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use serde_json::json;

/// Get embedding model configuration
fn get_embedding_config(provider: &str) -> (&'static str, i32) {
    match provider {
        "openai" => ("text-embedding-3-small", 1536),
        "zhipu" => ("embedding-2", 1024),
        "siliconflow" => ("BAAI/bge-large-zh-v1.5", 1024),
        _ => ("text-embedding-3-small", 1536),
    }
}

/// Get embedding API URL
fn get_embedding_url(provider: &str) -> String {
    match provider {
        "openai" => "https://api.openai.com/v1/embeddings".to_string(),
        "zhipu" => "https://open.bigmodel.cn/api/paas/v4/embeddings".to_string(),
        "siliconflow" => "https://api.siliconflow.cn/v1/embeddings".to_string(),
        _ => "https://api.openai.com/v1/embeddings".to_string(),
    }
}

/// Generate embeddings for text batch
pub async fn generate_embeddings(
    texts: Vec<String>,
    provider: &str,
    api_key: &str,
    model: &str,
) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    
    let url = get_embedding_url(provider);
    let client = reqwest::Client::new();
    
    // Build request body
    let body = match provider {
        "zhipu" => {
            json!({
                "model": model,
                "input": texts,
            })
        }
        _ => {
            json!({
                "model": model,
                "input": texts,
                "encoding_format": "float",
            })
        }
    };
    
    // Build headers
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    
    if provider == "zhipu" {
        // Zhipu uses Authorization: Bearer token
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key).parse().unwrap(),
        );
    } else {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", api_key).parse().unwrap(),
        );
    }
    
    log::info!("Sending embedding request to {} for {} texts", provider, texts.len());
    
    let response = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| KnowledgeBaseError::EmbeddingError(format!("Request failed: {}", e)))?;
    
    if !response.status().is_success() {
        let error_text = response.text().await
            .map_err(|e| KnowledgeBaseError::EmbeddingError(format!("Failed to read error: {}", e)))?;
        return Err(KnowledgeBaseError::EmbeddingError(format!("API error: {}", error_text)));
    }
    
    let json: serde_json::Value = response.json().await
        .map_err(|e| KnowledgeBaseError::EmbeddingError(format!("Failed to parse response: {}", e)))?;
    
    // Parse embeddings based on provider format
    let embeddings = parse_embedding_response(&json, provider)?;
    
    log::info!("Generated {} embeddings", embeddings.len());
    Ok(embeddings)
}

/// Parse embedding response
fn parse_embedding_response(json: &serde_json::Value, provider: &str) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
    match provider {
        "zhipu" => {
            // Zhipu format: data[].embedding
            let data = json.get("data")
                .and_then(|d| d.as_array())
                .ok_or_else(|| KnowledgeBaseError::EmbeddingError("Invalid response format".to_string()))?;
            
            let mut embeddings = Vec::new();
            for item in data {
                let embedding = item.get("embedding")
                    .and_then(|e| e.as_array())
                    .ok_or_else(|| KnowledgeBaseError::EmbeddingError("Missing embedding field".to_string()))?;
                
                let vec: Vec<f32> = embedding.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                embeddings.push(vec);
            }
            Ok(embeddings)
        }
        _ => {
            // OpenAI format: data[].embedding
            let data = json.get("data")
                .and_then(|d| d.as_array())
                .ok_or_else(|| KnowledgeBaseError::EmbeddingError("Invalid response format".to_string()))?;
            
            let mut embeddings = Vec::new();
            for item in data {
                let embedding = item.get("embedding")
                    .and_then(|e| e.as_array())
                    .ok_or_else(|| KnowledgeBaseError::EmbeddingError("Missing embedding field".to_string()))?;
                
                let vec: Vec<f32> = embedding.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                embeddings.push(vec);
            }
            Ok(embeddings)
        }
    }
}

/// Generate single embedding
pub async fn generate_single_embedding(
    text: &str,
    provider: &str,
    api_key: &str,
    model: &str,
) -> Result<Vec<f32>, KnowledgeBaseError> {
    let embeddings = generate_embeddings(vec![text.to_string()], provider, api_key, model).await?;
    embeddings.into_iter().next()
        .ok_or_else(|| KnowledgeBaseError::EmbeddingError("No embedding generated".to_string()))
}

/// Get embedding dimension for a model
pub fn get_embedding_dimension(provider: &str, model: &str) -> i32 {
    match (provider, model) {
        ("openai", "text-embedding-3-small") => 1536,
        ("openai", "text-embedding-3-large") => 3072,
        ("openai", "text-embedding-ada-002") => 1536,
        ("zhipu", _) => 1024,
        ("siliconflow", _) => 1024,
        _ => 1536,
    }
}

/// Available embedding models
pub fn get_available_embedding_models() -> Vec<(String, String, i32)> {
    vec![
        ("openai".to_string(), "text-embedding-3-small".to_string(), 1536),
        ("openai".to_string(), "text-embedding-3-large".to_string(), 3072),
        ("zhipu".to_string(), "embedding-2".to_string(), 1024),
        ("siliconflow".to_string(), "BAAI/bge-large-zh-v1.5".to_string(), 1024),
    ]
}
