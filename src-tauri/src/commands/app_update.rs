// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 设置页"检测最新版本"按钮用：查询 GitHub Releases，分别找出最新正式版
//! 和最新 Beta 版，连同各自 Release 页面上的更新说明一起返回给前端展示。
//! 这是纯信息查询，不涉及下载/安装（安装走 tauri-plugin-updater 的
//! updater-manifest 端点），因此直接用总超时即可，不需要读间隔超时。

use std::time::Duration;

use serde::{Deserialize, Serialize};

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
        .map_err(|e| e.to_string())?;

    // GitHub API 要求请求带 User-Agent，否则一律拒绝
    let resp = client
        .get(GITHUB_RELEASES_API)
        .header("User-Agent", "BaiyuAISpace2")
        .header("Accept", "application/vnd.github+json")
        .query(&[("per_page", "20")])
        .send()
        .await
        .map_err(|e| format!("请求 GitHub Releases 失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub 返回错误状态: {}", resp.status()));
    }

    let releases: Vec<GithubRelease> = resp
        .json()
        .await
        .map_err(|e| format!("解析 GitHub Releases 响应失败: {e}"))?;

    // 列表按发布时间从新到旧排列；跳过草稿和"仅供更新清单用"的固定 tag
    let releases: Vec<&GithubRelease> = releases
        .iter()
        .filter(|r| !r.draft && r.tag_name != "updater-manifest")
        .collect();

    let stable = releases.iter().find(|r| !r.prerelease).map(|r| ReleaseInfo::from(*r));
    let beta = releases.iter().find(|r| r.prerelease).map(|r| ReleaseInfo::from(*r));

    Ok(LatestReleasesResult {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        stable,
        beta,
    })
}
