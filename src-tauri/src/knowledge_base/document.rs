// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 文档处理模块
 * 
 * 功能说明:
 * - 支持多种文档格式解析 (PDF, DOCX, XLSX, MD, HTML, TXT)
 * - 文件哈希计算
 * - 文本分块
 * - Token 数量估算
 */

use super::types::*;
use crate::commands::local_model::hide_console_window;
use sha2::{Digest, Sha256};
use std::path::Path;

/// 支持的文档格式枚举
#[derive(Debug, Clone, Copy)]
pub enum DocumentFormat {
    /// PDF 文档
    Pdf,
    /// Word 文档 (docx)
    Word,
    /// Excel 文档 (xlsx)
    Excel,
    /// Markdown 文档
    Markdown,
    /// HTML 文档
    Html,
    /// 纯文本
    Txt,
}

#[allow(dead_code)]
impl DocumentFormat {
    /// 根据文件扩展名获取文档格式
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "pdf" => Some(DocumentFormat::Pdf),
            "docx" | "doc" => Some(DocumentFormat::Word),
            "xlsx" | "xls" | "csv" => Some(DocumentFormat::Excel),
            "md" | "markdown" => Some(DocumentFormat::Markdown),
            "html" | "htm" => Some(DocumentFormat::Html),
            "txt" | "text" | "rs" | "js" | "ts" | "py" | "java" | "c" | "cpp" | "h" | "go" => Some(DocumentFormat::Txt),
            _ => None,
        }
    }

    /// 获取格式对应的文件扩展名
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentFormat::Pdf => "pdf",
            DocumentFormat::Word => "docx",
            DocumentFormat::Excel => "xlsx",
            DocumentFormat::Markdown => "md",
            DocumentFormat::Html => "html",
            DocumentFormat::Txt => "txt",
        }
    }
}

/// 解析文档内容为纯文本
/// 
/// # 参数
/// - file_path: 文件路径
/// 
/// # 返回
/// 提取的文本内容
pub async fn parse_document(file_path: &str) -> Result<String, KnowledgeBaseError> {
    let path = Path::new(file_path);
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    let format = DocumentFormat::from_extension(ext)
        .ok_or_else(|| KnowledgeBaseError::DocumentParseError(format!("Unsupported format: {}", ext)))?;
    
    let content = match format {
        DocumentFormat::Pdf => parse_pdf(file_path).await?,
        DocumentFormat::Word => parse_word(file_path).await?,
        DocumentFormat::Excel => parse_excel(file_path).await?,
        DocumentFormat::Markdown | DocumentFormat::Html | DocumentFormat::Txt => {
            tokio::fs::read_to_string(file_path)
                .await
                .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?
        }
    };
    
    // Clean up content
    let cleaned = clean_text(&content);
    
    Ok(cleaned)
}

/// Parse PDF file
async fn parse_pdf(file_path: &str) -> Result<String, KnowledgeBaseError> {
    // For now, use basic text extraction
    // In production, use pdf-extract or pdfium
    // Since pdf-extract has dependency issues, we'll use a placeholder
    
    // Try to use external pdftotext if available
    let mut cmd = tokio::process::Command::new("pdftotext");
    cmd.args(["-layout", file_path, "-"]);
    hide_console_window(&mut cmd);
    let output = cmd.output().await;
    
    match output {
        Ok(result) if result.status.success() => {
            Ok(String::from_utf8_lossy(&result.stdout).to_string())
        }
        _ => {
            // Fallback: try to read as binary and extract strings (very basic)
            let bytes = tokio::fs::read(file_path)
                .await
                .map_err(|e| KnowledgeBaseError::DocumentParseError(format!("Failed to read PDF: {}", e)))?;
            
            // Basic PDF text extraction - look for text between parentheses
            let text = extract_text_from_pdf_bytes(&bytes)?;
            Ok(text)
        }
    }
}

