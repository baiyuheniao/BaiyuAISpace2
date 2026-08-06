/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! 外部 Agent 配置迁移。
//!
//! 本模块会在用户主动点击“自动发现”后，仅读取常见 Agent 配置目录中的已知文件；也
//! 支持读取用户在文件选择器中明确选中的路径。MCP 凭证、环境变量值和请求头绝不
//! 导入；所有导入的 MCP 默认禁用，必须由用户检查后再启用。

use crate::commands::mcp::{MCPServer, MCPServerType};
use crate::commands::skills::{skill_resources_dir, Skill};
use crate::db::DbState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use thiserror::Error;
use uuid::Uuid;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_SKILL_RESOURCES: usize = 64;
const MAX_SKILL_RESOURCE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_RULE_FILES: usize = 100;
const MAX_RULE_SCAN_DEPTH: usize = 8;
const MAX_DISCOVERED_SOURCES: usize = 100;
const MAX_SKILL_SCAN_DEPTH: usize = 3;

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("无法读取所选文件：{0}")]
    Read(String),
    #[error("配置格式无法识别：{0}")]
    Parse(String),
    #[error("导入内容无效：{0}")]
    Invalid(String),
    #[error("保存导入内容失败：{0}")]
    Save(String),
}

impl Serialize for ImportError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedMcpPreview {
    pub name: String,
    pub server_type: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub environment_variable_names: Vec<String>,
    pub credential_field_names: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedSkillPreview {
    pub name: String,
    pub description: String,
    pub resource_file_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentImportPreview {
    pub source_path: String,
    pub source_kind: String,
    pub mcp_servers: Vec<ImportedMcpPreview>,
    pub skill: Option<ImportedSkillPreview>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported_names: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuleFile {
    pub path: String,
    pub filename: String,
    pub content: String,
}

/// 自动发现只返回来源的路径和种类，不会把配置内容发送到前端。
/// 用户仍需先预览并明确确认，才会写入本应用数据库。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedImportSource {
    pub path: String,
    pub source_kind: String,
    pub import_kind: String,
    pub label: String,
}

fn read_limited(path: &Path) -> Result<String, ImportError> {
    let metadata = fs::metadata(path).map_err(|e| ImportError::Read(e.to_string()))?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(ImportError::Invalid(
            "单个配置文件不能超过 1 MB".to_string(),
        ));
    }
    fs::read_to_string(path).map_err(|e| ImportError::Read(e.to_string()))
}

fn source_kind(path: &Path) -> &'static str {
    match path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "claude_desktop_config.json" => "Claude Desktop",
        ".claude.json" => "Claude Code",
        ".mcp.json" => "Claude Code",
        "config.toml" => "Codex",
        "skill.md" => "Skill",
        _ => "通用配置",
    }
}

