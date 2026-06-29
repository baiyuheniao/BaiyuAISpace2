// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use crate::commands::local_model::hide_console_window;
use sha2::{Digest, Sha256};
use std::path::Path;

/// 支持的文档格式枚举
#[derive(Debug, Clone, Copy)]
pub enum DocumentFormat {
    Pdf,
    Word,
    Pptx,
    Excel,
    Markdown,
    Html,
    Txt,
}

#[allow(dead_code)]
impl DocumentFormat {
    /// 根据文件扩展名获取文档格式
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "pdf" => Some(DocumentFormat::Pdf),
            "docx" => Some(DocumentFormat::Word),
            "pptx" => Some(DocumentFormat::Pptx),
            "xlsx" | "xls" | "csv" => Some(DocumentFormat::Excel),
            "md" | "markdown" => Some(DocumentFormat::Markdown),
            "html" | "htm" => Some(DocumentFormat::Html),
            "txt" | "text" | "rs" | "js" | "ts" | "py" | "java" | "c" | "cpp" | "h" | "go" => {
                Some(DocumentFormat::Txt)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentFormat::Pdf => "pdf",
            DocumentFormat::Word => "docx",
            DocumentFormat::Pptx => "pptx",
            DocumentFormat::Excel => "xlsx",
            DocumentFormat::Markdown => "md",
            DocumentFormat::Html => "html",
            DocumentFormat::Txt => "txt",
        }
    }
}

/// 解析文档内容为纯文本
pub async fn parse_document(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let path = Path::new(file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let format = DocumentFormat::from_extension(&ext).ok_or_else(|| {
        let hint = match ext.as_str() {
            "doc" => "不支持旧版 .doc 格式，请在 Word 中另存为 .docx 后重新导入".to_string(),
            "ppt" => "不支持旧版 .ppt 格式，请在 PowerPoint 中另存为 .pptx 后重新导入".to_string(),
            other => format!("不支持的格式: .{}", other),
        };
        KnowledgeBaseError::DocumentParseError(hint)
    })?;

    let content = match format {
        DocumentFormat::Pdf => parse_pdf(file_path).await?,
        DocumentFormat::Word => parse_word(file_path).await?,
        DocumentFormat::Pptx => parse_pptx(file_path).await?,
        DocumentFormat::Excel => parse_excel(file_path).await?,
        DocumentFormat::Html => {
            let raw = tokio::fs::read_to_string(file_path)
                .await
                .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;
            strip_html_tags(&raw)
        }
        DocumentFormat::Markdown | DocumentFormat::Txt => {
            tokio::fs::read_to_string(file_path)
                .await
                .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
        }
    };

    Ok(clean_text(&content))
}

// ============ PDF ============

/// 尝试通过外部 pdftotext（poppler-utils）提取文本
async fn try_pdftotext(file_path: &str) -> Result<String, ()> {
    let mut cmd = tokio::process::Command::new("pdftotext");
    cmd.args(["-layout", file_path, "-"]);
    hide_console_window(&mut cmd);
    match cmd.output().await {
        Ok(result) if result.status.success() => {
            let text = String::from_utf8_lossy(&result.stdout).to_string();
            if !text.trim().is_empty() {
                Ok(text)
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

/// 解析 PDF 文件
/// 优先用外部 pdftotext（精度最高）；不可用时回退到 pdf-extract（纯 Rust）
async fn parse_pdf(file_path: &str) -> Result<String, KnowledgeBaseError> {
    if let Ok(text) = try_pdftotext(file_path).await {
        return Ok(text);
    }
    let path_owned = file_path.to_string();
    tokio::task::spawn_blocking(move || {
        pdf_extract::extract_text(&path_owned)
            .map_err(|e| KnowledgeBaseError::DocumentParseError(format!("PDF 解析失败: {e}")))
    })
    .await
    .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
}

// ============ Word / DOCX ============

/// 解析 Word 文档（.docx）
async fn parse_word(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| KnowledgeBaseError::DocumentParseError(format!("读取 DOCX 失败: {}", e)))?;

    use std::io::Read;
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| {
        KnowledgeBaseError::DocumentParseError("无法解析 DOCX 文件（格式损坏或不是有效 ZIP）".into())
    })?;

    let mut xml_content = String::new();
    if let Ok(mut file) = archive.by_name("word/document.xml") {
        file.read_to_string(&mut xml_content)
            .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;
    }

    Ok(extract_text_from_docx_xml(&xml_content))
}

/// 从 DOCX XML 中提取纯文本，保留段落换行及表格结构（单元格用 Tab 分隔，行用换行）。
///
/// 用哨兵字符串标记结构边界，避免直接替换 `</w:p>` 为 `\n` 时位置错乱的问题。
/// 判断逻辑：行结束（TR）优先于单元格结束（TC）优先于段落结束（PP）。
fn extract_text_from_docx_xml(xml: &str) -> String {
    const CELL_END: &str = "\x02TC\x02";
    const ROW_END: &str  = "\x02TR\x02";
    const PARA_END: &str = "\x02PP\x02";

    let xml = xml
        .replace("<w:br/>", "\n")
        .replace("<w:br />", "\n")
        .replace("</w:tc>", CELL_END)
        .replace("</w:tr>", ROW_END)
        .replace("</w:p>", PARA_END);

    let mut result = String::new();
    for chunk in xml.split("<w:t") {
        if let Some(end) = chunk.find("</w:t>") {
            if let Some(start) = chunk.find('>') {
                result.push_str(&chunk[start + 1..end]);
            }
            let after = &chunk[end..];
            // Row end takes priority; within a row, cell-end → tab; else paragraph → newline.
            if after.contains(ROW_END) {
                result.push('\n');
            } else if after.contains(CELL_END) {
                result.push('\t');
            } else if after.contains(PARA_END) {
                result.push('\n');
            }
        }
    }
    result
}

// ============ PowerPoint / PPTX ============

/// 解析 PowerPoint 文档（.pptx）
async fn parse_pptx(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;

    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let cursor = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|_| {
            KnowledgeBaseError::DocumentParseError("无法解析 PPTX 文件".into())
        })?;

        // 收集幻灯片文件名并按页码排序
        let mut slide_names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
            .collect();
        slide_names.sort();

        let mut result = String::new();
        for (idx, name) in slide_names.iter().enumerate() {
            let mut xml = String::new();
            archive
                .by_name(name)
                .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
                .read_to_string(&mut xml)
                .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;
            result.push_str(&format!("--- 第 {} 页 ---\n", idx + 1));
            result.push_str(&extract_text_from_pptx_xml(&xml));
            result.push('\n');
        }
        Ok(result)
    })
    .await
    .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
}

