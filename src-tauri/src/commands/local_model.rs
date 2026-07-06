// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Local model management module
//!
//! Provides commands for managing locally deployed models via Ollama API.
//! Supports multiple model sources (Ollama official, HuggingFace, ModelScope)
//! for downloading/pulling models.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Child;
use once_cell::sync::Lazy;

/// Prevent the console window that Windows would otherwise briefly flash
/// when spawning a console subprocess (e.g. `ollama.exe`) from this GUI app.
pub(crate) fn hide_console_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

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

/// 流式下载专用（模型拉取等）：总超时会在下载超过设定时长时把还在
/// 正常传输的连接掐断，因此只设读间隔超时——断流才算失败。
fn create_download_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .read_timeout(crate::commands::constants::DOWNLOAD_READ_TIMEOUT)
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
    let client = create_download_client()
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

// ============ Ollama Installation & Service Management ============

/// Global state for the managed Ollama service process
static OLLAMA_PROCESS: Lazy<Mutex<Option<Child>>> = Lazy::new(|| Mutex::new(None));

/// Ollama installation detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaInstallInfo {
    /// Whether Ollama is installed on the system
    pub installed: bool,
    /// Path to the Ollama executable (if found)
    pub install_path: Option<String>,
    /// Ollama version (if detectable)
    pub version: Option<String>,
}

/// Ollama service status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaServiceStatus {
    /// Whether the Ollama service is currently running
    pub running: bool,
    /// Whether the service was started by our application
    pub managed_by_app: bool,
}

/// Model search result from Ollama library
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSearchResult {
    /// Model name (e.g. "llama3.2")
    pub name: String,
    /// Display description
    pub description: String,
    /// Available tags (e.g. ["1b", "3b", "7b", "70b"])
    pub tags: Vec<String>,
    /// Model size info string
    pub size_info: String,
}

/// Ollama installer download progress event
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaInstallProgress {
    /// Current stage: "downloading" | "installing" | "completed" | "error"
    pub stage: String,
    /// Download progress percentage (0-100)
    pub progress_percent: u64,
    /// Downloaded bytes
    pub downloaded_bytes: u64,
    /// Total bytes (if known)
    pub total_bytes: Option<u64>,
    /// Status message
    pub message: String,
}

/// Ollama download mirror sources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaDownloadMirror {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
}

/// Get the platform-specific Ollama download filename
fn get_ollama_download_filename() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("OllamaSetup.exe", "OllamaSetup.exe")
    } else if cfg!(target_os = "macos") {
        ("Ollama-darwin.zip", "Ollama-darwin.zip")
    } else {
        ("ollama-linux-amd64.tgz", "ollama-linux-amd64.tgz")
    }
}

/// Get available Ollama download mirrors
/// Returns platform-appropriate download URLs
pub fn get_ollama_download_mirrors() -> Vec<OllamaDownloadMirror> {
    let (filename, _) = get_ollama_download_filename();
    let github_base = format!(
        "https://github.com/ollama/ollama/releases/latest/download/{}",
        filename
    );

    let mut mirrors = vec![OllamaDownloadMirror {
        id: "github".to_string(),
        name: "GitHub (官方)".to_string(),
        url: github_base.clone(),
        description: "Ollama 官方 GitHub Releases".to_string(),
    }];

    mirrors.push(OllamaDownloadMirror {
        id: "ghproxy".to_string(),
        name: "GHProxy 镜像".to_string(),
        url: format!("https://mirror.ghproxy.com/{}", github_base),
        description: "GitHub 代理镜像，国内下载速度更快".to_string(),
    });

    mirrors.push(OllamaDownloadMirror {
        id: "ghfast".to_string(),
        name: "GHFast 镜像".to_string(),
        url: format!("https://ghfast.top/{}", github_base),
        description: "GitHub 加速镜像".to_string(),
    });

    // Linux: add official install script as the recommended option
    if cfg!(target_os = "linux") {
        mirrors.insert(0, OllamaDownloadMirror {
            id: "install_script".to_string(),
            name: "官方安装脚本 (推荐)".to_string(),
            url: "https://ollama.com/install.sh".to_string(),
            description: "Ollama 官方安装脚本，自动检测系统并安装".to_string(),
        });
    }

    mirrors
}