fn value_as_string(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn value_as_strings(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn likely_secret_name(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "authorization",
        "credential",
        "cookie",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn sanitize_args(args: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut cleaned = Vec::with_capacity(args.len());
    let mut warnings = Vec::new();
    let mut hide_next = false;
    for argument in args {
        if hide_next {
            cleaned.push("[需要重新填写的敏感参数]".to_string());
            hide_next = false;
            continue;
        }
        if likely_secret_name(&argument)
            || argument.contains("Bearer ")
            || argument.contains("api_")
        {
            warnings.push("检测到可能包含凭证的命令参数，已替换为占位符".to_string());
            if argument.starts_with('-') && !argument.contains('=') {
                cleaned.push(argument);
                hide_next = true;
            } else {
                cleaned.push("[需要重新填写的敏感参数]".to_string());
            }
        } else {
            cleaned.push(argument);
        }
    }
    (cleaned, warnings)
}

fn parse_server(name: &str, value: &serde_json::Value) -> Result<ImportedMcpPreview, ImportError> {
    let object = value
        .as_object()
        .ok_or_else(|| ImportError::Parse(format!("MCP 服务 {name} 不是对象")))?;
    let raw_type = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let command = value_as_string(object.get("command"));
    let url = object
        .get("url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let server_type = if raw_type == "sse" {
        "sse"
    } else if raw_type == "http"
        || raw_type == "streamable_http"
        || !url.as_deref().unwrap_or_default().is_empty()
    {
        "http"
    } else {
        "stdio"
    };
    if server_type == "stdio" && command.is_empty() {
        return Err(ImportError::Parse(format!("MCP 服务 {name} 缺少 command")));
    }
    if server_type != "stdio" && url.as_deref().unwrap_or_default().is_empty() {
        return Err(ImportError::Parse(format!("MCP 服务 {name} 缺少 url")));
    }
    let (args, mut warnings) = sanitize_args(value_as_strings(object.get("args")));
    let environment_variable_names: Vec<String> = object
        .get("env")
        .and_then(serde_json::Value::as_object)
        .map(|env| env.keys().cloned().collect())
        .unwrap_or_default();
    let mut credential_field_names: Vec<String> = environment_variable_names
        .iter()
        .filter(|key| likely_secret_name(key))
        .cloned()
        .collect();
    if let Some(headers) = object.get("headers").and_then(serde_json::Value::as_object) {
        credential_field_names.extend(headers.keys().cloned());
    }
    if !environment_variable_names.is_empty() {
        warnings.push("环境变量仅保留名称，值不会被导入".to_string());
    }
    if !credential_field_names.is_empty() {
        warnings.push("认证字段不会被导入，请在确认服务可信后重新配置".to_string());
    }
    Ok(ImportedMcpPreview {
        name: name.to_string(),
        server_type: server_type.to_string(),
        command,
        args,
        url,
        environment_variable_names,
        credential_field_names,
        warnings,
    })
}

fn parse_json_mcp(content: &str) -> Result<Vec<ImportedMcpPreview>, ImportError> {
    let root: serde_json::Value =
        serde_json::from_str(content).map_err(|e| ImportError::Parse(e.to_string()))?;
    let servers = root
        .get("mcpServers")
        .or_else(|| root.get("mcp_servers"))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| ImportError::Parse("没有找到 mcpServers 配置段".to_string()))?;
    servers
        .iter()
        .map(|(name, value)| parse_server(name, value))
        .collect()
}

/// Codex MCP 配置只需要读取 `[mcp_servers.<name>]` / `.env` 段中的基本标量和字符串数组。
/// 这里不用完整 TOML 解析器，既避免为了导入一个小配置面引入网络依赖，也避免接受本功能不支持的
/// TOML 表达式；遇到未知复杂表达式会留下原文警告，而不是猜测或执行它。
fn parse_toml_string(value: &str) -> String {
    let value = value.trim();
    value
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn parse_toml_string_array(value: &str) -> Vec<String> {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if character == ',' && !quoted {
            if !current.trim().is_empty() {
                values.push(parse_toml_string(&current));
            }
            current.clear();
        } else {
            current.push(character);
        }
    }
    if !current.trim().is_empty() {
        values.push(parse_toml_string(&current));
    }
    values
}

fn parse_toml_mcp(content: &str) -> Result<Vec<ImportedMcpPreview>, ImportError> {
    let mut servers: HashMap<String, serde_json::Map<String, serde_json::Value>> = HashMap::new();
    let mut current_server: Option<String> = None;
    let mut in_env = false;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let section = &line[1..line.len() - 1];
            let parts: Vec<_> = section.split('.').collect();
            if parts
                .first()
                .is_some_and(|part| *part == "mcp_servers" || *part == "mcpServers")
                && parts.len() >= 2
            {
                let name = parts[1].trim_matches('"').to_string();
                servers.entry(name.clone()).or_default();
                current_server = Some(name);
                in_env = parts.get(2).is_some_and(|part| *part == "env");
            } else {
                current_server = None;
                in_env = false;
            }
            continue;
        }
        let Some(server_name) = current_server.as_ref() else {
            continue;
        };
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = raw_value.split(" #").next().unwrap_or(raw_value).trim();
        let server = servers
            .get_mut(server_name)
            .expect("current server is inserted");
        if in_env {
            let env = server
                .entry("env".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if let Some(env) = env.as_object_mut() {
                env.insert(
                    key.to_string(),
                    serde_json::Value::String(parse_toml_string(value)),
                );
            }
        } else if key == "args" {
            server.insert(
                key.to_string(),
                serde_json::Value::Array(
                    parse_toml_string_array(value)
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        } else {
            server.insert(
                key.to_string(),
                serde_json::Value::String(parse_toml_string(value)),
            );
        }
    }
    if servers.is_empty() {
        return Err(ImportError::Parse(
            "没有找到 [mcp_servers.<名称>] 配置段".to_string(),
        ));
    }
    servers
        .iter()
        .map(|(name, server)| parse_server(name, &serde_json::Value::Object(server.clone())))
        .collect()
}

fn parse_mcp_file(path: &Path) -> Result<Vec<ImportedMcpPreview>, ImportError> {
    let content = read_limited(path)?;
    if path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
    {
        parse_toml_mcp(&content)
    } else {
        parse_json_mcp(&content)
    }
}

fn skill_root(path: &Path) -> Result<PathBuf, ImportError> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| ImportError::Invalid("Skill 文件路径无效".to_string()))?
    };
    if !root.join("SKILL.md").is_file() {
        return Err(ImportError::Invalid(
            "所选文件夹中没有 SKILL.md".to_string(),
        ));
    }
    Ok(root)
}

