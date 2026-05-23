// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Local model management module
//!
//! Provides commands for managing locally deployed models via Ollama API.
//! Supports multiple model sources (Ollama official, HuggingFace, ModelScope)
//! for downloading/pulling models.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ============ Types ============

/// Information about a locally available model from Ollama
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelInfo {
    pub name: String,
    /// Model display name (e.g. "llama3:latest")
    pub model: String,
    /// Modified timestamp
    pub modified_at: String,
    /// Model size in bytes
    pub size: u64,
    /// Model digest hash
    pub digest: String,
    /// Model details (family, parameter size, quantization level)
    pub details: Option<ModelDetails>,
}

/// Detailed model information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetails {
    pub parent_model: Option<String>,
    pub format: Option<String>,
    pub family: Option<String>,
    pub families: Option<Vec<String>>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
}

/// Ollama API response for listing models
#[derive(Debug, Deserialize)]
struct OllamaListResponse {
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    name: String,
    model: String,
    modified_at: String,
    size: u64,
    digest: String,
    details: Option<ModelDetails>,
}

/// Ollama API response for model show
#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    details: Option<ModelDetails>,
    // Other fields we don't need
    #[allow(dead_code)]
    license: Option<String>,
    #[allow(dead_code)]
    modelfile: Option<String>,
    #[allow(dead_code)]
    parameters: Option<String>,
    #[allow(dead_code)]
    template: Option<String>,
}

/// Model source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSource {
    /// Unique identifier for the source
    pub id: String,
    /// Display name
    pub name: String,
    /// Base URL for the model registry
    pub base_url: String,
    /// Description
    pub description: String,
}

/// Download progress event emitted to frontend
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model_name: String,
    pub status: String,
    pub digest: String,
    pub total: Option<u64>,
    pub completed: Option<u64>,
}

/// Pull request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullModelRequest {
    /// Model name (e.g. "llama3:latest" or "huggingface/user/model:tag")
    pub model_name: String,
    /// Source ID to pull from (optional, uses configured default)
    pub source_id: Option<String>,
    /// Whether to use insecure connection
    pub insecure: Option<bool>,
}

/// Delete request parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteModelRequest {
    /// Model name to delete
    pub model_name: String,
}

/// Configuration for local model service
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelConfig {
    /// Ollama service base URL
    pub ollama_base_url: String,
    /// Default model source ID
    pub default_source_id: String,
}

/// Predefined model sources
pub fn get_model_sources() -> Vec<ModelSource> {
    vec![
        ModelSource {
            id: "ollama".to_string(),
            name: "Ollama 官方".to_string(),
            base_url: "https://registry.ollama.ai".to_string(),
            description: "Ollama 官方模型库，包含主流开源模型".to_string(),
        },
        ModelSource {
            id: "huggingface".to_string(),
            name: "HuggingFace".to_string(),
            base_url: "https://huggingface.co".to_string(),
            description: "全球最大的开源模型社区".to_string(),
        },
        ModelSource {
            id: "modelscope".to_string(),
            name: "ModelScope (魔搭)".to_string(),
            base_url: "https://modelscope.cn".to_string(),
            description: "阿里巴巴达摩院开源模型社区".to_string(),
        },
    ]
}

// ============ Helper functions ============

/// Create HTTP client for Ollama API calls
fn create_ollama_client(_base_url: &str) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        .build()
}

/// Build Ollama API URL from base URL and endpoint
fn build_ollama_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{}{}", base, endpoint)
}

// ============ Tauri Commands ============

/// Check if Ollama service is running and accessible
#[tauri::command]
pub async fn check_ollama_status(
    ollama_base_url: String,
) -> Result<bool, String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/tags");

    match client.get(&url).send().await {
        Ok(response) => Ok(response.status().is_success()),
        Err(e) => {
            log::debug!("Ollama status check failed: {}", e);
            Ok(false)
        }
    }
}