/// Parse Ollama version from `ollama --version` output
/// Handles formats like "ollama version is 0.5.7" or just "0.5.7"
fn parse_ollama_version(output: &str) -> String {
    let trimmed = output.trim();
    // Try to extract version number from common patterns
    // "ollama version is 0.5.7" -> "0.5.7"
    if let Some(version) = trimmed.rsplit(' ').next() {
        // Check if it looks like a version number (starts with digit)
        if version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return version.to_string();
        }
    }
    // Fallback: return the whole trimmed output
    trimmed.to_string()
}

/// Detect Ollama installation on the system
/// Searches PATH and common install locations
#[tauri::command]
pub async fn detect_ollama_installation() -> Result<OllamaInstallInfo, String> {
    // Common Ollama install paths on Windows
    let search_paths: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        vec![
            PathBuf::from(format!("{}\\Programs\\Ollama\\ollama.exe", local_app_data)),
            PathBuf::from(format!("{}\\Ollama\\ollama.exe", program_files)),
            PathBuf::from("ollama.exe".to_string()), // Check PATH
        ]
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap_or_default();
        vec![
            PathBuf::from("/usr/local/bin/ollama"),
            PathBuf::from("/opt/homebrew/bin/ollama"),
            PathBuf::from(format!("{}/.ollama/bin/ollama", home)),
            PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"),
            PathBuf::from("ollama".to_string()),
        ]
    } else {
        vec![
            PathBuf::from("/usr/local/bin/ollama"),
            PathBuf::from("/usr/bin/ollama"),
            PathBuf::from(format!("{}/.ollama/bin/ollama", std::env::var("HOME").unwrap_or_default())),
            PathBuf::from("ollama".to_string()),
        ]
    };

    for path in &search_paths {
        // For PATH-based lookup (just "ollama" or "ollama.exe"), use `which`
        if path.parent().is_none() || path.as_os_str() == "ollama" || path.as_os_str() == "ollama.exe" {
            let mut cmd = tokio::process::Command::new("ollama");
            cmd.arg("--version");
            hide_console_window(&mut cmd);
            if let Ok(output) = cmd.output().await {
                if output.status.success() {
                    let version_str = parse_ollama_version(&String::from_utf8_lossy(&output.stdout));
                    return Ok(OllamaInstallInfo {
                        installed: true,
                        install_path: Some("ollama".to_string()),
                        version: Some(version_str),
                    });
                }
            }
            continue;
        }

        // For absolute paths, check if file exists
        if path.exists() {
            // Try to get version
            let mut cmd = tokio::process::Command::new(path);
            cmd.arg("--version");
            hide_console_window(&mut cmd);
            let version = cmd
                .output()
                .await
                .ok()
                .filter(|o| o.status.success())
                .map(|o| parse_ollama_version(&String::from_utf8_lossy(&o.stdout)));

            return Ok(OllamaInstallInfo {
                installed: true,
                install_path: Some(path.to_string_lossy().to_string()),
                version,
            });
        }
    }

    Ok(OllamaInstallInfo {
        installed: false,
        install_path: None,
        version: None,
    })
}

/// Start Ollama service in background
/// If Ollama is already running, returns success immediately
#[tauri::command]
pub async fn start_ollama_service(
    ollama_base_url: String,
) -> Result<OllamaServiceStatus, String> {
    // First check if already running
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/tags");
    if let Ok(response) = client.get(&url).send().await {
        if response.status().is_success() {
            return Ok(OllamaServiceStatus {
                running: true,
                managed_by_app: false,
            });
        }
    }

    // Find ollama executable
    let install_info = detect_ollama_installation().await?;
    if !install_info.installed {
        return Err("Ollama is not installed".to_string());
    }

    let ollama_path = install_info.install_path.ok_or("Cannot find Ollama executable")?;

    // Start ollama serve in background
    let mut cmd = tokio::process::Command::new(&ollama_path);
    cmd.arg("serve")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hide_console_window(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start Ollama service: {}", e))?;

    // Store the child process for later management
    {
        let mut proc = OLLAMA_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        *proc = Some(child);
    }

    log::info!("Ollama service started, waiting for it to be ready...");

    // Wait for service to become available (with timeout)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let max_retries = 30u32;
    let mut retries = 0u32;

    while retries < max_retries {
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                log::info!("Ollama service is ready");
                return Ok(OllamaServiceStatus {
                    running: true,
                    managed_by_app: true,
                });
            }
        }

        retries += 1;
    }

    // Service didn't start in time, but it might still be starting
    log::warn!("Ollama service didn't respond within timeout, but process was started");
    Ok(OllamaServiceStatus {
        running: false,
        managed_by_app: true,
    })
}