fn parse_frontmatter(content: &str, fallback_name: String) -> (String, String, String) {
    let mut name = fallback_name;
    let mut description = String::new();
    let mut body = content.to_string();
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let header = &rest[..end];
            for line in header.lines() {
                if let Some(value) = line.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').to_string();
                }
                if let Some(value) = line.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').to_string();
                }
            }
            body = rest[end + 4..].trim_start_matches(['\r', '\n']).to_string();
        }
    }
    if description.is_empty() {
        description = "从外部 Agent Skill 导入".to_string();
    }
    (name, description, body)
}

fn direct_skill_resources(root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| ImportError::Read(e.to_string()))? {
        let entry = entry.map_err(|e| ImportError::Read(e.to_string()))?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| ImportError::Read(e.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_SKILL_RESOURCE_BYTES {
            continue;
        }
        files.push(path);
        if files.len() >= MAX_SKILL_RESOURCES {
            break;
        }
    }
    Ok(files)
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn add_detected_mcp_source(path: PathBuf, sources: &mut Vec<DetectedImportSource>) {
    if sources.len() >= MAX_DISCOVERED_SOURCES || !path.is_file() {
        return;
    }
    // 只保留确实含有可迁移 MCP 的已知配置，避免展示无法导入的普通配置文件。
    if !matches!(parse_mcp_file(&path), Ok(servers) if !servers.is_empty()) {
        return;
    }
    sources.push(DetectedImportSource {
        label: source_kind(&path).to_string(),
        source_kind: source_kind(&path).to_string(),
        import_kind: "mcp".to_string(),
        path: path.to_string_lossy().to_string(),
    });
}

fn scan_skill_roots(dir: &Path, depth: usize, sources: &mut Vec<DetectedImportSource>) {
    if depth > MAX_SKILL_SCAN_DEPTH || sources.len() >= MAX_DISCOVERED_SOURCES {
        return;
    }
    if dir.join("SKILL.md").is_file() {
        let label = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Skill")
            .to_string();
        sources.push(DetectedImportSource {
            path: dir.to_string_lossy().to_string(),
            source_kind: "Skill".to_string(),
            import_kind: "skill".to_string(),
            label,
        });
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if sources.len() >= MAX_DISCOVERED_SOURCES {
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some(".git") | Some("node_modules") | Some("target") | Some("dist")
        ) {
            continue;
        }
        scan_skill_roots(&path, depth + 1, sources);
    }
}

#[tauri::command]
pub async fn detect_agent_import_sources() -> Vec<DetectedImportSource> {
    let Some(home) = user_home_dir() else {
        return vec![];
    };
    let mut sources = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        add_detected_mcp_source(
            PathBuf::from(app_data)
                .join("Claude")
                .join("claude_desktop_config.json"),
            &mut sources,
        );
    }
    for path in [
        home.join(".claude.json"),
        home.join(".codex").join("config.toml"),
    ] {
        add_detected_mcp_source(path, &mut sources);
    }
    for root in [
        home.join(".claude").join("skills"),
        home.join(".codex").join("skills"),
    ] {
        scan_skill_roots(&root, 0, &mut sources);
    }
    sources
}

