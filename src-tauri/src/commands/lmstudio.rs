// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! LM Studio 本地服务器集成
//!
//! LM Studio 会跑自己的本地推理服务器（默认是 `http://localhost:1234`），
//! 同时提供兼容 OpenAI 的接口面（`/v1/chat/completions`、`/v1/models`）和
//! 一套原生的 v1 管理 API（`/api/v1/models/download`、`/load`、`/unload`），
//! 本模块用后者做模型管理；此外还有一个信息更丰富的 v0 列表接口
//! （`/api/v0/models`），会报告每个模型的发布者/架构/量化方式/加载状态。
//! 与 Ollama 不同，LM Studio 是闭源的 GUI 桌面应用，没有官方文档记载的
//! 无头/静默安装方式，所以本模块自始至终只跟一个已经在运行的服务器通信——
//! 它从不尝试安装或启动 LM Studio 本身。

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use super::local_model::friendly_err;

// ============ 类型定义 ============

/// LM Studio `/api/v0/models` 接口返回的模型信息。
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

/// 下发给前端的下载进度事件。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioDownloadProgress {
    pub model_id: String,
    /// "downloading" | "paused" | "completed" | "failed"
    pub status: String,
    pub downloaded_bytes: Option<u64>,
    pub total_size_bytes: Option<u64>,
}

// ============ 辅助函数 ============

fn create_lmstudio_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        // 这个客户端访问的永远是用户自己已经在运行的 LM Studio 服务器，
        // 绝不是远程主机 -- 如果按 reqwest 默认行为走系统代理，在用户为
        // 其他服务商开着全局代理模式时，会平白多绕几秒钟。
        .no_proxy()
        .build()
}

fn build_lmstudio_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{}{}", base, endpoint)
}

/// LM Studio 的本地服务器默认没有鉴权；只有用户在其设置里显式开启了
/// "Require API key" 才需要 API token，所以只有真正配置了密钥时才会加这个请求头。
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

// ============ Tauri 命令 ============

/// 检查 `base_url` 处的 LM Studio 服务器是否可达。
#[tauri::command]
pub async fn check_lmstudio_status(base_url: String) -> Result<bool, String> {
    let client = create_lmstudio_client().map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;
    let url = build_lmstudio_url(&base_url, "/v1/models");

    match client.get(&url).send().await {
        Ok(response) => Ok(response.status().is_success()),
        Err(e) => {
            log::debug!("LM Studio status check failed: {}", e);
            Ok(false)
        }
    }
}

/// 列出 LM Studio 已知的模型（已下载和/或当前已加载的）。
#[tauri::command]
pub async fn list_lmstudio_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<LMStudioModelInfo>, String> {
    let client = create_lmstudio_client().map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v0/models");

    let response = client
        .get(&url)
        .headers(auth_headers(&api_key))
        .send()
        .await
        .map_err(|e| friendly_err("无法连接到 LM Studio，请确认 LM Studio 服务器已启动", e))?;

    if !response.status().is_success() {
        return Err(friendly_err("LM Studio 返回异常状态，请检查服务是否正常运行", response.status()));
    }

    let body: LMStudioModelsResponse = response
        .json()
        .await
        .map_err(|e| friendly_err("解析 LM Studio 返回的数据失败，请确认版本是否兼容", e))?;

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

/// 通过目录标识符（如 `"qwen2.5-7b-instruct"`）或 Hugging Face 引用下载模型，
/// 在轮询返回的任务直到完成或失败的过程中，持续下发 `lmstudio-download-progress` 事件。
#[tauri::command]
pub async fn pull_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;
    let download_url = build_lmstudio_url(&base_url, "/api/v1/models/download");

    log::info!("Downloading LM Studio model: {}", model_id);

    let response = client
        .post(&download_url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| friendly_err("无法开始下载模型，请检查网络连接和 LM Studio 服务状态", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(friendly_err("模型下载失败，请确认模型标识是否正确", error_text));
    }

    let job: serde_json::Value = response
        .json()
        .await
        .map_err(|e| friendly_err("解析下载任务响应失败", e))?;

    // 模型可能已经在磁盘上了，这种情况下没有任务需要轮询。
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
        .ok_or("下载任务响应格式异常，请重试")?
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
            .map_err(|e| friendly_err("查询下载进度失败，请检查网络连接", e))?;

        if !status_response.status().is_success() {
            return Err(friendly_err(
                "查询下载进度失败，请检查 LM Studio 服务是否正常运行",
                status_response.status(),
            ));
        }

        let status_json: serde_json::Value = status_response
            .json()
            .await
            .map_err(|e| friendly_err("解析下载进度失败", e))?;

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
                return Err(format!("模型 {} 下载失败，请重试或更换模型", model_id));
            }
            _ => continue, // "downloading" / "paused" -- 继续轮询
        }
    }
}

/// 把已下载的模型加载进内存，使其可用于推理。
#[tauri::command]
pub async fn load_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v1/models/load");

    let response = client
        .post(&url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| friendly_err("加载模型失败，请确认 LM Studio 服务正在运行", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(friendly_err("加载模型失败，请检查模型是否已下载完整", error_text));
    }

    log::info!("LM Studio model loaded: {}", model_id);
    Ok(())
}

/// 卸载已加载的模型以释放内存。
#[tauri::command]
pub async fn unload_lmstudio_model(
    model_id: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let client = create_lmstudio_client().map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;
    let url = build_lmstudio_url(&base_url, "/api/v1/models/unload");

    let response = client
        .post(&url)
        .headers(auth_headers(&api_key))
        .json(&serde_json::json!({ "model": model_id }))
        .send()
        .await
        .map_err(|e| friendly_err("卸载模型失败，请确认 LM Studio 服务正在运行", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(friendly_err("卸载模型失败，请稍后重试", error_text));
    }

    log::info!("LM Studio model unloaded: {}", model_id);
    Ok(())
}
