// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 本地模型管理模块
//!
//! 提供通过 Ollama API 管理本地部署模型的命令。
//! 支持从多个模型源（Ollama 官方、HuggingFace、ModelScope）
//! 下载/拉取模型。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::process::Child;
use once_cell::sync::Lazy;

/// 防止从这个 GUI 应用启动控制台子进程（例如 `ollama.exe`）时，
/// Windows 原本会一闪而过弹出的控制台窗口。
pub(crate) fn hide_console_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// ============ 类型定义 ============

/// Ollama 提供的本地可用模型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelInfo {
    pub name: String,
    /// 模型显示名称（如 "llama3:latest"）
    pub model: String,
    /// 修改时间戳
    pub modified_at: String,
    /// 模型大小（字节）
    pub size: u64,
    /// 模型摘要哈希
    pub digest: String,
    /// 模型详情（family、参数量、量化等级）
    pub details: Option<ModelDetails>,
}

/// 模型详细信息
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

/// Ollama 列出模型接口的响应
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

/// Ollama 模型详情（show）接口的响应
#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    details: Option<ModelDetails>,
    // 其余我们用不到的字段
    #[allow(dead_code)]
    license: Option<String>,
    #[allow(dead_code)]
    modelfile: Option<String>,
    #[allow(dead_code)]
    parameters: Option<String>,
    #[allow(dead_code)]
    template: Option<String>,
}

/// 模型源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSource {
    /// 模型源的唯一标识
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 模型仓库的 base URL
    pub base_url: String,
    /// 描述
    pub description: String,
}

/// 下发给前端的下载进度事件
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model_name: String,
    pub status: String,
    pub digest: String,
    pub total: Option<u64>,
    pub completed: Option<u64>,
}

/// 拉取模型请求的参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullModelRequest {
    /// 模型名称（如 "llama3:latest" 或 "huggingface/user/model:tag"）
    pub model_name: String,
    /// 从哪个源拉取（可选，不填则用已配置的默认源）
    pub source_id: Option<String>,
    /// 是否使用非安全连接
    pub insecure: Option<bool>,
}

/// 删除请求的参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteModelRequest {
    /// 要删除的模型名称
    pub model_name: String,
}

/// 本地模型服务的配置
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelConfig {
    /// Ollama 服务的 base URL
    pub ollama_base_url: String,
    /// 默认模型源 ID
    pub default_source_id: String,
}

/// 预定义的模型源
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

// ============ 辅助函数 ============

/// 为 Ollama API 调用创建 HTTP 客户端。这个客户端访问的永远是用户自己的
/// Ollama 服务器（本机或局域网），绝不是远程 SaaS 端点，所以这里排除了
/// 系统代理 -- 否则为了访问境外服务商而开的全局代理模式，也会把这部分
/// 本地流量一并绕过去。
fn create_ollama_client(_base_url: &str) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
}

/// 流式下载专用（模型拉取等）：总超时会在下载超过设定时长时把还在
/// 正常传输的连接掐断，因此只设读间隔超时——断流才算失败。同样只连
/// 本地 Ollama 的 /api/pull，绕开系统代理。
fn create_download_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .read_timeout(crate::commands::constants::DOWNLOAD_READ_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
}

/// 根据 base URL 和端点拼出 Ollama API 的完整 URL
fn build_ollama_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{}{}", base, endpoint)
}

// ============ Tauri 命令 ============

/// 检查 Ollama 服务是否正在运行且可访问
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

/// 列出 Ollama 上所有本地可用的模型
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

/// 获取指定本地模型的详细信息
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

/// 从指定源拉取（下载）模型
/// 会向前端下发下载进度事件
#[tauri::command]
pub async fn pull_local_model(
    request: PullModelRequest,
    ollama_base_url: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let client = create_download_client()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let url = build_ollama_url(&ollama_base_url, "/api/pull");

    // 按需给模型名加上来源前缀
    let model_ref = match request.source_id.as_deref() {
        Some("huggingface") => {
            // HuggingFace 格式：hf.co/user/model:tag 或 hf.co/user/model
            if !request.model_name.starts_with("hf.co/") {
                format!("hf.co/{}", request.model_name)
            } else {
                request.model_name.clone()
            }
        }
        Some("modelscope") => {
            // ModelScope 格式：ms://user/model，或者直接是模型名
            // Ollama 支持直接用模型名从 ModelScope 拉取
            request.model_name.clone()
        }
        _ => {
            // 默认：Ollama 官方仓库
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

    // 处理流式响应，持续更新进度
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

/// 从 Ollama 删除一个本地模型
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

/// 获取可用模型源列表
#[tauri::command]
pub async fn get_model_sources_cmd() -> Result<Vec<ModelSource>, String> {
    Ok(get_model_sources())
}

/// 获取 Ollama 版本信息
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

// ============ Ollama 安装与服务管理 ============

/// 被托管的 Ollama 服务进程的全局状态
static OLLAMA_PROCESS: Lazy<Mutex<Option<Child>>> = Lazy::new(|| Mutex::new(None));

/// Ollama 安装检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaInstallInfo {
    /// 系统上是否已安装 Ollama
    pub installed: bool,
    /// Ollama 可执行文件路径（如果找到）
    pub install_path: Option<String>,
    /// Ollama 版本（如果能检测到）
    pub version: Option<String>,
}

/// Ollama 服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaServiceStatus {
    /// Ollama 服务当前是否在运行
    pub running: bool,
    /// 该服务是否是本应用启动的
    pub managed_by_app: bool,
}

/// 来自 Ollama 库的模型搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSearchResult {
    /// 模型名称（如 "llama3.2"）
    pub name: String,
    /// 展示用的描述
    pub description: String,
    /// 可用标签（如 ["1b", "3b", "7b", "70b"]）
    pub tags: Vec<String>,
    /// 模型体积信息字符串
    pub size_info: String,
}

