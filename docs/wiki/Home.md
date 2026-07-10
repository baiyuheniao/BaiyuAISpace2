# BaiyuAISpace Wiki

<p align="center"><strong>桌面 AI 工作台：LLM 客户端 + Agent 编排 + 本地模型托管</strong></p>

**BaiyuAISpace** 是一个专为开发者和 AI 爱好者打造的桌面 AI 工作台，基于 **Tauri 2 + Vue 3 + Rust** 构建。既是接入 15+ 家云端服务商与 Ollama / LM Studio 本地模型的 LLM 客户端，也内置多 Agent 协作、MCP 工具、RAG 知识库与定时任务等 Agent 编排能力——自带 API Key、用多少付多少，自由度与成本都由你自己掌控。

> 当前版本：**v0.2.0-beta.3**（Beta 公测阶段）。核心功能已完整可用并经过全场景测试，但仍可能存在未知问题，欢迎[提交 Issue](https://github.com/baiyuheniao/BaiyuAISpace2/issues/new/choose) 反馈。

## 核心设计理念

- 🪶 **原生轻快**：Tauri 2 原生窗口，安装包比 Electron 方案小约 80%，秒级启动，低配置设备也能流畅运行——轻在壳，功能不缩水
- 🖥️ **真跨平台**：一套代码同时支持 Windows、macOS、Linux（当前以 Windows 为主力平台）
- 🔒 **本地优先**：API 密钥加密存入系统密钥链，会话与知识库全存本地 SQLite；搭配 Ollama / LM Studio 可完全离线使用
- 🤖 **Agent 原生**：内置多 Agent 协作工作组、MCP 工具调用、RAG 知识库、Skill 技能、定时任务

## 应用一览

应用侧边栏共九个模块，本 Wiki 按模块组织功能文档：

| 模块 | 功能 | 文档 |
|---|---|---|
| **Chat 对话** | 与云端 / 本地大模型对话，多模态、思考模式、文档注入 | [[对话与模型接入]] |
| **Skill 技能** | 自定义可复用的提示词技能，绑定工具与知识库 | [[Skill技能]] |
| **RAG 知识库** | 导入文档构建向量知识库，对话中检索引用 | [[知识库RAG]] |
| **MCP 模型工具** | 接入 MCP 工具服务器，让模型调用外部工具 | [[MCP工具]] |
| **Local 本地部署** | 管理 Ollama / LM Studio / Docker 本地模型 | [[本地部署]] |
| **Agents 协作团队** | 多 Agent 工作组：消息路由、圆桌会议、生命周期管理 | [[Agent Team协作团队]] |
| **Cron 定时任务** | 单次 / 间隔 / 每天 / 每周调度，可唤醒 Agent | [[定时任务]] |
| **History 历史记录** | 浏览与恢复历史会话 | [[对话与模型接入]] |
| **Settings 设置** | LLM / Embedding / Reranker API 配置、日志导出 | [[快速上手]] |

## 快速导航

- 🆕 新用户从这里开始：[[安装指南]] → [[快速上手]]
- 🔌 想知道支持哪些模型：[[支持的LLM提供商]]
- ❓ 遇到问题先查：[[常见问题FAQ]]
- 🛠️ 想参与开发或从源码构建：[[开发者指南]]
- 📜 能不能商用：[[许可证说明]]

## 反馈渠道

- **Bug 报告 / 功能建议**：使用 [Issue 模板](https://github.com/baiyuheniao/BaiyuAISpace2/issues/new/choose)提交（Bug 请务必附上日志，位置见 [[常见问题FAQ]]）
- **使用咨询与开放讨论**：[GitHub Discussions](https://github.com/baiyuheniao/BaiyuAISpace2/discussions)
- **安全漏洞**：请按 [SECURITY.md](https://github.com/baiyuheniao/BaiyuAISpace2/blob/main/SECURITY.md) 私下报告，不要公开发 Issue