/// Basic PDF text extraction (fallback)
fn extract_text_from_pdf_bytes(bytes: &[u8]) -> Result<String, KnowledgeBaseError> {
    let content = String::from_utf8_lossy(bytes);
    let mut result = String::new();
    
    // Improved PDF text extraction strategy:
    // 1. Look for BT...ET blocks (PDF text objects) with Tf (font) and Tj/TJ (text) operators
    let mut in_text_block = false;
    let mut current_text = String::new();
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        // Track text block boundaries
        if trimmed == "BT" {
            in_text_block = true;
            current_text.clear();
            continue;
        }
        if trimmed == "ET" {
            in_text_block = false;
            if !current_text.is_empty() {
                result.push_str(&current_text);
                result.push(' ');
            }
            current_text.clear();
            continue;
        }
        
        if in_text_block {
            // Extract text from Tj operator: (text) Tj
            if let Some(pos) = trimmed.find(")Tj") {
                let before = &trimmed[..pos];
                if let Some(start) = before.rfind('(') {
                    let text = &before[start + 1..];
                    // Decode PDF string escapes
                    let decoded = decode_pdf_string(text);
                    current_text.push_str(&decoded);
                }
            }
            
            // Extract text from TJ array operator: [(text1) (text2)] TJ
            if trimmed.contains("TJ") {
                for part in trimmed.split(')') {
                    let part = part.trim();
                    if let Some(start) = part.rfind('(') {
                        let text = &part[start + 1..];
                        let decoded = decode_pdf_string(text);
                        current_text.push_str(&decoded);
                    }
                }
            }
        }
        
        // Also try the simple parenthesis extraction as a secondary method
        if !in_text_block && line.contains("(") && line.contains(")") {
            let mut in_paren = false;
            let mut text = String::new();
            for ch in line.chars() {
                match ch {
                    '(' => in_paren = true,
                    ')' => {
                        in_paren = false;
                        if !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) {
                            result.push_str(&text);
                            result.push(' ');
                        }
                        text.clear();
                    }
                    _ if in_paren => text.push(ch),
                    _ => {}
                }
            }
        }
    }
    
    // Clean up the result
    let cleaned: String = result
        .chars()
        .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace() || *c == '\n')
        .collect();
    
    if cleaned.trim().is_empty() {
        return Err(KnowledgeBaseError::DocumentParseError(
            "PDF parsing not available. Please install pdftotext (poppler-utils) or use text files.".to_string()
        ));
    }
    
    Ok(cleaned)
}

/// Decode PDF string escape sequences
fn decode_pdf_string(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                Some('n') => { result.push('\n'); chars.next(); }
                Some('r') => { result.push('\r'); chars.next(); }
                Some('t') => { result.push('\t'); chars.next(); }
                Some('b') => { result.push('\u{08}'); chars.next(); }
                Some('f') => { result.push('\u{0c}'); chars.next(); }
                Some('(') => { result.push('('); chars.next(); }
                Some(')') => { result.push(')'); chars.next(); }
                Some('\\') => { result.push('\\'); chars.next(); }
                Some('0'..='7') => {
                    // Octal escape
                    let mut octal = String::new();
                    for _ in 0..3 {
                        if let Some(&digit @ '0'..='7') = chars.peek() {
                            octal.push(digit);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                        result.push(byte as char);
                    }
                }
                _ => {} // Unknown escape, skip backslash
            }
        } else {
            result.push(ch);
        }
    }
    
    result
}

/// Parse Word document
async fn parse_word(file_path: &str) -> Result<String, KnowledgeBaseError> {
    // For now, read as zip and extract document.xml text
    // Real implementation should use docx crate
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| KnowledgeBaseError::DocumentParseError(format!("Failed to read DOCX: {}", e)))?;
    
    // Try to unzip and read word/document.xml
    use std::io::Read;
    
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(_) => return Err(KnowledgeBaseError::DocumentParseError(
            "Failed to parse DOCX. Install docx support or use text files.".to_string()
        )),
    };
    
    let mut xml_content = String::new();
    if let Ok(mut file) = archive.by_name("word/document.xml") {
        file.read_to_string(&mut xml_content)
            .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;
    }
    
    // Extract text from XML
    let text = extract_text_from_docx_xml(&xml_content);
    Ok(text)
}

