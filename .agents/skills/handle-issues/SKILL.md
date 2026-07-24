---
name: handle-issues
description: Triage and resolve GitHub issues for baiyuheniao/BaiyuAISpace2 end-to-end (evaluate, fix, verify build, comment/close, report). Use when asked to 处理一下项目的 Issue / 看看 Issues / 提个 Issue 记录一下.
---

用户会不定期说"处理一下项目的 Issue"然后离开，期望自主完成并汇报。

## 仓库与工具

- 仓库：`baiyuheniao/BaiyuAISpace2`（origin 已配置）。
- GitHub MCP 工具（`mcp__github__*`）已配置，`gh` CLI 也可用，任选。

## 处理流程

1. 列出全部 open issues，逐个评估：
   - **不合理/超纲的**：跳过，在汇报里写明跳过原因（用户授权过这种判断）。
   - **需要讨论的设计类**：不擅自定方案，留言或汇报时列出选项。
   - **明确的 bug/改进**：直接修。
2. 修复注意：
   - 改动大的先想清楚副作用再动手。历史教训：Issue #18 给向量检索加
     `LIMIT 50000` 被用户驳回——超限文档对 RAG 完全不可见，最终改成流式
     top-k 堆。**修复不能引入新的能力截断**，同类取舍要在汇报中主动指出。
   - 修完必跑 `pnpm tauri build` 验证编译；涉及运行时行为的用
     `run-baiyuaispace2` skill 实际跑一下。
3. 收尾：
   - 在 issue 下留言说明修复内容；确认修复后可 close。
   - 未经用户明确要求不 commit/push；如果用户出门前说了"干好了提交"，
     模仿现有 commit 风格（`fix:`/`feat:` + 中文摘要）。
   - 用户不在场时用钉钉 MCP 发详细汇报：处理了哪些、跳过了哪些及原因、
     每个问题附人话解释。

## 反向操作：记录新 Issue

用户说"提个 Issue 记录一下"时：用中文写清现象、复现方式、影响面、
暂缓处理的原因，套用仓库里已有 issue 的行文风格。
