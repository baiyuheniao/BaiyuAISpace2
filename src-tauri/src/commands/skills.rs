// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Skill 模块
//!
//! 一个 Skill 是用户自定义的可复用能力包，包含：
//! - instructions: 一段指令文本（类似 SKILL.md 正文），激活时注入到对话的系统提示里
//! - bound_mcp_server_ids: 绑定的 MCP 服务器，激活该 Skill 时把这些服务器的工具
//!   一并带入可用工具列表（即便全局 MCP 开关是关闭的）
//! - resource_files: 关联的资源文件（类似 Claude Agent Skills 里 SKILL.md 同目录下
//!   的辅助文件），实际内容存放在 app_data/skills/<id>/resources/ 下，激活时把可
//!   读取为文本的文件内容一并附带给模型
//!
//! Skill 在 Chat 模块里有两种调用方式：
//! - 手动选择：用户在输入框旁选中某个 Skill，对该轮及后续对话生效
//! - 模型自主判断：把所有启用的 Skill 暴露成一个可被模型调用的工具
//!   (`skill__<id>`)，模型根据 name/description 自行判断要不要调用

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use thiserror::Error;
use uuid::Uuid;

use crate::db::DbState;

#[derive(Error, Debug)]
pub enum SkillError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Skill not found: {0}")]
    NotFound(String),
    #[error("Invalid skill configuration: {0}")]
    InvalidConfig(String),
    #[error("File error: {0}")]
    FileError(String),
}

impl Serialize for SkillError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// 一个 Skill 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    /// 激活该 Skill 时一并带入可用工具列表的 MCP 服务器 ID
    pub bound_mcp_server_ids: Vec<String>,
    pub enabled: bool,
    /// 关联资源文件名列表（实际内容存放在磁盘上，见 skill_resources_dir）
    pub resource_files: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Skill 资源文件存放目录: app_data/skills/<skill_id>/resources/
pub fn skill_resources_dir(app_handle: &AppHandle, skill_id: &str) -> Result<PathBuf, SkillError> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| SkillError::FileError(format!("Failed to get app data dir: {}", e)))?;
    Ok(app_data_dir.join("skills").join(skill_id).join("resources"))
}

/// 读取某个 Skill 资源文件的文本内容
/// 仅支持 UTF-8 文本文件；非文本/读取失败时返回 None（不阻断 Skill 激活）
pub async fn read_skill_resource_text(
    app_handle: &AppHandle,
    skill_id: &str,
    filename: &str,
) -> Option<String> {
    let dir = skill_resources_dir(app_handle, skill_id).ok()?;
    let path = dir.join(filename);
    tokio::fs::read_to_string(&path).await.ok()
}

// ============ Tauri Commands ============

/// 创建或更新 Skill (id 为空则创建)
#[tauri::command]
pub async fn save_skill(
    state: tauri::State<'_, DbState>,
    skill: Skill,
) -> Result<Skill, SkillError> {
    if skill.name.trim().is_empty() {
        return Err(SkillError::InvalidConfig("Skill 名称不能为空".to_string()));
    }

    let mut config = skill;
    let is_new = config.id.is_empty();
    if is_new {
        config.id = Uuid::new_v4().to_string();
        config.created_at = chrono::Utc::now().timestamp_millis();
    }
    config.updated_at = chrono::Utc::now().timestamp_millis();

    let db = state.0.lock().await;
    db.save_skill(&config)
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;

    log::info!("Skill saved: {} ({})", config.name, config.id);
    Ok(config)
}

/// 获取所有 Skill
#[tauri::command]
pub async fn list_skills(state: tauri::State<'_, DbState>) -> Result<Vec<Skill>, SkillError> {
    let db = state.0.lock().await;
    let skills = db
        .get_skills()
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;
    Ok(skills)
}

/// 删除 Skill (同时清理其资源文件目录)
#[tauri::command]
pub async fn delete_skill(
    state: tauri::State<'_, DbState>,
    skill_id: String,
    app_handle: AppHandle,
) -> Result<(), SkillError> {
    let db = state.0.lock().await;
    db.delete_skill(&skill_id)
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;
    drop(db);

    if let Ok(dir) = skill_resources_dir(&app_handle, &skill_id) {
        if let Some(skill_dir) = dir.parent() {
            let _ = tokio::fs::remove_dir_all(skill_dir).await;
        }
    }

    log::info!("Skill deleted: {}", skill_id);
    Ok(())
}

/// 添加一个资源文件到 Skill (file_path 是用户通过文件选择器选中的绝对路径)
/// 拷贝文件到 Skill 的资源目录下，并更新数据库里的 resource_files 列表
#[tauri::command]
pub async fn add_skill_resource_file(
    state: tauri::State<'_, DbState>,
    skill_id: String,
    file_path: String,
    app_handle: AppHandle,
) -> Result<Skill, SkillError> {
    let dir = skill_resources_dir(&app_handle, &skill_id)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| SkillError::FileError(format!("Failed to create resources dir: {}", e)))?;

    let source = PathBuf::from(&file_path);
    let filename = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| SkillError::FileError("Invalid file path".to_string()))?
        .to_string();

    let dest = dir.join(&filename);
    tokio::fs::copy(&source, &dest)
        .await
        .map_err(|e| SkillError::FileError(format!("Failed to copy file: {}", e)))?;

    let db = state.0.lock().await;
    let mut skills = db
        .get_skills()
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;
    let skill = skills
        .iter_mut()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| SkillError::NotFound(skill_id.clone()))?;

    if !skill.resource_files.contains(&filename) {
        skill.resource_files.push(filename);
    }
    skill.updated_at = chrono::Utc::now().timestamp_millis();

    db.save_skill(skill)
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;

    Ok(skill.clone())
}

/// 从 Skill 移除一个资源文件 (同时删除磁盘上的文件)
#[tauri::command]
pub async fn remove_skill_resource_file(
    state: tauri::State<'_, DbState>,
    skill_id: String,
    filename: String,
    app_handle: AppHandle,
) -> Result<Skill, SkillError> {
    let dir = skill_resources_dir(&app_handle, &skill_id)?;
    let path = dir.join(&filename);
    let _ = tokio::fs::remove_file(&path).await;

    let db = state.0.lock().await;
    let mut skills = db
        .get_skills()
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;
    let skill = skills
        .iter_mut()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| SkillError::NotFound(skill_id.clone()))?;

    skill.resource_files.retain(|f| f != &filename);
    skill.updated_at = chrono::Utc::now().timestamp_millis();

    db.save_skill(skill)
        .map_err(|e| SkillError::DatabaseError(e.to_string()))?;

    Ok(skill.clone())
}

/// 读取某个资源文件的文本内容 (用于前端预览)
#[tauri::command]
pub async fn read_skill_resource_file(
    skill_id: String,
    filename: String,
    app_handle: AppHandle,
) -> Result<String, SkillError> {
    read_skill_resource_text(&app_handle, &skill_id, &filename)
        .await
        .ok_or_else(|| SkillError::FileError(format!("无法读取文件（可能不是文本文件）: {}", filename)))
}