/// Stop the Ollama service process managed by our application
#[tauri::command]
pub async fn stop_ollama_service() -> Result<(), String> {
    let child = {
        let mut proc = OLLAMA_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        proc.take()
    };

    if let Some(mut child) = child {
        child.kill().await.map_err(|e| format!("Failed to stop Ollama: {}", e))?;
        log::info!("Ollama service stopped");
    }

    Ok(())
}

/// Get current Ollama service status
#[tauri::command]
pub async fn get_ollama_service_status(
    ollama_base_url: String,
) -> Result<OllamaServiceStatus, String> {
    // Check if our managed process is still alive
    let managed_by_app = {
        let mut proc = OLLAMA_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        match proc.as_mut() {
            Some(child) => {
                // Try to check if the process has exited
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        // Process has exited, clean up
                        *proc = None;
                        false
                    }
                    Ok(None) => true, // Still running
                    Err(_) => {
                        // Can't check, assume still running
                        true
                    }
                }
            }
            None => false,
        }
    };

    // Check if service is actually responding
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/tags");
    let running = match client.get(&url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    };

    Ok(OllamaServiceStatus {
        running,
        managed_by_app,
    })
}

/// Download Ollama installer with progress events
#[tauri::command]
pub async fn download_ollama(
    mirror_id: Option<String>,
    app_handle: AppHandle,
) -> Result<String, String> {
    let mirrors = get_ollama_download_mirrors();
    let mirror = mirror_id.and_then(|id| mirrors.into_iter().find(|m| m.id == id))
        .unwrap_or_else(|| get_ollama_download_mirrors().into_iter().next().unwrap());

    let download_url = mirror.url;

    log::info!("Downloading Ollama from: {}", download_url);

    // Determine save path based on platform
    let temp_dir = std::env::temp_dir();
    let (_, raw_filename) = get_ollama_download_filename();

    // For Linux install script mirror, save as .sh instead
    let actual_filename = if download_url.ends_with("/install.sh") {
        "ollama_install.sh"
    } else {
        raw_filename
    };
    let installer_path = temp_dir.join(actual_filename);

    let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
        stage: "downloading".to_string(),
        progress_percent: 0,
        downloaded_bytes: 0,
        total_bytes: None,
        message: format!("正在从 {} 下载 Ollama...", mirror.name),
    });

    let client = reqwest::Client::builder()
        .read_timeout(crate::commands::constants::DOWNLOAD_READ_TIMEOUT)
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client.get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let total_size = response.content_length();
    let mut downloaded: u64 = 0;

    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut file = tokio::fs::File::create(&installer_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut stream = response.bytes_stream();
    let mut last_progress_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        file.write_all(&chunk).await.map_err(|e| format!("Write error: {}", e))?;

        downloaded += chunk.len() as u64;
        let percent = total_size
            .map(|total| (downloaded * 100) / total)
            .unwrap_or(0);

        // Throttle progress events to avoid flooding
        if last_progress_emit.elapsed() >= Duration::from_millis(200) {
            let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
                stage: "downloading".to_string(),
                progress_percent: percent,
                downloaded_bytes: downloaded,
                total_bytes: total_size,
                message: format!("正在下载... {}%", percent),
            });
            last_progress_emit = std::time::Instant::now();
        }
    }

    file.flush().await.map_err(|e| format!("Flush error: {}", e))?;

    let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
        stage: "completed".to_string(),
        progress_percent: 100,
        downloaded_bytes: downloaded,
        total_bytes: total_size,
        message: "下载完成，准备安装...".to_string(),
    });

    log::info!("Ollama installer downloaded to: {:?}", installer_path);

    Ok(installer_path.to_string_lossy().to_string())
}

