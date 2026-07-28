//! 内置文件工具：授权必须在后端执行，绝不信任模型给出的路径。
use crate::commands::mcp::MCPTool;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_access_cannot_escape_or_write_from_readonly_rule() {
        let root = std::env::temp_dir().join(format!("baiyu-file-tools-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let writable = vec![FileAccessRule { root: root.clone(), write: true }];
        let write = execute(&writable, "baiyu_file_write", &json!({ "path": "src/a.txt", "content": "ok" }));
        assert!(write.get("error").is_none());
        assert_eq!(fs::read_to_string(root.join("src/a.txt")).unwrap(), "ok");
        assert!(execute(&writable, "baiyu_file_read", &json!({ "path": "../outside.txt" })).get("error").is_some());
        let readonly = vec![FileAccessRule { root: root.clone(), write: false }];
        assert!(execute(&readonly, "baiyu_file_write", &json!({ "path": "blocked.txt", "content": "no" })).get("error").is_some());
        fs::remove_dir_all(root).unwrap();
    }
}

const MAX_READ_BYTES: u64 = 1 * 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 800;
const MAX_SEARCH_RESULTS: usize = 100;

#[derive(Clone, Debug)]
pub struct FileAccessRule { pub root: PathBuf, pub write: bool }

pub fn rules_for_directory(directory: Option<&str>, mode: &str) -> Vec<FileAccessRule> {
    if mode == "none" { return vec![]; }
    directory.map(|d| FileAccessRule { root: PathBuf::from(d), write: mode == "write" }).into_iter().collect()
}

pub fn tool_defs(rules: &[FileAccessRule]) -> Vec<MCPTool> {
    if rules.is_empty() { return vec![]; }
    let readonly = vec![def("baiyu_file_list", "列出授权目录中的文件和目录", json!({"path":{"type":"string","description":"相对授权目录的路径，省略为根目录"}})),
        def("baiyu_file_search", "按文件名或文本搜索授权目录", json!({"query":{"type":"string"},"path":{"type":"string"},"content":{"type":"boolean","description":"是否搜索文本内容，默认 false"}})),
        def("baiyu_file_read", "读取 UTF-8 文本文件；二进制文件会明确说明", json!({"path":{"type":"string"}})),
        def("baiyu_file_info", "查看文件或目录元数据", json!({"path":{"type":"string"}}))];
    if rules.iter().any(|r| r.write) { readonly.into_iter().chain(vec![
        def("baiyu_file_write", "创建或覆盖授权范围内的文本文件", json!({"path":{"type":"string"},"content":{"type":"string"}})),
        def("baiyu_file_edit", "精确替换文件中的指定文本；匹配零次或多次会拒绝", json!({"path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"}})),
        def("baiyu_file_create_directory", "创建目录", json!({"path":{"type":"string"}})),
        def("baiyu_file_move", "移动或重命名，默认不覆盖目标", json!({"from":{"type":"string"},"to":{"type":"string"}})),
        def("baiyu_file_delete", "将文件或目录移入授权根目录的可恢复回收目录", json!({"path":{"type":"string"}})),
    ]).collect() } else { readonly }
}
fn def(name: &str, description: &str, properties: Value) -> MCPTool { MCPTool { server_id:"baiyu-file-tools".into(), server_name:"内置文件工具".into(), name:name.into(), description:description.into(), input_schema:json!({"type":"object","properties":properties}) } }

pub fn execute(rules: &[FileAccessRule], name: &str, args: &Value) -> Value {
    let path = |key| args.get(key).and_then(Value::as_str).unwrap_or("");
    let read = |p: &str| resolve(rules, p, false);
    let write = |p: &str| resolve(rules, p, true);
    let result: Result<Value, String> = (|| match name {
        "baiyu_file_list" => list(read(path("path"))?),
        "baiyu_file_info" => info(read(path("path"))?),
        "baiyu_file_read" => read_text(read(path("path"))?),
        "baiyu_file_search" => search(read(path("path"))?, path("query"), args.get("content").and_then(Value::as_bool).unwrap_or(false)),
        "baiyu_file_write" => { let p=write(path("path"))?; write_text(p, args.get("content").and_then(Value::as_str).unwrap_or("")) },
        "baiyu_file_edit" => edit(write(path("path"))?, args.get("old_text").and_then(Value::as_str).unwrap_or(""), args.get("new_text").and_then(Value::as_str).unwrap_or("")),
        "baiyu_file_create_directory" => { let p=write(path("path"))?; fs::create_dir_all(&p).map_err(|e|e.to_string()).map(|_|json!({"status":"created","path":p})) },
        "baiyu_file_move" => { let from=write(path("from"))?; let to=write(path("to"))?; if to.exists(){Err("目标已存在；内置文件工具默认不覆盖".into())}else{fs::rename(&from,&to).map_err(|e|e.to_string()).map(|_|json!({"status":"moved","from":from,"to":to}))} },
        "baiyu_file_delete" => delete_recoverable(write(path("path"))?, rules),
        _ => Err("未知的内置文件工具".into()),
    })(); result.unwrap_or_else(|error| json!({"error":error, "allowed_roots": rules.iter().map(|r|r.root.display().to_string()).collect::<Vec<_>>() }))
}

