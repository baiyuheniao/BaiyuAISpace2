// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use jieba_rs::Jieba;
use once_cell::sync::Lazy;
use std::collections::HashSet;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

// 这些词描述的是提问动作或 RAG 容器本身，不是用户真正想查的知识。
// 去掉它们能避免“根据知识库回答一下”之类套话淹没实际关键词。
const QUERY_STOP_WORDS: &[&str] = &[
    "一下",
    "一个",
    "这个",
    "那个",
    "什么",
    "为什么",
    "怎么",
    "如何",
    "是否",
    "请问",
    "请",
    "回答",
    "告诉",
    "根据",
    "基于",
    "只按",
    "关于",
    "有关",
    "问题",
    "内容",
    "文档",
    "知识库",
    "教材",
    "里面",
    "中的",
    "中",
    "是",
    "的",
    "了",
    "吗",
    "呢",
    "和",
    "与",
    "或",
];

fn normalize_token(token: &str) -> Option<String> {
    let normalized = token.trim().to_lowercase();
    if normalized.is_empty() || !normalized.chars().any(char::is_alphanumeric) {
        return None;
    }
    Some(normalized)
}

fn is_significant_query_token(token: &str) -> bool {
    if QUERY_STOP_WORDS.contains(&token) {
        return false;
    }

    // 英文缩写、数字、题号即使只有 1 个字符也可能是关键实体；中文单字通常
    // 噪声较大，至少保留两个字符。
    token.chars().any(|c| c.is_ascii_alphanumeric()) || token.chars().count() >= 2
}

/// 将原文转换为适合 SQLite FTS5 unicode61 tokenizer 的预分词文本。
/// 原始 chunk 内容仍保存在 chunks 表中，这里只服务于轻量关键词索引。
pub fn tokenize_for_fts(text: &str) -> String {
    JIEBA
        .cut_for_search(text, true)
        .into_iter()
        .filter_map(normalize_token)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 从自然语言问题中提取可用于 FTS/LIKE 的关键词。
/// 搜索模式会同时给出较长词和适合检索的短词，随后去重并限制数量。
pub fn extract_query_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = Vec::new();

    for raw in JIEBA.cut_for_search(query, true) {
        let Some(token) = normalize_token(raw) else {
            continue;
        };
        if !is_significant_query_token(&token) || !seen.insert(token.clone()) {
            continue;
        }
        terms.push(token);
        if terms.len() >= 24 {
            break;
        }
    }

    terms
}

/// FTS5 查询使用 OR 组合关键词。相关度由候选内容的关键词覆盖率和 bm25
/// 共同排序，不再要求用户整句原样出现在文档中。
pub fn build_fts_query(query: &str) -> Option<(String, Vec<String>)> {
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return None;
    }

    let expression = terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");

    Some((expression, terms))
}

pub fn keyword_coverage(content: &str, terms: &[String]) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }

    let normalized_content = content.to_lowercase();
    let matched = terms
        .iter()
        .filter(|term| normalized_content.contains(term.as_str()))
        .count();
    matched as f32 / terms.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_natural_question_extracts_real_keywords() {
        let terms = extract_query_terms(
            "根据教材，第16题中第二个 better 是什么词性、意思是什么？只按教材答案回答。",
        );

        assert!(terms.iter().any(|term| term == "better"));
        assert!(terms.iter().any(|term| term == "词性"));
        assert!(terms.iter().any(|term| term == "意思"));
        assert!(!terms.iter().any(|term| term == "根据"));
        assert!(!terms.iter().any(|term| term == "教材"));
    }

    #[test]
    fn fts_query_uses_or_instead_of_exact_sentence() {
        let (query, terms) = build_fts_query("第二个 better 的词性和意思").unwrap();

        assert!(query.contains(" OR "));
        assert!(query.contains("\"better\""));
        assert!(terms.len() >= 3);
    }

    #[test]
    fn coverage_rewards_the_chunk_containing_the_answer() {
        let terms = extract_query_terms("第二个 better 的词性和意思");
        let relevant = keyword_coverage("⑤ better（词性：动词；词义：改善，使更好）", &terms);
        let irrelevant = keyword_coverage("spaceship 是由 space 和 ship 构成的合成词", &terms);

        assert!(relevant > irrelevant);
        assert!(relevant > 0.0);
    }
}
