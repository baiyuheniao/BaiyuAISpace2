// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * 命令模块
 * 
 * 模块说明:
 * - auth: 认证相关命令 (获取百度 access token 等)
 * - llm: LLM 聊天相关命令 (流式消息、对话管理)
 * - mcp: MCP 服务器相关命令 (工具调用、服务器管理)
 */

pub mod auth;
pub mod llm;
pub mod mcp;
