# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概况

BaiyuAISpace2 是一个 Tauri 2 桌面应用（Windows 为主力平台）：轻量级 LLM 客户端 + Agent 开发环境。
Rust 后端（`src-tauri/`）+ Vue 3 / TypeScript 前端（`src/`），SQLite 本地存储，UI 与提交信息均为中文。

## 常用命令

```bash
pnpm install                       # 安装前端依赖
pnpm build                         # vue-tsc --noEmit + vite build（前端类型检查兼构建门禁）
pnpm tauri dev                     # 开发模式（真窗口 + vite HMR，首次 ~25s）
pnpm tauri build                   # 生产构建。修完代码后必须跑它验证编译（用户硬性要求）
cd src-tauri && cargo build        # 仅后端 debug 编译（比 tauri build 快，日常验证用）
pnpm lint                          # eslint --fix（.vue/.ts/.tsx）
pnpm format                        # prettier
```

- **构建顺序**：`cargo build` 编译期读取 `dist/`（tauri.conf.json 的 `frontendDist`），
  从未跑过 `pnpm build` 时先跑它，否则 cargo 直接 panic。
- **没有常规测试套件**。唯一的自动化测试是 `src-tauri/src/workspace_smoke_test.rs`
  （Agent Team 冒烟测试）。正确性门禁就是 `pnpm build` + `cargo build`。
- 以 Agent 身份启动/操作/截图应用：用 `run-baiyuaispace2` skill（CDP driver），
  不要用 `pnpm tauri dev`。注意 WebView2 profile 锁：同一时刻只能跑一个实例。

## 架构

数据流单向：**Vue 视图 → Pinia store → `invoke` Tauri command → Rust 模块 → SQLite/外部 API**。
前后端各按领域一一对应，找代码时先定位领域：

| 领域 | 前端 | 后端 |
|---|---|---|
| 聊天 / LLM 调用 | `src/views/ChatView.vue` + `src/stores/chat.ts` | `src-tauri/src/commands/llm.rs` |
| Agent Team（多 Agent 工作组） | `AgentTeamView.vue` + `stores/workspace.ts` | `src-tauri/src/workspace/` |
| RAG 知识库 | `KnowledgeBaseView.vue` + `stores/knowledgeBase.ts` | `src-tauri/src/knowledge_base/` |
| MCP 工具 | `MCPView.vue` + `stores/mcp.ts` | `src-tauri/src/commands/mcp.rs` |
| 本地部署（Ollama/LM Studio/Docker） | `LocalDeployView.vue` + 对应 stores | `commands/local_model.rs`、`lmstudio.rs`、`docker.rs` |
| Skill | `SkillsView.vue` + `stores/skills.ts` | `commands/skills.rs` |
| 定时任务 | `SchedulerView.vue` + `stores/scheduler.ts` | `src-tauri/src/scheduler/` |

关键单点：

- **`commands/llm.rs`（~97KB）是全部 15+ 家 LLM 服务商的对接层**：`PROVIDER_CONFIGS`
  常量定义服务商清单/端点/认证方式，同文件内完成请求体构造（按 provider 分支处理
  多模态、思考模式、缓存）、SSE 流式解析、多轮 MCP 工具调用循环。改任何服务商行为都在这里。
  各家 API 的核对结论沉淀在 `docs/api-manuals/`，改对接代码前先读对应手册
  （系统性核对用 `audit-llm-providers` skill）。
- `main.rs` 注册所有 command 并 `app.manage()` 各领域 state（DbState、KbState、
  WorkspaceState 等）；新增 command 记得两处都登记。
- `db.rs` 是主 SQLite 层；knowledge_base 和 workspace 各有自己的 `db.rs`。
  API 密钥走 `secure_storage.rs` 本地加密，不进普通表。
- 前端路由是 hash 模式（`createWebHashHistory`）；Pinia 用 persistedstate 持久化。

## 运行时数据位置（两个目录名不同，别找错）

- 日志：`%APPDATA%\BaiyuAISpace2\logs\app_<date>.log`
- SQLite 数据库 + WebView2 profile：`%APPDATA%\com.baiyu.aispace\`

## 项目约定

- **提交**：未经用户明确要求不 commit/push。信息风格 `feat:`/`fix:`/`chore:` + 中文摘要。
- **UI**：严格的黑白编辑设计系统——无彩色、无圆角、指定字体与缓动曲线。
  动 UI 前必读 `design-system` skill；token 权威源是 `src/styles/variables.scss`。
  所有提示/报错/警告统一走左下角弹窗机制；表单必须写中文 placeholder。
- **超时**：流式响应与大文件下载禁用总超时，只用读间隔超时（历史上因此出过五处 bug）。
- **汇报**：发现问题除技术描述外，附一段人话解释；用户不在场时用钉钉 MCP 通知结果。
- 项目级 skills 在 `.claude/skills/`：`run-baiyuaispace2`（启动/驱动应用）、
  `self-test`（完整测试流程）、`audit-llm-providers`（API 手册审计）、
  `handle-issues`（GitHub Issue 处理）、`debug-from-logs`（报障排查）、
  `design-system`（UI 规范）。对应任务先用 skill，别从零推导。