/// Ollama 安装包下载进度事件
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaInstallProgress {
    /// 当前阶段："downloading" | "installing" | "completed" | "error"
    pub stage: String,
    /// 下载进度百分比 (0-100)
    pub progress_percent: u64,
    /// 已下载字节数
    pub downloaded_bytes: u64,
    /// 总字节数（如果已知）
    pub total_bytes: Option<u64>,
    /// 状态消息
    pub message: String,
}

/// Ollama 下载镜像源
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaDownloadMirror {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
}

/// 获取当前平台对应的 Ollama 安装包文件名
fn get_ollama_download_filename() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("OllamaSetup.exe", "OllamaSetup.exe")
    } else if cfg!(target_os = "macos") {
        ("Ollama-darwin.zip", "Ollama-darwin.zip")
    } else {
        ("ollama-linux-amd64.tgz", "ollama-linux-amd64.tgz")
    }
}

/// 获取可用的 Ollama 下载镜像源
/// 返回适配当前平台的下载 URL
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

    // Linux：把官方安装脚本加为推荐选项
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

/// 从 `ollama --version` 的输出中解析出版本号
/// 兼容 "ollama version is 0.5.7" 这类格式，或者直接就是 "0.5.7"
fn parse_ollama_version(output: &str) -> String {
    let trimmed = output.trim();
    // 尝试从常见格式中提取版本号
    // "ollama version is 0.5.7" -> "0.5.7"
    if let Some(version) = trimmed.rsplit(' ').next() {
        // 检查它是否像个版本号（以数字开头）
        if version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return version.to_string();
        }
    }
    // 兜底：返回整个 trim 过的输出
    trimmed.to_string()
}