/// Run the Ollama installer
/// Platform-specific installation logic
#[tauri::command]
pub async fn install_ollama(
    installer_path: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let path = PathBuf::from(&installer_path);
    if !path.exists() {
        return Err(format!("Installer not found: {}", installer_path));
    }

    let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
        stage: "installing".to_string(),
        progress_percent: 0,
        downloaded_bytes: 0,
        total_bytes: None,
        message: "正在安装 Ollama，请等待...".to_string(),
    });

    log::info!("Installing Ollama from: {}", installer_path);

    let install_result = if cfg!(target_os = "windows") {
        install_ollama_windows(&path).await
    } else if cfg!(target_os = "macos") {
        install_ollama_macos(&path).await
    } else {
        install_ollama_linux(&path).await
    };

    match install_result {
        Ok(()) => {
            let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
                stage: "completed".to_string(),
                progress_percent: 100,
                downloaded_bytes: 0,
                total_bytes: None,
                message: "Ollama 安装完成！".to_string(),
            });
            log::info!("Ollama installed successfully");
            let _ = tokio::fs::remove_file(&installer_path).await;
            Ok(())
        }
        Err(e) => {
            let _ = app_handle.emit("ollama-install-progress", OllamaInstallProgress {
                stage: "error".to_string(),
                progress_percent: 0,
                downloaded_bytes: 0,
                total_bytes: None,
                message: format!("安装失败: {}", e),
            });
            Err(e)
        }
    }
}

/// Windows: run NSIS installer with /S flag for silent install
async fn install_ollama_windows(installer_path: &PathBuf) -> Result<(), String> {
    let output = tokio::process::Command::new(installer_path)
        .arg("/S")
        .output()
        .await
        .map_err(|e| format!("Failed to run installer: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Installer failed: {}", stderr));
    }

    Ok(())
}

/// macOS: extract zip and copy Ollama.app to /Applications
async fn install_ollama_macos(installer_path: &PathBuf) -> Result<(), String> {
    let extension = installer_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    if extension != "zip" {
        return Err(format!("Unsupported installer format on macOS: {}", extension));
    }

    let default_tmp = PathBuf::from("/tmp");
    let temp_dir = installer_path.parent().unwrap_or(&default_tmp);
    let extract_dir = temp_dir.join("ollama_extract");

    let _ = tokio::fs::remove_dir_all(&extract_dir).await;
    tokio::fs::create_dir_all(&extract_dir)
        .await
        .map_err(|e| format!("Failed to create extract dir: {}", e))?;

    // Use ditto to extract (macOS native, preserves resource forks)
    let output = tokio::process::Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(installer_path)
        .arg(&extract_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to extract zip: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Extraction failed: {}", stderr));
    }

    // Find Ollama.app in the extracted directory
    let app_path = extract_dir.join("Ollama.app");
    let app_path = if !app_path.exists() {
        // Search for it
        let find_output = tokio::process::Command::new("find")
            .arg(&extract_dir)
            .arg("-name")
            .arg("Ollama.app")
            .arg("-maxdepth")
            .arg("2")
            .output()
            .await
            .map_err(|e| format!("Failed to find Ollama.app: {}", e))?;

        let found_path = String::from_utf8_lossy(&find_output.stdout).trim().to_string();
        if found_path.is_empty() {
            return Err("Could not find Ollama.app in the downloaded archive".to_string());
        }
        PathBuf::from(&found_path)
    } else {
        app_path
    };

    // Copy to /Applications
    let dest = PathBuf::from("/Applications/Ollama.app");
    if dest.exists() {
        let _ = tokio::process::Command::new("rm")
            .arg("-rf")
            .arg(&dest)
            .output()
            .await;
    }

    let copy_output = tokio::process::Command::new("cp")
        .arg("-R")
        .arg(&app_path)
        .arg(&dest)
        .output()
        .await
        .map_err(|e| format!("Failed to copy Ollama.app: {}", e))?;

    if !copy_output.status.success() {
        let stderr = String::from_utf8_lossy(&copy_output.stderr);
        if stderr.contains("Permission denied") || stderr.contains("denied") {
            log::warn!("Permission denied copying to /Applications, trying with osascript");
            let osascript_output = tokio::process::Command::new("osascript")
                .arg("-e")
                .arg(format!(
                    "do shell script \"cp -R '{}' '/Applications/Ollama.app'\" with administrator privileges",
                    app_path.display()
                ))
                .output()
                .await
                .map_err(|e| format!("Failed to request admin privileges: {}", e))?;

            if !osascript_output.status.success() {
                let err = String::from_utf8_lossy(&osascript_output.stderr);
                return Err(format!("Failed to copy with admin privileges: {}", err));
            }
        } else {
            return Err(format!("Failed to copy Ollama.app: {}", stderr));
        }
    }

    // Clean up extraction directory
    let _ = tokio::fs::remove_dir_all(&extract_dir).await;

    // Ensure the ollama CLI symlink exists
    let ollama_symlink = PathBuf::from("/usr/local/bin/ollama");
    if !ollama_symlink.exists() {
        let cli_path = dest.join("Contents/Resources/ollama");
        if cli_path.exists() {
            let _ = tokio::process::Command::new("ln")
                .arg("-sf")
                .arg(&cli_path)
                .arg(&ollama_symlink)
                .output()
                .await;
        }
    }

    Ok(())
}

