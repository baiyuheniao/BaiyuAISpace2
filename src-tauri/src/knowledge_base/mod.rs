// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 知识库模块
 * 
 * 模块说明:
 * - commands: 知识库相关 Tauri 命令
 * - db: 向量数据库操作
 * - document: 文档处理
 * - embedding: 文本嵌入
 * - retrieval: 相似度检索
 * - types: 类型定义
 */

pub mod commands;
pub mod db;
pub mod document;
pub mod embedding;
pub mod retrieval;
pub mod types;
