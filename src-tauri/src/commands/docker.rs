// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 用于本地 AI 模型部署的 Docker 容器管理。
//!
//! 封装 `docker` CLI，提供容器生命周期管理，
//! 以及带实时进度事件的镜像拉取功能。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::local_model::hide_console_window;

// ============ 类型定义 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerStatus {
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub ports: String,
    pub created: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerImage {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: String,
    pub created: String,
}

/// 基于 Docker 镜像的预定义 AI 模型部署方案。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerProfile {
    pub id: String,
    pub name: String,
    pub image: String,
    pub description: String,
    /// 端口映射，格式为 "host:container"，如 "11434:11434"
    pub ports: Vec<String>,
    /// 具名数据卷挂载，格式为 "volume_name:container_path"
    pub volumes: Vec<String>,
    /// 容器运行起来后，兼容 OpenAI 的 API base URL
    pub api_url: String,
    /// "ollama" 或 "openai" —— 决定聊天模块走哪条客户端路径
    pub api_type: String,
    /// 是否给 `docker run` 传 `--gpus all`
    pub gpu: bool,
}

/// `docker pull` 执行期间下发给前端的进度事件。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerPullProgress {
    pub image: String,
    /// "starting" | "pulling" | "completed" | "failed"
    pub status: String,
    pub message: String,
}

// ============ 预定义方案 ============

pub fn get_docker_profiles() -> Vec<DockerProfile> {
    vec![
        DockerProfile {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            image: "ollama/ollama:latest".to_string(),
            description: "在 Docker 中运行 Ollama，支持所有 Ollama 模型，API 完全兼容 OpenAI".to_string(),
            ports: vec!["11434:11434".to_string()],
            volumes: vec!["ollama:/root/.ollama".to_string()],
            api_url: "http://localhost:11434/v1".to_string(),
            api_type: "ollama".to_string(),
            gpu: false,
        },
        DockerProfile {
            id: "localai-cpu".to_string(),
            name: "LocalAI (CPU)".to_string(),
            image: "localai/localai:latest-aio-cpu".to_string(),
            description: "LocalAI CPU 版本，无需 GPU 即可运行，支持多种模型格式，OpenAI 兼容 API".to_string(),
            ports: vec!["8080:8080".to_string()],
            volumes: vec!["localai-models:/build/models".to_string()],
            api_url: "http://localhost:8080/v1".to_string(),
            api_type: "openai".to_string(),
            gpu: false,
        },
        DockerProfile {
            id: "localai-gpu".to_string(),
            name: "LocalAI (GPU)".to_string(),
            image: "localai/localai:latest-aio-gpu".to_string(),
            description: "LocalAI GPU 版本，需要 NVIDIA GPU 及 CUDA 驱动，推理速度更快".to_string(),
            ports: vec!["8080:8080".to_string()],
            volumes: vec!["localai-models:/build/models".to_string()],
            api_url: "http://localhost:8080/v1".to_string(),
            api_type: "openai".to_string(),
            gpu: true,
        },
    ]
}

// ============ 辅助函数 ============

/// 构建一个 `docker` 的 `tokio::process::Command`，在 Windows 上已经预先
/// 应用了不弹控制台窗口的标志位。
fn docker_cmd() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("docker");
    hide_console_window(&mut cmd);
    cmd
}

// ============ Tauri 命令 ============

/// 检查是否安装了 Docker，以及 Docker 守护进程是否可达。
#[tauri::command]
pub async fn check_docker_status() -> Result<DockerStatus, String> {
    let mut cmd = docker_cmd();
    cmd.args(["version", "--format", "{{.Client.Version}}"]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let output = match cmd.output().await {
        Err(_) => {
            return Ok(DockerStatus {
                installed: false,
                running: false,
                version: None,
            });
        }
        Ok(o) => o,
    };

    // 守护进程不可达时 `docker version` 会以非零码退出，但仍会把客户端
    // 版本号打印到 stdout。
    let version_raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let version = if version_raw.is_empty() {
        None
    } else {
        Some(version_raw)
    };

    if !output.status.success() {
        return Ok(DockerStatus {
            installed: true,
            running: false,
            version,
        });
    }

    // 用一次快速的 `docker info` 调用确认守护进程确实可达。
    let mut info_cmd = docker_cmd();
    info_cmd.args(["info", "--format", "{{.ServerVersion}}"]);
    info_cmd.stdout(std::process::Stdio::null());
    info_cmd.stderr(std::process::Stdio::null());
    let running = info_cmd
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);

    Ok(DockerStatus {
        installed: true,
        running,
        version,
    })
}

