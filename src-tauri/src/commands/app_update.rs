// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 设置页"检测最新版本"按钮用：查询 GitHub Releases，分别找出最新正式版
//! 和最新 Beta 版，连同各自 Release 页面上的更新说明一起返回给前端展示。
//! 这是纯信息查询，不涉及下载/安装（安装走 tauri-plugin-updater 默认的
//! endpoint，由前端直接调用插件的 `check()`），因此直接用总超时即可，不需要
//! 读间隔超时。
//!
//! tauri.conf.json 里的默认 endpoint 现指向 `updater-manifest-beta`（项目还
//! 没有真正的正式版之前，`updater-manifest` 那条"仅正式版"通道会因为所有
//! 发布都带 `-beta.N` 后缀被判定为 prerelease 而永远没有更新，见
//! `.github/workflows/release.yml` 的 `prerelease` 判定逻辑）。等项目发布
//! 真正的非预发布版本后，再把默认 endpoint 切回 `updater-manifest`。
//!
//! 本文件下方的 `check_and_install_beta_update` 是设置页"立即更新并安装
//! Beta"按钮专用：tauri-plugin-updater 的 JS `check()` 不支持运行时覆盖
//! endpoint（只认 tauri.conf.json 里的静态配置），所以这里在 Rust 侧用
//! `UpdaterBuilder::endpoints()` 显式指向 `updater-manifest-beta` 清单——
//! 即便默认 endpoint 现在也指向同一个清单，这个显式指定仍然保留，不依赖
//! 默认配置将来切回正式版通道后这个按钮还能正常工作。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use super::local_model::friendly_err;

const GITHUB_RELEASES_API: &str = "https://api.github.com/repos/baiyuheniao/BaiyuAISpace2/releases";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    prerelease: bool,
    draft: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestReleasesResult {
    pub current_version: String,
    pub stable: Option<ReleaseInfo>,
    pub beta: Option<ReleaseInfo>,
}

impl From<&GithubRelease> for ReleaseInfo {
    fn from(r: &GithubRelease) -> Self {
        ReleaseInfo {
            version: r.tag_name.trim_start_matches('v').to_string(),
            name: r
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| r.tag_name.clone()),
            body: r.body.clone().unwrap_or_default(),
            html_url: r.html_url.clone(),
            published_at: r.published_at.clone(),
        }
    }
}

/// 检测 GitHub 上最新的正式版和 Beta 版
#[tauri::command]
pub async fn check_latest_releases() -> Result<LatestReleasesResult, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| friendly_err("创建网络连接失败，请重启应用后重试", e))?;

    // GitHub API 要求请求带 User-Agent，否则一律拒绝
    let resp = client
        .get(GITHUB_RELEASES_API)
        .header("User-Agent", "BaiyuAISpace2")
        .header("Accept", "application/vnd.github+json")
        .query(&[("per_page", "20")])
        .send()
        .await
        .map_err(|e| friendly_err("检查更新失败，请检查网络连接", e))?;

    if !resp.status().is_success() {
        return Err(friendly_err("检查更新失败，GitHub 服务异常，请稍后重试", resp.status()));
    }

    let releases: Vec<GithubRelease> = resp
        .json()
        .await
        .map_err(|e| friendly_err("解析更新信息失败，请稍后重试", e))?;

    // 列表按发布时间从新到旧排列；跳过草稿和"仅供更新清单用"的固定 tag
    let releases: Vec<&GithubRelease> = releases
        .iter()
        .filter(|r| !r.draft && r.tag_name != "updater-manifest" && r.tag_name != "updater-manifest-beta")
        .collect();

    let stable = releases.iter().find(|r| !r.prerelease).map(|r| ReleaseInfo::from(*r));
    let beta = releases.iter().find(|r| r.prerelease).map(|r| ReleaseInfo::from(*r));

    Ok(LatestReleasesResult {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        stable,
        beta,
    })
}

const BETA_UPDATE_MANIFEST_URL: &str =
    "https://github.com/baiyuheniao/BaiyuAISpace2/releases/download/updater-manifest-beta/latest.json";

/// Beta 安装进度事件，供设置页的"立即更新并安装"按钮（Beta 分支）展示进度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BetaUpdateProgress {
    status: String,
    percent: Option<u32>,
}

fn emit_beta_progress(app_handle: &AppHandle, status: &str, percent: Option<u32>) {
    let _ = app_handle.emit(
        "beta-update-progress",
        BetaUpdateProgress { status: status.to_string(), percent },
    );
}

/// 检测并安装 Beta 版更新。tauri-plugin-updater 的 JS `check()` 只认
/// tauri.conf.json 里固定配置的稳定版 endpoint，这里改在 Rust 侧用
/// `endpoints()` 显式指向 `updater-manifest-beta` 清单，实现独立于稳定版的
/// Beta 更新通道。返回 `Ok(Some(version))` 表示已下载安装完成（前端随后需要
/// 调用 `relaunch()` 重启），`Ok(None)` 表示当前已是最新 Beta。
#[tauri::command]
pub async fn check_and_install_beta_update(app_handle: AppHandle) -> Result<Option<String>, String> {
    let endpoint = tauri::Url::parse(BETA_UPDATE_MANIFEST_URL)
        .map_err(|e| friendly_err("更新地址解析失败，请联系开发者", e))?;

    emit_beta_progress(&app_handle, "checking", None);

    let updater = app_handle
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| friendly_err("初始化更新检测失败，请重启应用后重试", e))?
        .build()
        .map_err(|e| friendly_err("初始化更新检测失败，请重启应用后重试", e))?;

    let update = updater
        .check()
        .await
        .map_err(|e| friendly_err("检查 Beta 更新失败，请检查网络连接", e))?;

    let Some(update) = update else {
        return Ok(None);
    };

    let version = update.version.clone();
    let mut downloaded: u64 = 0;
    let mut total: u64 = 0;
    let progress_handle = app_handle.clone();

    emit_beta_progress(&app_handle, "downloading", Some(0));

    update
        .download_and_install(
            move |chunk_len, content_len| {
                if let Some(len) = content_len {
                    total = len;
                }
                downloaded += chunk_len as u64;
                let percent = if total > 0 {
                    Some(((downloaded as f64 / total as f64) * 100.0).min(100.0) as u32)
                } else {
                    None
                };
                emit_beta_progress(&progress_handle, "downloading", percent);
            },
            || {},
        )
        .await
        .map_err(|e| friendly_err("下载或安装 Beta 更新失败，请检查网络连接和磁盘空间", e))?;

    emit_beta_progress(&app_handle, "finished", Some(100));

    Ok(Some(version))
}
