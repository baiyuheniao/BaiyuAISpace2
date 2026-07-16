// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * 命令模块
 * 
 * 模块说明:
 * - llm: LLM 聊天相关命令 (流式消息、对话管理)
 * - mcp: MCP 服务器相关命令 (工具调用、服务器管理)
 * - constants: 超时和延迟常量
 * - local_model: 本地模型管理命令 (Ollama 集成)
 * - lmstudio: 本地模型管理命令 (LM Studio 集成)
 * - skills: Skill (技能) 管理命令
 */

pub mod app_update;
pub mod constants;
pub mod docker;
pub mod llm;
pub mod lmstudio;
pub mod local_model;
pub mod mcp;
pub mod skills;