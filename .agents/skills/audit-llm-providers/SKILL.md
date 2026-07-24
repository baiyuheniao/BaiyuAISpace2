---
name: audit-llm-providers
description: Audit BaiyuAISpace2's backend LLM provider integrations against each provider's official API manual. Use when asked to 核对/检查各家 API 服务商手册、检查图片/视频/多模态参数传递、检查缓存/上下文/流式兼容性, or when a provider-specific API bug is suspected (wrong format, hardcoded param, missing capability).
---

BaiyuAISpace2 后端对接了 15+ 家 LLM API 服务商。这类审计任务用户已提出过多次
（多模态格式核对、Prompt Caching 兼容、Max Tokens 硬编码、无状态上下文拼接等），
流程固定如下。

## 代码位置

- 核心对接代码：`src-tauri/src/commands/llm.rs`
  - `PROVIDER_CONFIGS`（约 179 行）：全部服务商清单 = openai / anthropic /
    google / azure / mistral / moonshot / zhipu / aliyun / baidu / doubao /
    deepseek / siliconflow / minimax / yi / local / custom。
    以代码里的这个常量为准，不要凭记忆列清单。
  - `build_url`、请求体构造、流式解析都在同一文件，按 provider 名 grep。
- Embedding 服务商在 `src-tauri/src/knowledge_base/embedding.rs`（openai /
  zhipu / siliconflow），审计范围含 RAG 时一并核对。

## 本地手册库（先读这个，再查网络）

`docs/api-manuals/` 下每家一份手册（`anthropic.md`、`google-gemini.md`、
`deepseek.md` 等，见该目录 README）。这是之前几轮审计沉淀下来的：

1. 先读本地手册了解已核对过的结论和已知差异；
2. 再用 WebSearch/WebFetch 查官方最新文档，确认手册没有过时；
3. 发现手册与官方文档不一致时，**先更新手册再改代码**，让手册保持可信。

## 审计流程

对每家服务商逐项核对代码 vs 手册：

1. 端点 URL、认证头（bearer / x-api-key）、必需 header（如 anthropic-version）
2. 请求体格式：消息结构、system 的位置、参数名与取值范围
3. 多模态：图片/视频的传递格式（这里历史上出过 bug：Mistral 图片格式错误、
   Gemini 视频要用 inline_data base64）。各家格式互不相同，逐家核对。
4. 流式：SSE 事件格式、工具调用增量拼接、`[DONE]` 处理
5. 上下文机制：无状态 API（如 DeepSeek）需要客户端拼完整历史；
   缓存机制（Anthropic prompt caching、Gemini context caching）是否利用
6. 参数是否硬编码：历史教训是 Anthropic 的 max_tokens 曾被写死，
   凡是用户应可配置的参数都检查一遍

## 验证与收尾

- 用户配置过 API Key 的服务商，直接在软件里发真实请求验证（含发图片测多模态）。
  启动和操作软件用 `run-baiyuaispace2` skill。
- 改完代码必须跑 `pnpm tauri build` 验证编译（用户硬性要求）。
- 汇报时每个问题除技术描述外，附一段人话解释（用户硬性要求）。
- 用户不在场时，完成后用钉钉 MCP 发详细汇报。