fn is_rule_file(path: &Path) -> bool {
    matches!(
        path.file_name()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some("claude.md") | Some("agents.md")
    )
}

fn scan_rules(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MAX_RULE_SCAN_DEPTH || found.len() >= MAX_RULE_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if found.len() >= MAX_RULE_FILES {
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if is_rule_file(&path) {
                found.push(path);
            }
            continue;
        }
        if metadata.is_dir()
            && !matches!(
                path.file_name().and_then(|v| v.to_str()),
                Some("node_modules") | Some(".git") | Some("target") | Some("dist")
            )
        {
            scan_rules(&path, depth + 1, found);
        }
    }
}

#[tauri::command]
pub async fn preview_agent_import(source_path: String) -> Result<AgentImportPreview, ImportError> {
    let path = PathBuf::from(&source_path);
    let source_kind = source_kind(&path).to_string();
    if path.is_dir()
        || path
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
    {
        let root = skill_root(&path)?;
        let content = read_limited(&root.join("SKILL.md"))?;
        let fallback = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("导入技能")
            .to_string();
        let (name, description, _) = parse_frontmatter(&content, fallback);
        let resource_file_names = direct_skill_resources(&root)?
            .iter()
            .filter_map(|file| {
                file.file_name()
                    .and_then(|v| v.to_str())
                    .map(str::to_string)
            })
            .collect();
        return Ok(AgentImportPreview {
            source_path,
            source_kind: "Skill".to_string(),
            mcp_servers: vec![],
            skill: Some(ImportedSkillPreview {
                name,
                description,
                resource_file_names,
            }),
            warnings: vec![
                "只复制 SKILL.md 同级的常规文件；链接、子目录和超大文件不会导入".to_string(),
            ],
        });
    }
    let mcp_servers = parse_mcp_file(&path)?;
    let mut warnings =
        vec!["导入的 MCP 会保持禁用；请核对命令、路径和服务地址后再启用".to_string()];
    if mcp_servers.iter().any(|server| {
        !server.environment_variable_names.is_empty() || !server.credential_field_names.is_empty()
    }) {
        warnings.push(
            "环境变量值、请求头和凭证不会迁移，避免将敏感信息复制进 BaiyuAISpace2".to_string(),
        );
    }
    Ok(AgentImportPreview {
        source_path,
        source_kind,
        mcp_servers,
        skill: None,
        warnings,
    })
}

#[tauri::command]
pub async fn import_mcp_servers(
    state: tauri::State<'_, DbState>,
    source_path: String,
    selected_names: Vec<String>,
) -> Result<ImportResult, ImportError> {
    let previews = parse_mcp_file(Path::new(&source_path))?;
    let wanted: HashSet<String> = selected_names.into_iter().collect();
    let selected: Vec<_> = previews
        .into_iter()
        .filter(|server| wanted.contains(&server.name))
        .collect();
    if selected.is_empty() {
        return Err(ImportError::Invalid("请至少选择一个 MCP 服务".to_string()));
    }
    let db = state.0.lock().await;
    let mut existing: HashSet<String> = db
        .get_mcp_servers()
        .map_err(|e| ImportError::Save(e.to_string()))?
        .into_iter()
        .map(|server| server.name)
        .collect();
    let mut imported_names = Vec::new();
    let mut warnings = Vec::new();
    for preview in selected {
        let mut final_name = preview.name.clone();
        let mut suffix = 2;
        while existing.contains(&final_name) {
            final_name = format!("{}（导入 {}）", preview.name, suffix);
            suffix += 1;
        }
        let server_type = match preview.server_type.as_str() {
            "sse" => MCPServerType::SSE,
            "http" => MCPServerType::HTTP,
            _ => MCPServerType::Stdio,
        };
        let now = chrono::Utc::now().timestamp_millis();
        let server = MCPServer {
            id: Uuid::new_v4().to_string(),
            name: final_name.clone(),
            description: "从外部 Agent 配置导入。凭证未迁移，启用前请检查。".to_string(),
            server_type,
            command: preview.command,
            args: preview.args,
            env: HashMap::new(),
            port: None,
            url: preview.url,
            api_key: None,
            enabled: false,
            created_at: now,
            updated_at: now,
        };
        db.save_mcp_server(&server)
            .map_err(|e| ImportError::Save(e.to_string()))?;
        existing.insert(final_name.clone());
        imported_names.push(final_name);
        warnings.extend(preview.warnings);
    }
    warnings.sort();
    warnings.dedup();
    Ok(ImportResult {
        imported_names,
        warnings,
    })
}