/// 从 PPTX 幻灯片 XML（DrawingML）中提取纯文本（保留段落换行）
fn extract_text_from_pptx_xml(xml: &str) -> String {
    const PARA_END: &str = "\x02PARA\x02";
    let xml = xml.replace("</a:p>", PARA_END);
    let mut result = String::new();
    for chunk in xml.split("<a:t") {
        if let Some(end) = chunk.find("</a:t>") {
            if let Some(start) = chunk.find('>') {
                result.push_str(&chunk[start + 1..end]);
            }
            if chunk[end..].contains(PARA_END) {
                result.push('\n');
            }
        }
    }
    result
}

// ============ Excel / XLSX / XLS / CSV ============

/// 解析 Excel 文件（.xlsx、.xls 用 calamine；.csv 直接读文本）
async fn parse_excel(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if ext.eq_ignore_ascii_case("csv") {
        return tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()));
    }

    let path_owned = file_path.to_string();
    tokio::task::spawn_blocking(move || {
        use calamine::{open_workbook_auto, Reader};
        let mut workbook = open_workbook_auto(&path_owned).map_err(|e| {
            KnowledgeBaseError::DocumentParseError(format!("Excel 解析失败: {e}"))
        })?;
        let sheet_names = workbook.sheet_names().to_vec();
        let mut result = String::new();
        for sheet_name in sheet_names {
            result.push_str(&format!("=== {} ===\n", sheet_name));
            if let Ok(range) = workbook.worksheet_range(&sheet_name) {
                for row in range.rows() {
                    let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
                    result.push_str(&cells.join("\t"));
                    result.push('\n');
                }
            }
            result.push('\n');
        }
        Ok(result)
    })
    .await
    .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
}

// ============ 通用工具 ============

/// Strip HTML tags and decode common entities, preserving block-element whitespace.
///
/// Handles script/style content removal, block-element newlines, and table cell tabs.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut skip_content = false; // inside <script> or <style>

    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag = tag_buf.trim().to_lowercase();
                let tag_name = tag.split_whitespace().next().unwrap_or("");
                let is_closing = tag_name.starts_with('/');
                let base = tag_name.trim_start_matches('/');

                if is_closing && (base == "script" || base == "style") {
                    skip_content = false;
                } else if !is_closing && (base == "script" || base == "style") {
                    skip_content = true;
                } else if !skip_content {
                    match base {
                        "br" => result.push('\n'),
                        "td" | "th" => result.push('\t'),
                        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        | "li" | "tr" | "blockquote" | "pre" | "article"
                        | "section" | "header" | "footer" | "nav" | "main" => {
                            result.push('\n');
                        }
                        _ => {}
                    }
                }
                tag_buf.clear();
            }
            c if in_tag => tag_buf.push(c),
            c if !skip_content => result.push(c),
            _ => {}
        }
    }

    // Decode common HTML entities (single pass, ordered to avoid double-decode)
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
}

