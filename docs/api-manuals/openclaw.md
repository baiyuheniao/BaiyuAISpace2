# OpenClaw OpenAI 兼容 API 手册

## 官方文档地址

- **项目文档**: https://docs.openclaw.ai/
- **网关配置**: https://docs.openclaw.ai/gateway/configuration
- **模型与提供商**: https://docs.openclaw.ai/providers

## 代码端点

项目将 OpenClaw 作为本地 OpenAI 兼容网关使用：

```text
http://127.0.0.1:18789/v1/chat/completions
```

端点是否启用取决于 `gateway.http.endpoints.chatCompletions.enabled`。默认情况下网关认证也应保持开启，客户端使用与 `gateway.auth.token` 相同的 Bearer token。

## 兼容边界

- 请求体与 Chat Completions 的 `messages`、`model`、`stream` 等字段保持 OpenAI 兼容形态。
- 实际可用模型、工具调用和流式事件取决于 OpenClaw 网关配置及其后端模型提供商。
- OpenClaw 本地网关不是独立的模型供应商；这里记录的是 BaiyuAISpace2 的适配入口，不替代具体上游供应商手册。

## 维护说明

本文件记录的是仓库代码中的本地网关适配约定。OpenClaw 的配置项和兼容端点可能随版本变化，升级网关后应重新核对上述官方文档。