#[tauri::command]
pub async fn import_skill_directory(
    state: tauri::State<'_, DbState>,
    source_path: String,
    app_handle: AppHandle,
) -> Result<ImportResult, ImportError> {
    let root = skill_root(Path::new(&source_path))?;
    let content = read_limited(&root.join("SKILL.md"))?;
    let fallback = root
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("导入技能")
        .to_string();
    let (name, description, instructions) = parse_frontmatter(&content, fallback);
    if instructions.trim().is_empty() {
        return Err(ImportError::Invalid(
            "SKILL.md 没有可用的正文指令".to_string(),
        ));
    }
    let resource_paths = direct_skill_resources(&root)?;
    let id = Uuid::new_v4().to_string();
    let resource_dir =
        skill_resources_dir(&app_handle, &id).map_err(|e| ImportError::Save(e.to_string()))?;
    fs::create_dir_all(&resource_dir).map_err(|e| ImportError::Save(e.to_string()))?;
    let mut resource_files = Vec::new();
    for source in resource_paths {
        let Some(filename) = source.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        fs::copy(&source, resource_dir.join(filename))
            .map_err(|e| ImportError::Save(e.to_string()))?;
        resource_files.push(filename.to_string());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let skill = Skill {
        id,
        name: name.clone(),
        description,
        instructions,
        bound_mcp_server_ids: vec![],
        enabled: false,
        resource_files,
        created_at: now,
        updated_at: now,
    };
    let db = state.0.lock().await;
    db.save_skill(&skill)
        .map_err(|e| ImportError::Save(e.to_string()))?;
    Ok(ImportResult {
        imported_names: vec![name],
        warnings: vec!["导入的 Skill 默认关闭，请检查指令和资源后再启用".to_string()],
    })
}

#[tauri::command]
pub async fn scan_project_rule_files(
    root_path: String,
) -> Result<Vec<ProjectRuleFile>, ImportError> {
    let root = PathBuf::from(root_path);
    if !root.is_dir() {
        return Err(ImportError::Invalid("请选择一个项目文件夹".to_string()));
    }
    let mut paths = Vec::new();
    scan_rules(&root, 0, &mut paths);
    paths
        .into_iter()
        .filter_map(|path| {
            let content = read_limited(&path).ok()?;
            Some(ProjectRuleFile {
                filename: path.file_name()?.to_str()?.to_string(),
                path: path.to_string_lossy().to_string(),
                content,
            })
        })
        .collect::<Vec<_>>()
        .pipe(Ok)
}

#[tauri::command]
pub async fn read_project_rule_file(path: String) -> Result<ProjectRuleFile, ImportError> {
    let path = PathBuf::from(path);
    if !is_rule_file(&path) {
        return Err(ImportError::Invalid(
            "只能打开 CLAUDE.md 或 AGENTS.md".to_string(),
        ));
    }
    let filename = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| ImportError::Invalid("文件路径无效".to_string()))?
        .to_string();
    Ok(ProjectRuleFile {
        content: read_limited(&path)?,
        path: path.to_string_lossy().to_string(),
        filename,
    })
}

