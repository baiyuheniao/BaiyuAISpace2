<!-- This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

# 文档索引

## 当前文档（反映现状，持续维护）

| 文档 | 说明 |
|---|---|
| [MANUAL_TEST_CHECKLIST.md](./MANUAL_TEST_CHECKLIST.md) | 全场景人工测试清单，可逐项打勾执行 |
| [SOFTWARE_TESTING_PLAN.md](./SOFTWARE_TESTING_PLAN.md) | 测试策略、环境准备与流程规范 |
| [api-manuals/](./api-manuals/) | 15 个 LLM 提供商的官方 API 技术手册 |
| [AgentTeamMode/](./AgentTeamMode/) | Agent Team（多 Agent 协作）设计文档（md/docx/pdf） |

## 历史归档（仅供追溯，不代表当前实现）

`archive/` 目录下的文档是特定时间点的开发记录或早期规划方案，此后代码已演进，内容可能与当前实现不符：

| 文档 | 记录时间 | 说明 |
|---|---|---|
| [archive/development-log.md](./archive/development-log.md) | 2026-02-10 ~ 2026-02-15 | MVP 阶段开发日志，此后未再更新 |
| [archive/pr-review-report.md](./archive/pr-review-report.md) | 2026-05-23 | 某次 PR 批量审核的快照报告，涉及 PR 均已处理完毕 |
| [archive/mcp-integration-guide.md](./archive/mcp-integration-guide.md) | 早期 | MCP 集成规划文档，TODO 项（SQLite 持久化、Function Calling）均已实现 |
| [archive/mcp-llm-integration.md](./archive/mcp-llm-integration.md) | 早期 | 早期基于文本提示词+正则解析的工具调用方案，已被原生 Function Calling 取代 |

需要了解当前功能全貌，请看[根目录 README](../README.md)。