/// Linux: use install script or extract tgz to /usr/local/bin
async fn install_ollama_linux(installer_path: &PathBuf) -> Result<(), String> {
    let filename = installer_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename.ends_with(".sh") {
        // Run the official install script
        let output = tokio::process::Command::new("sh")
            .arg(installer_path)
            .output()
            .await
            .map_err(|e| format!("Failed to run install script: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Install script failed: {}", stderr));
        }
    } else if filename.ends_with(".tgz") || filename.ends_with(".tar.gz") {
        // Extract the binary to /usr/local/bin
        let output = tokio::process::Command::new("tar")
            .arg("-xzf")
            .arg(installer_path)
            .arg("-C")
            .arg("/usr/local/bin")
            .output()
            .await
            .map_err(|e| format!("Failed to extract: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Permission denied") || stderr.contains("denied") {
                log::warn!("Permission denied extracting to /usr/local/bin, trying with pkexec");
                let pkexec_output = tokio::process::Command::new("pkexec")
                    .arg("tar")
                    .arg("-xzf")
                    .arg(installer_path)
                    .arg("-C")
                    .arg("/usr/local/bin")
                    .output()
                    .await
                    .map_err(|e| format!("Failed to run with pkexec: {}", e))?;

                if !pkexec_output.status.success() {
                    let err = String::from_utf8_lossy(&pkexec_output.stderr);
                    return Err(format!("Failed to extract with elevated privileges: {}", err));
                }
            } else {
                return Err(format!("Extraction failed: {}", stderr));
            }
        }
    } else {
        return Err(format!("Unsupported installer format on Linux: {}", filename));
    }

    Ok(())
}

/// Search for models in the Ollama library
/// Uses the Ollama website search to find models matching the query
#[tauri::command]
pub async fn search_ollama_models(
    query: String,
) -> Result<Vec<ModelSearchResult>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }

    let client = std::sync::Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?
    );

    // Fetch the Ollama library search page
    let search_url = format!("https://ollama.com/library?q={}", urlencoding::encode(&query));

    let response = client.get(&search_url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| format!("Search request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Search failed with status: {}", response.status()));
    }

    let html = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;

    // Collect unique model series names from library links
    let mut series_names: Vec<String> = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for line in html.lines() {
        if let Some(start) = line.find("href=\"/library/") {
            let rest = &line[start + 15..];
            if let Some(end) = rest.find('"') {
                let model_name = &rest[..end];
                if model_name.contains('/') || model_name.contains('?')
                    || model_name.contains('#') || model_name.contains(':')
                    || model_name.is_empty()
                {
                    continue;
                }
                if seen_names.contains(model_name) {
                    continue;
                }
                seen_names.insert(model_name.to_string());
                series_names.push(model_name.to_string());
                if series_names.len() >= 10 {
                    break;
                }
            }
        }
    }

    // For each model series, concurrently fetch its tag list
    let html_ref = std::sync::Arc::new(html);
    let tasks: Vec<_> = series_names.into_iter().map(|name| {
        let client = client.clone();
        let html_ref = html_ref.clone();
        tokio::spawn(async move {
            let description = extract_nearby_text(&html_ref, &name);
            let tags = fetch_model_tags(&client, &name).await;
            (name, description, tags)
        })
    }).collect();

    let mut results = Vec::new();
    for task in tasks {
        if let Ok((name, description, tags)) = task.await {
            results.push(ModelSearchResult {
                name,
                description,
                tags,
                size_info: String::new(),
            });
        }
    }

    Ok(results)
}