#[tauri::command]
pub async fn save_project_rule_file(path: String, content: String) -> Result<(), ImportError> {
    let path = PathBuf::from(path);
    if !is_rule_file(&path) {
        return Err(ImportError::Invalid(
            "只能保存 CLAUDE.md 或 AGENTS.md".to_string(),
        ));
    }
    if content.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ImportError::Invalid("规则文件不能超过 1 MB".to_string()));
    }
    fs::write(path, content).map_err(|e| ImportError::Save(e.to_string()))
}

#[tauri::command]
pub async fn import_project_rule_as_skill(
    state: tauri::State<'_, DbState>,
    path: String,
) -> Result<ImportResult, ImportError> {
    let rule = read_project_rule_file(path).await?;
    let stem = Path::new(&rule.path)
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .unwrap_or("项目");
    let name = format!("{} · {}", stem, rule.filename);
    let now = chrono::Utc::now().timestamp_millis();
    let skill = Skill {
        id: Uuid::new_v4().to_string(),
        name: name.clone(),
        description: format!("从 {} 导入的项目规则", rule.filename),
        instructions: rule.content,
        bound_mcp_server_ids: vec![],
        enabled: false,
        resource_files: vec![],
        created_at: now,
        updated_at: now,
    };
    let db = state.0.lock().await;
    db.save_skill(&skill)
        .map_err(|e| ImportError::Save(e.to_string()))?;
    Ok(ImportResult {
        imported_names: vec![name],
        warnings: vec!["项目规则已转为关闭状态的 Skill 草稿；它不会回写原规则文件".to_string()],
    })
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_mcp_preview_redacts_credentials_and_preserves_only_names() {
        let preview = parse_json_mcp(
            r#"{
                "mcpServers": {
                    "github": {
                        "command": "npx",
                        "args": ["-y", "server", "--api-key", "secret-value"],
                        "env": { "GITHUB_TOKEN": "super-secret", "CACHE_DIR": "C:/cache" }
                    }
                }
            }"#,
        )
        .expect("valid MCP JSON must parse");

        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].name, "github");
        assert_eq!(
            preview[0].environment_variable_names,
            vec!["CACHE_DIR", "GITHUB_TOKEN"]
        );
        assert!(preview[0]
            .credential_field_names
            .contains(&"GITHUB_TOKEN".to_string()));
        assert!(!preview[0].args.iter().any(|arg| arg == "secret-value"));
    }

    #[test]
    fn codex_toml_preview_reads_stdio_and_remote_servers() {
        let preview = parse_toml_mcp(
            r#"
                [mcp_servers.files]
                command = "npx"
                args = ["-y", "@modelcontextprotocol/server-filesystem", "C:/work"]

                [mcp_servers.remote]
                url = "https://example.com/mcp"
                type = "streamable_http"

                [mcp_servers.files.env]
                API_TOKEN = "not-imported"
            "#,
        )
        .expect("basic Codex TOML must parse");

        let files = preview
            .iter()
            .find(|server| server.name == "files")
            .unwrap();
        let remote = preview
            .iter()
            .find(|server| server.name == "remote")
            .unwrap();
        assert_eq!(files.server_type, "stdio");
        assert_eq!(files.args[2], "C:/work");
        assert_eq!(remote.server_type, "http");
        assert_eq!(remote.url.as_deref(), Some("https://example.com/mcp"));
        assert!(files
            .credential_field_names
            .contains(&"API_TOKEN".to_string()));
    }

    #[test]
    fn skill_frontmatter_is_split_from_instructions() {
        let (name, description, instructions) = parse_frontmatter(
            "---\nname: Release notes\ndescription: Build a release summary\n---\n\n# Steps\n- Verify version",
            "fallback".to_string(),
        );

        assert_eq!(name, "Release notes");
        assert_eq!(description, "Build a release summary");
        assert_eq!(instructions, "# Steps\n- Verify version");
    }
}
