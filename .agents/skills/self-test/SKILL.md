---
name: self-test
description: Run an autonomous full test pass of BaiyuAISpace2 (backend gates + driving the real UI + live-model tests via Ollama/LM Studio), following the repo's test plan docs. Use when asked to 做一次完整测试 / 自己测试 / 端到端测试 / 按测试方案测一遍, or to verify a batch of changes across modules.
---

用户会周期性地要求"自己做一次完整测试，前后端都做"。流程和资源如下。

## 测试依据文档

- `docs/SOFTWARE_TESTING_PLAN.md` — 完整测试方案（200+ 用例，按模块分）
- `docs/MANUAL_TEST_CHECKLIST.md` — 全场景人工测试清单

以文档为纲逐模块执行，能自动化的自动化，不能的记入"未测"并说明原因。

## 测试顺序（用户指定过：先后端再前端）

1. **后端门禁**：`pnpm build`（vue-tsc + vite）、`cd src-tauri && cargo build`。
   仓库没有常规测试套件；`src-tauri/src/workspace_smoke_test.rs` 是
   Agent Team 的冒烟测试，可单独跑。
2. **前端/整体**：用 `run-baiyuaispace2` skill 的 driver 启动真实应用，
   逐视图导航、截图、点击核心流程（Chat / History / 知识库 / MCP / Skill /
   本地部署 / Agent Team / Settings）。MCP 和 Skill 模块容易漏测，
   用户专门补充要求过，必须包含。

## 真实模型资源

- **Ollama**：本机已装。先 `ollama list` 看现有模型（之前拉过 qwen3.5:4b、
  gemma4:e4b 级别的小模型），缺了再拉小参数模型。用于 Chat、Agent Team
  会议等需要真实 LLM 的用例。
- **LM Studio**：本机已装，但 Local Server 需要用户手动开启。
  要测 LM Studio 路径时先确认服务在（默认 `http://localhost:1234/v1`），
  不在就跳过并在报告里注明，不要干等。
- **云端 API**：软件内已有用户配置的 API（如硅基流动、DeepSeek），
  可直接用于真实请求测试，注意控制调用量。

## 结果呈现（用户明确要求过的格式）

- 输出一张表格：**测了什么 / 结果 / 没测什么及原因**。
- 发现 bug：先修，修完 `pnpm tauri build` 验证编译，再回归该用例。
- 每个问题除技术描述外附一段人话解释。
- 测试涉及 UI 时顺手记录排版不合理之处，用户之前专门问过这个。
- 未经用户明确要求不 commit；用户不在场时用钉钉 MCP 汇报结果。