/// List all locally available models from Ollama
#[tauri::command]
pub async fn list_local_models(
    ollama_base_url: String,
) -> Result<Vec<LocalModelInfo>, String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/tags");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status: {}", response.status()));
    }

    let body: OllamaListResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

    let models: Vec<LocalModelInfo> = body
        .models
        .into_iter()
        .map(|m| LocalModelInfo {
            name: m.name.clone(),
            model: m.model,
            modified_at: m.modified_at,
            size: m.size,
            digest: m.digest,
            details: m.details,
        })
        .collect();

    Ok(models)
}

/// Get detailed information about a specific local model
#[tauri::command]
pub async fn show_local_model(
    ollama_base_url: String,
    model_name: String,
) -> Result<LocalModelInfo, String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/show");

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "name": model_name }))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status: {}", response.status()));
    }

    let body: OllamaShowResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

    Ok(LocalModelInfo {
        name: model_name.clone(),
        model: model_name,
        modified_at: String::new(),
        size: 0,
        digest: String::new(),
        details: body.details,
    })
}

/// Pull (download) a model from a specified source
/// Emits download progress events to the frontend
#[tauri::command]
pub async fn pull_local_model(
    request: PullModelRequest,
    ollama_base_url: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/pull");

    // Build the model name with source prefix if needed
    let model_ref = match request.source_id.as_deref() {
        Some("huggingface") => {
            // HuggingFace format: hf.co/user/model:tag or hf.co/user/model
            if !request.model_name.starts_with("hf.co/") {
                format!("hf.co/{}", request.model_name)
            } else {
                request.model_name.clone()
            }
        }
        Some("modelscope") => {
            // ModelScope format: ms://user/model or just the model name
            // Ollama supports pulling from ModelScope via the model name
            request.model_name.clone()
        }
        _ => {
            // Default: Ollama official registry
            request.model_name.clone()
        }
    };

    let mut body = serde_json::json!({
        "name": model_ref,
        "stream": true,
    });

    if request.insecure.unwrap_or(false) {
        body["insecure"] = serde_json::json!(true);
    }

    log::info!("Pulling model: {}", model_ref);

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Failed to start model pull: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Pull failed: {}", error_text));
    }

    // Process streaming response for progress updates
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error during pull: {}", e))?;
        let text = String::from_utf8_lossy(&chunk);
        buffer.push_str(&text);

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Ok(progress) = serde_json::from_str::<serde_json::Value>(&line) {
                let status = progress["status"].as_str().unwrap_or("unknown").to_string();
                let digest = progress["digest"].as_str().unwrap_or("").to_string();
                let total = progress["total"].as_u64();
                let completed = progress["completed"].as_u64();

                let _ = app_handle.emit("download-progress", DownloadProgress {
                    model_name: request.model_name.clone(),
                    status,
                    digest,
                    total,
                    completed,
                });
            }
        }
    }

    log::info!("Model pull completed: {}", request.model_name);
    Ok(())
}

/// Delete a local model from Ollama
#[tauri::command]
pub async fn delete_local_model(
    request: DeleteModelRequest,
    ollama_base_url: String,
) -> Result<(), String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/delete");

    let response = client
        .delete(&url)
        .json(&serde_json::json!({ "name": request.model_name }))
        .send()
        .await
        .map_err(|e| format!("Failed to delete model: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Delete failed: {}", error_text));
    }

    log::info!("Model deleted: {}", request.model_name);
    Ok(())
}

/// Get the list of available model sources
#[tauri::command]
pub async fn get_model_sources_cmd() -> Result<Vec<ModelSource>, String> {
    Ok(get_model_sources())
}

/// Get Ollama version info
#[tauri::command]
pub async fn get_ollama_version(
    ollama_base_url: String,
) -> Result<String, String> {
    let client = create_ollama_client(&ollama_base_url)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/version");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status: {}", response.status()));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(body["version"].as_str().unwrap_or("unknown").to_string())
}