fn resolve(rules:&[FileAccessRule], raw:&str, write:bool)->Result<PathBuf,String>{
 if raw.is_empty(){return Err("path 不能为空".into())}; let input=Path::new(raw); if input.components().any(|c|matches!(c,Component::ParentDir)){return Err("不允许使用 .. 越出授权目录".into())};
 for rule in rules.iter().filter(|r|!write||r.write){ let root=fs::canonicalize(&rule.root).map_err(|_|format!("授权目录不可用：{}",rule.root.display()))?; let candidate=if input.is_absolute(){input.to_path_buf()}else{root.join(input)}; let existing=nearest_existing(&candidate)?; let canon=fs::canonicalize(&existing).map_err(|e|e.to_string())?; if canon.starts_with(&root){return Ok(candidate)} }
 Err("路径不在当前授权目录内，或当前权限不允许写入".into())
}
fn nearest_existing(path:&Path)->Result<PathBuf,String>{let mut p=path; loop{if p.exists(){return Ok(p.to_path_buf())} p=p.parent().ok_or_else(||"无法解析路径".to_string())?;}}
fn list(p:PathBuf)->Result<Value,String>{let mut entries=Vec::new();for e in fs::read_dir(&p).map_err(|e|e.to_string())?.take(MAX_LIST_ENTRIES){let e=e.map_err(|e|e.to_string())?;let m=e.metadata().map_err(|e|e.to_string())?;entries.push(json!({"name":e.file_name().to_string_lossy(),"path":e.path(),"kind":if m.is_dir(){"directory"}else{"file"},"size":m.len()}));}Ok(json!({"path":p,"entries":entries,"limited":entries.len()>=MAX_LIST_ENTRIES}))}
fn info(p:PathBuf)->Result<Value,String>{let m=fs::metadata(&p).map_err(|e|e.to_string())?;Ok(json!({"path":p,"kind":if m.is_dir(){"directory"}else{"file"},"size":m.len(),"readonly":m.permissions().readonly()}))}
fn read_text(p:PathBuf)->Result<Value,String>{let m=fs::metadata(&p).map_err(|e|e.to_string())?;if m.len()>MAX_READ_BYTES{return Err(format!("文件过大（{} 字节），默认上限 {} 字节",m.len(),MAX_READ_BYTES))};let b=fs::read(&p).map_err(|e|e.to_string())?;match String::from_utf8(b){Ok(content)=>Ok(json!({"path":p,"content":content})),Err(_)=>Ok(json!({"path":p,"binary":true,"message":"文件为二进制，未读取文本内容"}))}}
fn write_text(p:PathBuf,content:&str)->Result<Value,String>{if let Some(parent)=p.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?};fs::write(&p,content).map_err(|e|e.to_string()).map(|_|json!({"status":"written","path":p,"bytes":content.len()}))}
fn edit(p:PathBuf,old:&str,new:&str)->Result<Value,String>{if old.is_empty(){return Err("old_text 不能为空".into())};let s=fs::read_to_string(&p).map_err(|e|e.to_string())?;let n=s.matches(old).count();if n!=1{return Err(format!("需要精确匹配一次，实际匹配 {} 次",n))};write_text(p,&s.replacen(old,new,1))}
fn search(root:PathBuf,q:&str,content:bool)->Result<Value,String>{if q.is_empty(){return Err("query 不能为空".into())};let canonical_root=fs::canonicalize(&root).map_err(|e|e.to_string())?;let mut out=Vec::new();walk(&root,&canonical_root,q,content,&mut out)?;Ok(json!({"query":q,"results":out,"limited":out.len()>=MAX_SEARCH_RESULTS}))}
fn walk(p:&Path,root:&Path,q:&str,content:bool,out:&mut Vec<Value>)->Result<(),String>{if out.len()>=MAX_SEARCH_RESULTS{return Ok(())};for e in fs::read_dir(p).map_err(|e|e.to_string())?{let e=e.map_err(|e|e.to_string())?;let path=e.path();let meta=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;if meta.file_type().is_symlink(){continue} if meta.is_dir(){let real=fs::canonicalize(&path).map_err(|e|e.to_string())?;if real.starts_with(root){walk(&path,root,q,content,out)?}}else{let name=e.file_name().to_string_lossy().to_string();let matched=name.to_lowercase().contains(&q.to_lowercase())|| (content&&fs::read_to_string(&path).map(|s|s.contains(q)).unwrap_or(false));if matched{out.push(json!({"path":path,"name":name}))}}}Ok(())}
fn delete_recoverable(p:PathBuf,rules:&[FileAccessRule])->Result<Value,String>{let root=rules.iter().find(|r|r.write&&p.starts_with(&r.root)).ok_or_else(||"没有可写授权根目录".to_string())?;let trash=root.root.join(".baiyu-file-trash");fs::create_dir_all(&trash).map_err(|e|e.to_string())?;let dest=trash.join(format!("{}-{}",chrono::Utc::now().timestamp_millis(),p.file_name().unwrap_or_default().to_string_lossy()));fs::rename(&p,&dest).map_err(|e|e.to_string()).map(|_|json!({"status":"moved_to_trash","original":p,"trash_path":dest}))}
