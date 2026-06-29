// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::{KnowledgeBaseError, RetrievedChunk};

/// Re-rank retrieved chunks using a Cohere-compatible reranker API.
///
/// Compatible with: Cohere, Jina, Voyage, and BGE-style rerankers that expose
/// `POST {base_url}/v1/rerank` with the Cohere request/response schema.
///
/// The returned vec is sorted by relevance_score descending and trimmed to
/// `top_n` entries. Each chunk's `score` field is replaced with the reranker's
/// relevance score.
pub async fn rerank_chunks(
    query: &str,
    chunks: Vec<RetrievedChunk>,
    top_n: usize,
    api_key: &str,
    model: &str,
    base_url: &str,
) -> Result<Vec<RetrievedChunk>, KnowledgeBaseError> {
    if chunks.is_empty() {
        return Ok(chunks);
    }

    let documents: Vec<String> = chunks.iter().map(|c| c.chunk.content.clone()).collect();

    let url = format!("{}/v1/rerank", base_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| KnowledgeBaseError::RetrievalError(format!("Failed to build HTTP client: {}", e)))?;

    let body = serde_json::json!({
        "model": model,
        "query": query,
        "documents": documents,
        "top_n": top_n,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| KnowledgeBaseError::RetrievalError(format!("Reranker request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(KnowledgeBaseError::RetrievalError(
            format!("Reranker API returned {}: {}", status, error_text)
        ));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| KnowledgeBaseError::RetrievalError(format!("Failed to parse reranker response: {}", e)))?;

    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| KnowledgeBaseError::RetrievalError(
            "Reranker response missing 'results' array".to_string()
        ))?;

    let mut reranked: Vec<RetrievedChunk> = results
        .iter()
        .filter_map(|r| {
            let index = r.get("index")?.as_u64()? as usize;
            let score = r.get("relevance_score")?.as_f64()? as f32;
            let mut chunk = chunks.get(index)?.clone();
            chunk.score = score;
            Some(chunk)
        })
        .collect();

    reranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(reranked)
}
