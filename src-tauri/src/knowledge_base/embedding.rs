// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 文本嵌入模块
 * 
 * 功能说明:
 * - 调用外部 API 生成文本向量
 * - 支持多种 Embedding 提供商 (OpenAI, 智谱, SiliconFlow)
 * - 批量处理支持
 * 
 * Embedding 向量用于:
 * - 文档相似度计算
 * - 语义检索
 */

use super::types::*;
use serde_json::json;

/// 获取 Embedding 模型配置
/// 
/// # 参数
/// - provider: 提供商名称
/// 
/// # 返回
/// (模型名称, 向量维度)
#[allow(dead_code)]
fn get_embedding_config(provider: &str) -> (&'static str, i32) {
    match provider {
        "openai" => ("text-embedding-3-small", 1536),
        "zhipu" => ("embedding-2", 1024),
        "siliconflow" => ("BAAI/bge-large-zh-v1.5", 1024),
        _ => ("text-embedding-3-small", 1536),
    }
}

/// 获取 Embedding API 端点 URL
///
/// 直接基于用户配置的 base_url 拼接 `/embeddings`（与 llm.rs::build_url 对
/// custom/local 提供商的处理方式一致），而不是依赖一份只覆盖 3 个服务商的
/// 硬编码表 —— 这样能支持设置里任意一个 OpenAI 兼容的 Embedding API 配置，
/// 而不仅仅是 openai/zhipu/siliconflow
fn get_embedding_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.openai.com/v1/embeddings".to_string();
    }
    format!("{}/embeddings", trimmed)
}

/// 批量处理的大小限制
const EMBEDDING_BATCH_SIZE: usize = 100;

/// 生成文本批次嵌入向量
/// 
/// # 参数
/// - texts: 文本列表
/// - provider: Embedding 提供商
/// - api_key: API 密钥
/// - model: 模型名称
/// 
/// # 返回
/// 向量列表 (每个 f32 向量)
pub async fn generate_embeddings(
    texts: Vec<String>,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_embeddings = Vec::new();

    for chunk in texts.chunks(EMBEDDING_BATCH_SIZE) {
        let batch_embeddings = generate_embeddings_batch(
            chunk.to_vec(),
            provider,
            api_key,
            model,
            base_url,
        ).await?;
        all_embeddings.extend(batch_embeddings);

        if texts.len() > EMBEDDING_BATCH_SIZE {
            tokio::time::sleep(std::time::Duration::from_millis(
                crate::commands::constants::EMBEDDING_BATCH_DELAY_MS,
            )).await;
        }
    }

    Ok(all_embeddings)
}

async fn generate_embeddings_batch(
    texts: Vec<String>,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let url = get_embedding_url(base_url);
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
    
    let auth_value = format!("Bearer {}", api_key.trim())
        .parse()
        .map_err(|e| KnowledgeBaseError::EmbeddingError(format!("Invalid API key: {}", e)))?;
    headers.insert(reqwest::header::AUTHORIZATION, auth_value);
    
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
    
    let embeddings = parse_embedding_response(&json)?;
    
    log::info!("Generated {} embeddings", embeddings.len());
    Ok(embeddings)
}

fn parse_embedding_array(data: &[serde_json::Value]) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
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

fn parse_embedding_response(json: &serde_json::Value) -> Result<Vec<Vec<f32>>, KnowledgeBaseError> {
    let data = json.get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| KnowledgeBaseError::EmbeddingError("Invalid response format".to_string()))?;
    
    parse_embedding_array(data)
}

/// Generate single embedding
pub async fn generate_single_embedding(
    text: &str,
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Vec<f32>, KnowledgeBaseError> {
    let embeddings = generate_embeddings(vec![text.to_string()], provider, api_key, model, base_url).await?;
    embeddings.into_iter().next()
        .ok_or_else(|| KnowledgeBaseError::EmbeddingError("No embedding generated".to_string()))
}

/// Get embedding dimension for a model
#[allow(dead_code)]
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