/// Clean and normalize text
fn clean_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Calculate file hash
pub async fn calculate_file_hash(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;

    let hash = Sha256::digest(&bytes);
    Ok(format!("{:x}", hash))
}

/// 分隔符按"粗粒度 → 细粒度"优先级排列。
///
/// Markdown 标题行（`\n# ` 等）排在最前，使同一标题下的内容优先聚在同一个块里。
/// 标题分隔符保留在左侧块末尾（含 `\n# ` 字符），对语义影响极小。
/// 之后依次是段落、句子、逗号、空格，最后由 `hard_split_by_chars` 兜底。
const SPLIT_SEPARATORS: &[&str] = &[
    "\n# ",
    "\n## ",
    "\n### ",
    "\n#### ",
    "\n##### ",
    "\n\n",
    "\n",
    "。", "！", "？", "；",
    ". ", "! ", "? ", "; ",
    "，", ", ",
    " ",
];

/// 按字符数（而非字节数）统计长度。UI 上"分块大小"标注的单位是字符数
/// （见 KnowledgeBaseView.vue「分块大小（字符数）」），中文字符在 UTF-8
/// 下占 3 字节，如果直接用 `str::len()`（字节数）跟 chunk_size 比较，
/// 中文文本实际分出来的块会只有用户设置值的 1/3 左右。
fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// 返回 `s` 末尾 `n` 个字符对应的切片（按字符数而非字节数截取）
fn tail_chars(s: &str, n: usize) -> &str {
    let total = char_count(s);
    if n == 0 || total == 0 {
        return "";
    }
    if n >= total {
        return s;
    }
    let skip = total - n;
    let byte_idx = s.char_indices().nth(skip).map(|(b, _)| b).unwrap_or(s.len());
    &s[byte_idx..]
}

/// 在 `text` 中按 `sep` 切分，并把分隔符保留在前一段末尾
fn split_keep_separator<'a>(text: &'a str, sep: &str) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut last = 0usize;
    while let Some(pos) = text[last..].find(sep) {
        let end = last + pos + sep.len();
        result.push(&text[last..end]);
        last = end;
    }
    if last < text.len() {
        result.push(&text[last..]);
    }
    result
}

/// 递归分割：依次尝试用从粗到细的分隔符切分文本。
fn recursive_split(text: &str, chunk_size: usize, sep_index: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if char_count(text) <= chunk_size {
        return vec![text.to_string()];
    }
    if sep_index >= SPLIT_SEPARATORS.len() {
        return hard_split_by_chars(text, chunk_size);
    }

    let sep = SPLIT_SEPARATORS[sep_index];
    if !text.contains(sep) {
        return recursive_split(text, chunk_size, sep_index + 1);
    }

    let parts = split_keep_separator(text, sep);
    let mut result = Vec::new();
    let mut current = String::new();

    for part in parts {
        if char_count(part) > chunk_size {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
            result.extend(recursive_split(part, chunk_size, sep_index + 1));
            continue;
        }
        if char_count(&current) + char_count(part) > chunk_size && !current.is_empty() {
            result.push(std::mem::take(&mut current));
        }
        current.push_str(part);
    }
    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// 硬字符切分（最后一道兜底）
fn hard_split_by_chars(text: &str, chunk_size: usize) -> Vec<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        let start_byte = chars[start].0;
        let end_byte = chars.get(end).map(|&(b, _)| b).unwrap_or(text.len());
        result.push(text[start_byte..end_byte].to_string());
        start = end;
    }

    result
}

/// 在切好的块之间补上重叠
fn apply_overlap(chunks: Vec<String>, chunk_overlap: usize) -> Vec<String> {
    if chunk_overlap == 0 || chunks.len() <= 1 {
        return chunks;
    }

    let mut result = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        if i == 0 {
            result.push(chunk.clone());
        } else {
            let tail = tail_chars(&chunks[i - 1], chunk_overlap);
            result.push(format!("{}{}", tail, chunk));
        }
    }
    result
}

/// 文本分块
pub fn split_text(text: &str, chunk_size: usize, chunk_overlap: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let chunk_size = chunk_size.max(1);

    let chunks = recursive_split(trimmed, chunk_size, 0);
    let result = apply_overlap(chunks, chunk_overlap);

    log::debug!(
        "split_text: {} 字符 -> {} 块 (chunk_size={}, chunk_overlap={})",
        char_count(trimmed),
        result.len(),
        chunk_size,
        chunk_overlap
    );

    result
}

/// Estimate token count (rough approximation)
pub fn estimate_tokens(text: &str) -> i32 {
    let char_count = text.chars().count();
    (char_count / 3) as i32
}
