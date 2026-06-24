// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! LM Studio local server integration
//!
//! LM Studio runs its own local inference server (default
//! `http://localhost:1234`) with both an OpenAI-compatible surface
//! (`/v1/chat/completions`, `/v1/models`) and a native v1 management API
//! (`/api/v1/models/download`, `/load`, `/unload`) used here for model
//! management, plus a richer v0 listing endpoint (`/api/v0/models`) that
//! reports per-model publisher/arch/quantization/load-state. Unlike Ollama,
//! LM Studio is a closed-source GUI desktop app with no documented
//! headless/silent install, so this module only ever talks to an
//! already-running server -- it never tries to install or launch LM Studio
//! itself.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ============ Types ============

/// A model as reported by LM Studio's `/api/v0/models` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioModelInfo {
    pub id: String,
    pub model_type: String,
    pub publisher: Option<String>,
    pub arch: Option<String>,
    pub compatibility_type: Option<String>,
    pub quantization: Option<String>,
    /// "loaded" | "not-loaded"
    pub state: String,
    pub max_context_length: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LMStudioModelsResponse {
    data: Vec<LMStudioModelEntry>,
}

#[derive(Debug, Deserialize)]
struct LMStudioModelEntry {
    id: String,
    #[serde(rename = "type")]
    model_type: Option<String>,
    publisher: Option<String>,
    arch: Option<String>,
    compatibility_type: Option<String>,
    quantization: Option<String>,
    state: Option<String>,
    max_context_length: Option<u64>,
}

/// Download progress event emitted to the frontend.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioDownloadProgress {
    pub model_id: String,
    /// "downloading" | "paused" | "completed" | "failed"
    pub status: String,
    pub downloaded_bytes: Option<u64>,
    pub total_size_bytes: Option<u64>,
}

// ============ Helpers ============

fn create_lmstudio_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        .build()
}

fn build_lmstudio_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{}{}", base, endpoint)
}

/// LM Studio's local server has no auth by default; an API token is only
/// needed if the user explicitly enabled "Require API key" in its settings,
/// so this header is only added when one is actually configured.
fn auth_headers(api_key: &Option<String>) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(key) = api_key {
        if !key.is_empty() {
            if let Ok(value) = format!("Bearer {}", key).parse() {
                headers.insert(reqwest::header::AUTHORIZATION, value);
            }
        }
    }
    headers
}

// ============ Tauri Commands ============

/// Check whether an LM Studio server is reachable at `base_url`.
#[tauri::command]
pub async fn check_lmstudio_status(base_url: String) -> Result<bool, String> {
    let client = create_lmstudio_client().map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let url = build_lmstudio_url(&base_url, "/v1/models");

    match client.get(&url).send().await {
        Ok(response) => Ok(response.status().is_success()),
        Err(e) => {
            log::debug!("LM Studio status check failed: {}", e);
            Ok(false)
        }
    }
}

/// List models known to LM Studio (downloaded and/or currently loaded).
#[tauri::command]
pub async fn list_lmstudio_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<LMStudioModelInfo>, String> {
    let client = create_lmstudio_client().map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v0/models");

    let response = client
        .get(&url)
        .headers(auth_headers(&api_key))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to LM Studio: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("LM Studio returned status: {}", response.status()));
    }

    let body: LMStudioModelsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse LM Studio response: {}", e))?;

    Ok(body
        .data
        .into_iter()
        .map(|m| LMStudioModelInfo {
            id: m.id,
            model_type: m.model_type.unwrap_or_default(),
            publisher: m.publisher,
            arch: m.arch,
            compatibility_type: m.compatibility_type,
            quantization: m.quantization,
            state: m.state.unwrap_or_else(|| "unknown".to_string()),
            max_context_length: m.max_context_length,
        })
        .collect())
}

/// Download a model by catalog identifier (e.g. `"qwen2.5-7b-instruct"`) or
/// Hugging Face reference, emitting `lmstudio-download-progress` events
/// while polling the returned job until it completes or fails.
#[tauri::command]
pub async fn pull_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let download_url = build_lmstudio_url(&base_url, "/api/v1/models/download");

    log::info!("Downloading LM Studio model: {}", model_id);

    let response = client
        .post(&download_url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| format!("Failed to start model download: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Download failed: {}", error_text));
    }

    let job: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse download response: {}", e))?;

    // The model may already be on disk, in which case there's no job to poll.
    let initial_status = job["status"].as_str().unwrap_or("").to_string();
    if initial_status == "already_downloaded" || initial_status == "completed" {
        let _ = app_handle.emit("lmstudio-download-progress", LMStudioDownloadProgress {
            model_id,
            status: "completed".to_string(),
            downloaded_bytes: job["total_size_bytes"].as_u64(),
            total_size_bytes: job["total_size_bytes"].as_u64(),
        });
        return Ok(());
    }

    let job_id = job["job_id"]
        .as_str()
        .ok_or("Download response missing job_id")?
        .to_string();

    let status_url = build_lmstudio_url(
        &base_url,
        &format!("/api/v1/models/download/status/{}", job_id),
    );

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let status_response = client
            .get(&status_url)
            .headers(auth_headers(&api_key))
            .send()
            .await
            .map_err(|e| format!("Failed to poll download status: {}", e))?;

        if !status_response.status().is_success() {
            return Err(format!(
                "Download status check failed: {}",
                status_response.status()
            ));
        }

        let status_json: serde_json::Value = status_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse download status: {}", e))?;

        let status = status_json["status"].as_str().unwrap_or("").to_string();

        let _ = app_handle.emit("lmstudio-download-progress", LMStudioDownloadProgress {
            model_id: model_id.clone(),
            status: status.clone(),
            downloaded_bytes: status_json["downloaded_bytes"].as_u64(),
            total_size_bytes: status_json["total_size_bytes"].as_u64(),
        });

        match status.as_str() {
            "completed" => {
                log::info!("LM Studio model download completed: {}", model_id);
                return Ok(());
            }
            "failed" => {
                return Err(format!("Download failed for model: {}", model_id));
            }
            _ => continue, // "downloading" / "paused" -- keep polling
        }
    }
}

/// Load a downloaded model into memory so it's ready for inference.
#[tauri::command]
pub async fn load_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v1/models/load");

    let response = client
        .post(&url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| format!("Failed to load model: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Load failed: {}", error_text));
    }

    log::info!("LM Studio model loaded: {}", model_id);
    Ok(())
}

/// Unload a loaded model to free memory.
#[tauri::command]
pub async fn unload_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v1/models/unload");

    let response = client
        .post(&url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| format!("Failed to unload model: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Unload failed: {}", error_text));
    }

    log::info!("LM Studio model unloaded: {}", model_id);
    Ok(())
}