/// 检测系统上是否安装了 Ollama
/// 会搜索 PATH 以及常见的安装位置
#[tauri::command]
pub async fn detect_ollama_installation() -> Result<OllamaInstallInfo, String> {
    // Windows 上常见的 Ollama 安装路径
    let search_paths: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        vec![
            PathBuf::from(format!("{}\\Programs\\Ollama\\ollama.exe", local_app_data)),
            PathBuf::from(format!("{}\\Ollama\\ollama.exe", program_files)),
            PathBuf::from("ollama.exe".to_string()), // 检查 PATH
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
        // 对于基于 PATH 的查找（只有 "ollama" 或 "ollama.exe"），用 `which` 的方式来查
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

        // 对于绝对路径，检查文件是否存在
        if path.exists() {
            // 尝试获取版本号
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

/// 在后台启动 Ollama 服务
/// 如果 Ollama 已经在运行，直接返回成功
#[tauri::command]
pub async fn start_ollama_service(
    ollama_base_url: String,
) -> Result<OllamaServiceStatus, String> {
    // 先检查是否已经在运行
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
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

    // 查找 ollama 可执行文件
    let install_info = detect_ollama_installation().await?;
    if !install_info.installed {
        return Err("Ollama is not installed".to_string());
    }

    let ollama_path = install_info.install_path.ok_or("Cannot find Ollama executable")?;

    // 在后台启动 ollama serve
    let mut cmd = tokio::process::Command::new(&ollama_path);
    cmd.arg("serve")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hide_console_window(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start Ollama service: {}", e))?;

    // 保存子进程句柄，供后续管理使用
    {
        let mut proc = OLLAMA_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        *proc = Some(child);
    }

    log::info!("Ollama service started, waiting for it to be ready...");

    // 等待服务变为可用（带超时）
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
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

    // 服务没有在规定时间内起来，但可能仍在启动中
    log::warn!("Ollama service didn't respond within timeout, but process was started");
    Ok(OllamaServiceStatus {
        running: false,
        managed_by_app: true,
    })
}

/// 停止由本应用托管的 Ollama 服务进程
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

/// 获取当前 Ollama 服务状态
#[tauri::command]
pub async fn get_ollama_service_status(
    ollama_base_url: String,
) -> Result<OllamaServiceStatus, String> {
    // 检查我们托管的进程是否还活着
    let managed_by_app = {
        let mut proc = OLLAMA_PROCESS.lock().map_err(|e| format!("Lock error: {}", e))?;
        match proc.as_mut() {
            Some(child) => {
                // 尝试检查进程是否已退出
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        // 进程已退出，清理掉
                        *proc = None;
                        false
                    }
                    Ok(None) => true, // 仍在运行
                    Err(_) => {
                        // 无法检查，假定仍在运行
                        true
                    }
                }
            }
            None => false,
        }
    };

    // 检查服务是否真的有响应
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .no_proxy()
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

/// 下载 Ollama 安装包，同时下发进度事件
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

    // 根据平台确定保存路径
    let temp_dir = std::env::temp_dir();
    let (_, raw_filename) = get_ollama_download_filename();

    // 如果是 Linux 的安装脚本镜像，改存为 .sh
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

        // 对进度事件做节流，避免刷屏
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

/// 运行 Ollama 安装程序
/// 按平台执行对应的安装逻辑
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

/// Windows：带 /S 参数静默运行 NSIS 安装程序
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

/// macOS：解压 zip 并把 Ollama.app 拷贝到 /Applications
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

    // 用 ditto 解压（macOS 原生工具，能保留 resource fork）
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

    // 在解压出来的目录里找 Ollama.app
    let app_path = extract_dir.join("Ollama.app");
    let app_path = if !app_path.exists() {
        // 搜索它
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

    // 拷贝到 /Applications
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

    // 清理解压目录
    let _ = tokio::fs::remove_dir_all(&extract_dir).await;

    // 确保 ollama CLI 的软链接存在
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

/// Linux：使用安装脚本，或者把 tgz 解压到 /usr/local/bin
async fn install_ollama_linux(installer_path: &PathBuf) -> Result<(), String> {
    let filename = installer_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename.ends_with(".sh") {
        // 运行官方安装脚本
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
        // 把可执行文件解压到 /usr/local/bin
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

/// 在 Ollama 库中搜索模型
/// 使用 Ollama 网站的搜索功能来找匹配关键词的模型
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

    // 拉取 Ollama 库的搜索页面
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

    // 从库链接里收集不重复的模型系列名称
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

    // 对每个模型系列，并发拉取它的标签列表
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

/// 从模型在 Ollama 库中的页面获取可用标签。
/// 返回类似 ["1b", "3b", "7b", "70b"] 这样的标签名列表。
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

    // 标签页面列出的 href 形如 /library/{model}:{tag}
    // 这里只提取冒号后面的标签部分。
    let prefix = format!("/library/{}:", model_name);
    let mut tags: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in html.lines() {
        let mut search_start = 0;
        while let Some(pos) = line[search_start..].find(&prefix) {
            let abs_pos = search_start + pos + prefix.len();
            let rest = &line[abs_pos..];
            // 标签在遇到下一个 `"` 或 `/` 处结束
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

    // 过滤掉底层的量化变体标签（q2_K、q4_K_M、f16 等）。
    // 只保留对大多数用户有意义的顶层体积/类型标签。
    tags.retain(|t| is_top_level_tag(t));

    tags
}

/// 对典型用户想看到的标签返回 true：
///   - 单纯的体积标签："7b"、"72b"、"0.5b"
///   - 不带量化后缀的 instruction / base / vision / code / tool 变体，
///     例如 "7b-instruct"、"3b-base"、"8b-code-instruct"
///   会被过滤掉的：
///     - "latest"（只是个别名，不是真实版本号）
///     - 量化变体：-q2_K、-q4_0、-q4_K_M、-f16、-fp16、-q8_0 等
fn is_top_level_tag(tag: &str) -> bool {
    let lower = tag.to_lowercase();

    // 过滤掉 "latest" —— 它只是个别名，不是真实版本号
    if lower == "latest" {
        return false;
    }

    // Ollama 使用的量化后缀模式
    let quant_suffixes = [
        "-q2_k", "-q3_k_s", "-q3_k_m", "-q3_k_l",
        "-q4_0", "-q4_1", "-q4_k_s", "-q4_k_m",
        "-q5_0", "-q5_1", "-q5_k_s", "-q5_k_m",
        "-q6_k", "-q8_0", "-f16", "-fp16",
        // 以及不带前导横线的裸后缀（正常不该出现，但以防万一）
        "q2_k", "q3_k_s", "q3_k_m", "q3_k_l",
        "q4_0", "q4_1", "q4_k_s", "q4_k_m",
        "q5_0", "q5_1", "q5_k_s", "q5_k_m",
        "q6_k", "q8_0",
    ];
    !quant_suffixes.iter().any(|suffix| lower.ends_with(suffix))
}

/// 从 HTML 中提取模型名称附近的描述文本
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

/// 获取可用的 Ollama 下载镜像源
#[tauri::command]
pub async fn get_ollama_download_mirrors_cmd() -> Result<Vec<OllamaDownloadMirror>, String> {
    Ok(get_ollama_download_mirrors())
}