/// Extract text from DOCX XML
fn extract_text_from_docx_xml(xml: &str) -> String {
    let mut result = String::new();
    
    // Simple XML text extraction
    for chunk in xml.split("<w:t") {
        if let Some(end) = chunk.find("</w:t>") {
            if let Some(start) = chunk.find('>') {
                let text = &chunk[start+1..end];
                if !text.is_empty() {
                    result.push_str(text);
                }
            }
        }
    }
    
    // Also handle <w:tab/> and <w:br/>
    result = result.replace("<w:tab/>", "\t");
    result = result.replace("<w:br/>", "\n");
    
    result
}

/// Parse Excel file
async fn parse_excel(file_path: &str) -> Result<String, KnowledgeBaseError> {
    // For now, use csv-like parsing for CSV files
    // XLSX would need calamine or similar
    let path = Path::new(file_path);
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    
    if ext.eq_ignore_ascii_case("csv") {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| KnowledgeBaseError::DocumentParseError(e.to_string()))?;
        Ok(content)
    } else {
        // For xlsx, we'd need a proper library
        Err(KnowledgeBaseError::DocumentParseError(
            "Excel (.xlsx) parsing requires additional dependencies. Use CSV format.".to_string()
        ))
    }
}

/// Clean and normalize text
fn clean_text(text: &str) -> String {
    text
        // Normalize whitespace
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        // Remove excessive blank lines
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

/// 分隔符按"粗粒度 → 细粒度"优先级排列；中英文句末标点都包含在内，
/// 修复了原先只按英文句号 `.` 切句子、导致中文文本完全切不到句子级别、
/// 只能直接硬切的问题。
const SPLIT_SEPARATORS: &[&str] = &[
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
/// （例如 "你好。世界。" 切成 ["你好。", "世界。"]，不丢标点）
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

/// 递归分割：依次尝试用从粗到细的分隔符切分文本。任何切出来的片段如果仍
/// 超过 `chunk_size`（字符数），就用下一级更细的分隔符继续递归切分它；
/// 分隔符全部用完后退化为硬字符切分。
///
/// 相比之前"只有 > chunk_size*2 时才按英文句号切"的实现，这里没有任意的
/// 倍数阈值——任何超过 chunk_size 的片段都会被继续细分，块大小更可控；
/// 分隔符同时覆盖中英文标点，中文文本也能在句子边界切分而不是直接硬切。
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
            // 这一段本身就超长，先把已攒的内容收尾，再用更细的分隔符递归切它
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

/// 硬字符切分（最后一道兜底）：当文本里连一个可用分隔符都没有时
/// （例如一长串没有空格的字符或代码），按字符数（不是字节数）切分，
/// 保证永远不会切断一个 UTF-8 字符。
fn hard_split_by_chars(text: &str, chunk_size: usize) -> Vec<String> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut start = 0usize; // 字符下标（不是字节下标）

    while start < chars.len() {
        let end = (start + chunk_size).min(chars.len());
        let start_byte = chars[start].0;
        let end_byte = chars.get(end).map(|&(b, _)| b).unwrap_or(text.len());
        result.push(text[start_byte..end_byte].to_string());
        start = end;
    }

    result
}

/// 在切好的块之间补上重叠：每一块（除第一块）前面接上前一块末尾
/// `chunk_overlap` 个字符，用于在块边界保留上下文连续性。
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

/// 文本分块：按"粗到细"的分隔符递归切分（段落 → 换行 → 中英文句末标点 →
/// 逗号 → 空格 → 硬字符切分），再在块之间补上 `chunk_overlap` 个字符的重叠。
///
/// `chunk_size`/`chunk_overlap` 的单位是字符数，与设置界面"分块大小
/// （字符数）"保持一致——这里全程用 `chars().count()` 而不是 `str::len()`
/// 比较长度，避免中文等多字节字符被按字节数误判成更小的块。
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
    // Rough estimate: 1 token ≈ 4 characters for English, 2-3 for Chinese
    let char_count = text.chars().count();
    (char_count / 3) as i32
}