/// 列出本机上的 Docker 镜像。
#[tauri::command]
pub async fn list_docker_images() -> Result<Vec<DockerImage>, String> {
    let mut cmd = docker_cmd();
    cmd.args(["images", "--format", "{{json .}}"]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker images 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker images 出错: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let images = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(DockerImage {
                id: v["ID"].as_str().unwrap_or("").to_string(),
                repository: v["Repository"].as_str().unwrap_or("").to_string(),
                tag: v["Tag"].as_str().unwrap_or("").to_string(),
                size: v["Size"].as_str().unwrap_or("").to_string(),
                created: v["CreatedAt"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    Ok(images)
}

/// 列出 Docker 容器。当 `all` 为 true 时，也会包含已停止的容器。
#[tauri::command]
pub async fn list_docker_containers(all: Option<bool>) -> Result<Vec<DockerContainer>, String> {
    let mut cmd = docker_cmd();
    let mut args: Vec<&str> = vec!["ps", "--format", "{{json .}}"];
    if all.unwrap_or(true) {
        args.push("--all");
    }
    cmd.args(&args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker ps 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker ps 出错: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let containers = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(DockerContainer {
                id: v["ID"].as_str().unwrap_or("").to_string(),
                name: v["Names"].as_str().unwrap_or("").to_string(),
                image: v["Image"].as_str().unwrap_or("").to_string(),
                status: v["Status"].as_str().unwrap_or("").to_string(),
                ports: v["Ports"].as_str().unwrap_or("").to_string(),
                created: v["CreatedAt"].as_str().unwrap_or("").to_string(),
                state: v["State"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    Ok(containers)
}

/// 拉取一个 Docker 镜像，把 stdout/stderr 以 `docker-pull-progress` 事件的形式持续下发。
#[tauri::command]
pub async fn pull_docker_image(image: String, app_handle: AppHandle) -> Result<(), String> {
    let _ = app_handle.emit(
        "docker-pull-progress",
        DockerPullProgress {
            image: image.clone(),
            status: "starting".to_string(),
            message: format!("正在拉取镜像 {}...", image),
        },
    );

    let mut cmd = docker_cmd();
    cmd.args(["pull", &image]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 docker: {}", e))?;

    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");

    let app_out = app_handle.clone();
    let img_out = image.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if !line.is_empty() {
                let _ = app_out.emit(
                    "docker-pull-progress",
                    DockerPullProgress {
                        image: img_out.clone(),
                        status: "pulling".to_string(),
                        message: line,
                    },
                );
            }
        }
    });

    let app_err = app_handle.clone();
    let img_err = image.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if !line.is_empty() {
                let _ = app_err.emit(
                    "docker-pull-progress",
                    DockerPullProgress {
                        image: img_err.clone(),
                        status: "pulling".to_string(),
                        message: line,
                    },
                );
            }
        }
    });

    let exit_status = child
        .wait()
        .await
        .map_err(|e| format!("docker pull 执行失败: {}", e))?;
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    if exit_status.success() {
        let _ = app_handle.emit(
            "docker-pull-progress",
            DockerPullProgress {
                image: image.clone(),
                status: "completed".to_string(),
                message: "镜像拉取完成".to_string(),
            },
        );
        log::info!("Docker image pulled: {}", image);
        Ok(())
    } else {
        let msg = format!("docker pull {} 失败", image);
        let _ = app_handle.emit(
            "docker-pull-progress",
            DockerPullProgress {
                image: image.clone(),
                status: "failed".to_string(),
                message: msg.clone(),
            },
        );
        Err(msg)
    }
}

/// 根据预定义方案启动一个 Docker 容器。
///
/// 如果生成的名字对应的容器已经存在（无论已停止还是正在运行），
/// 会直接启动它而不是重新创建。返回容器 ID。
#[tauri::command]
pub async fn start_docker_container(
    profile_id: String,
    container_name: Option<String>,
) -> Result<String, String> {
    let profiles = get_docker_profiles();
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("未知的 Docker 部署方案: {}", profile_id))?;

    let name = container_name.unwrap_or_else(|| format!("baiyu-{}", profile_id));

    // 通过列出全部容器并匹配 Names 字段，检查这个名字的容器是否已经存在。
    let existing_id: String = {
        let mut cmd = docker_cmd();
        cmd.args(["ps", "-a", "--format", "{{json .}}"]);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        match cmd.output().await {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .find(|v| v["Names"].as_str() == Some(name.as_str()))
                    .and_then(|v| v["ID"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default()
            }
            _ => String::new(),
        }
    };

    if !existing_id.is_empty() {
        let mut cmd = docker_cmd();
        cmd.args(["start", &existing_id]);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());
        let out = cmd
            .output()
            .await
            .map_err(|e| format!("docker start 失败: {}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("docker start 出错: {}", stderr));
        }
        log::info!("Docker container '{}' restarted ({})", name, existing_id);
        return Ok(existing_id);
    }

    // 构建 `docker run` 的参数。
    let mut args: Vec<String> = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        name.clone(),
        "--restart".to_string(),
        "unless-stopped".to_string(),
    ];

    if profile.gpu {
        args.push("--gpus".to_string());
        args.push("all".to_string());
    }

    for port in &profile.ports {
        args.push("-p".to_string());
        args.push(port.clone());
    }

    for volume in &profile.volumes {
        args.push("-v".to_string());
        args.push(volume.clone());
    }

    args.push(profile.image.clone());

    let mut cmd = docker_cmd();
    cmd.args(&args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker run 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker run 出错: {}", stderr));
    }

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    log::info!(
        "Docker container '{}' created: {}",
        name,
        &container_id[..8.min(container_id.len())]
    );
    Ok(container_id)
}

/// 停止一个正在运行的 Docker 容器。
#[tauri::command]
pub async fn stop_docker_container(container_id: String) -> Result<(), String> {
    let mut cmd = docker_cmd();
    cmd.args(["stop", &container_id]);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker stop 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker stop 出错: {}", stderr));
    }

    log::info!("Docker container '{}' stopped", container_id);
    Ok(())
}

/// 强制删除一个 Docker 容器（无论它是已停止还是正在运行都能用）。
#[tauri::command]
pub async fn remove_docker_container(container_id: String) -> Result<(), String> {
    let mut cmd = docker_cmd();
    cmd.args(["rm", "-f", &container_id]);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker rm 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker rm 出错: {}", stderr));
    }

    log::info!("Docker container '{}' removed", container_id);
    Ok(())
}

/// 返回预定义的 Docker AI 部署方案列表。
#[tauri::command]
pub fn get_docker_profiles_cmd() -> Vec<DockerProfile> {
    get_docker_profiles()
}
