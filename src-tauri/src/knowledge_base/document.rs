// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::types::*;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Supported document formats
#[derive(Debug, Clone, Copy)]
pub enum DocumentFormat {
    Pdf,
    Word,  // docx
    Excel, // xlsx
    Markdown,
    Html,
    Txt,
}

impl DocumentFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "pdf" => Some(DocumentFormat::Pdf),
            "docx" | "doc" => Some(DocumentFormat::Word),
            "xlsx" | "xls" | "csv" => Some(DocumentFormat::Excel),
            "md" | "markdown" => Some(DocumentFormat::Markdown),
            "html" | "htm" => Some(DocumentFormat::Html),
            "txt" | "text" | "rs" | "js" | "ts" | "py" | "java" | "c" | "cpp" | "h" | "go" | "rs" => Some(DocumentFormat::Txt),
            _ => None,
        }
    }

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

/// Parse document content to text
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
    let output = tokio::process::Command::new("pdftotext")
        .args(["-layout", file_path, "-"])
        .output()
        .await;
    
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
    
    // Very basic extraction - this is not production ready
    // Real implementation should use a PDF library
    for line in content.lines() {
        // Look for BT...ET blocks (PDF text objects)
        if line.contains("(") && line.contains(")") {
            // Extract text between ( and )
            let mut in_paren = false;
            let mut text = String::new();
            for ch in line.chars() {
                match ch {
                    '(' => in_paren = true,
                    ')' => in_paren = false,
                    _ if in_paren => text.push(ch),
                    _ => {}
                }
            }
            if !text.is_empty() {
                result.push_str(&text);
                result.push(' ');
            }
        }
    }
    
    if result.is_empty() {
        return Err(KnowledgeBaseError::DocumentParseError(
            "PDF parsing not available. Please install pdftotext or use text files.".to_string()
        ));
    }
    
    Ok(result)
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
    let mut in_tag = false;
    let mut in_text = false;
    
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

/// Split text into chunks
pub fn split_text(text: &str, chunk_size: usize, chunk_overlap: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    
    // Try to split at paragraph boundaries first
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut current_chunk = String::new();
    
    for para in paragraphs {
        if current_chunk.len() + para.len() > chunk_size && !current_chunk.is_empty() {
            chunks.push(current_chunk.clone());
            // Keep overlap
            let overlap_start = if current_chunk.len() > chunk_overlap {
                current_chunk.len() - chunk_overlap
            } else {
                0
            };
            current_chunk = current_chunk[overlap_start..].to_string();
        }
        
        if !current_chunk.is_empty() {
            current_chunk.push('\n');
            current_chunk.push('\n');
        }
        current_chunk.push_str(para);
    }
    
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }
    
    // If any chunk is still too large, split by sentences
    let mut final_chunks = Vec::new();
    for chunk in chunks {
        if chunk.len() > chunk_size * 2 {
            // Split by sentences
            let sentences: Vec<&str> = chunk.split('.').collect();
            let mut sentence_chunk = String::new();
            
            for sentence in sentences {
                if sentence_chunk.len() + sentence.len() > chunk_size && !sentence_chunk.is_empty() {
                    final_chunks.push(sentence_chunk.clone());
                    sentence_chunk.clear();
                }
                sentence_chunk.push_str(sentence);
                sentence_chunk.push('.');
            }
            
            if !sentence_chunk.is_empty() {
                final_chunks.push(sentence_chunk);
            }
        } else {
            final_chunks.push(chunk);
        }
    }
    
    // Final fallback: hard split if still too large
    let mut result = Vec::new();
    for chunk in final_chunks {
        if chunk.len() > chunk_size * 2 {
            for i in (0..chunk.len()).step_by(chunk_size - chunk_overlap) {
                let end = (i + chunk_size).min(chunk.len());
                result.push(chunk[i..end].to_string());
                if end == chunk.len() {
                    break;
                }
            }
        } else {
            result.push(chunk);
        }
    }
    
    result
}

/// Estimate token count (rough approximation)
pub fn estimate_tokens(text: &str) -> i32 {
    // Rough estimate: 1 token ≈ 4 characters for English, 2-3 for Chinese
    let char_count = text.chars().count();
    (char_count / 3) as i32
}