/// Fetch the available tags for a model from its Ollama library page.
/// Returns a list of tag names such as ["1b", "3b", "7b", "70b"].
async fn fetch_model_tags(client: &reqwest::Client, model_name: &str) -> Vec<String> {
    let url = format!("https://ollama.com/library/{}/tags", model_name);
    let Ok(response) = client.get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await else { return vec![]; };

    if !response.status().is_success() {
        return vec![];
    }

    let Ok(html) = response.text().await else { return vec![]; };

    // The tags page lists hrefs like /library/{model}:{tag}
    // We extract only the tag part (after the colon).
    let prefix = format!("/library/{}:", model_name);
    let mut tags: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in html.lines() {
        let mut search_start = 0;
        while let Some(pos) = line[search_start..].find(&prefix) {
            let abs_pos = search_start + pos + prefix.len();
            let rest = &line[abs_pos..];
            // The tag ends at the next `"` or `/`
            let end = rest.find(|c: char| c == '"' || c == '/' || c == '?').unwrap_or(rest.len());
            let tag = rest[..end].trim();
            if !tag.is_empty() && !seen.contains(tag) {
                seen.insert(tag.to_string());
                tags.push(tag.to_string());
            }
            search_start = abs_pos + end;
            if search_start >= line.len() { break; }
        }
    }

    // Filter out low-level quantization variants (q2_K, q4_K_M, f16, etc.).
    // Keep top-level size/type tags that are meaningful for most users.
    tags.retain(|t| is_top_level_tag(t));

    tags
}

/// Returns true for tags a typical user wants to see:
///   - bare size tags: "7b", "72b", "0.5b"
///   - instruction / base / vision / code / tool variants WITHOUT a quantization suffix
///     e.g. "7b-instruct", "3b-base", "8b-code-instruct"
///   Filtered out:
///     - "latest" (alias, not a real version)
///     - quantization variants: -q2_K, -q4_0, -q4_K_M, -f16, -fp16, -q8_0, etc.
fn is_top_level_tag(tag: &str) -> bool {
    let lower = tag.to_lowercase();

    // Filter out "latest" — it's just an alias, not a real version
    if lower == "latest" {
        return false;
    }

    // Quantization suffix patterns used by Ollama
    let quant_suffixes = [
        "-q2_k", "-q3_k_s", "-q3_k_m", "-q3_k_l",
        "-q4_0", "-q4_1", "-q4_k_s", "-q4_k_m",
        "-q5_0", "-q5_1", "-q5_k_s", "-q5_k_m",
        "-q6_k", "-q8_0", "-f16", "-fp16",
        // also the bare suffixes without leading dash (shouldn't appear but be safe)
        "q2_k", "q3_k_s", "q3_k_m", "q3_k_l",
        "q4_0", "q4_1", "q4_k_s", "q4_k_m",
        "q5_0", "q5_1", "q5_k_s", "q5_k_m",
        "q6_k", "q8_0",
    ];
    !quant_suffixes.iter().any(|suffix| lower.ends_with(suffix))
}

/// Extract description text near a model name in the HTML
fn extract_nearby_text(html: &str, model_name: &str) -> String {
    let search_pattern = format!("/library/{}\"", model_name);
    if let Some(pos) = html.find(&search_pattern) {
        let nearby = &html[pos..std::cmp::min(pos + 500, html.len())];
        if let Some(start) = nearby.find(">") {
            let after_tag = &nearby[start + 1..];
            if let Some(end) = after_tag.find("<") {
                let text = after_tag[..end].trim();
                if !text.is_empty() && text != model_name {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

/// Get available Ollama download mirrors
#[tauri::command]
pub async fn get_ollama_download_mirrors_cmd() -> Result<Vec<OllamaDownloadMirror>, String> {
    Ok(get_ollama_download_mirrors())
}
