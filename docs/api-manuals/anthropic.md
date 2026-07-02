# Anthropic Claude API 官方文档

## 官方文档地址

- **主文档**: https://docs.anthropic.com/en/docs
- **Messages API**: https://docs.anthropic.com/en/api/messages
- **模型列表**: https://docs.anthropic.com/en/docs/models

## API 端点

```
https://api.anthropic.com/v1/messages
```

## 认证方式

```http
x-api-key: YOUR_API_KEY
anthropic-version: 2023-06-01
```

## 请求示例

```bash
curl https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.anthropic.com/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision)

图片不用 `image_url`，用独立的 `image` content block，`source` 是对象，`data` 字段是**纯 base64**（不带 `data:...;base64,` 前缀，前缀会被当成非法 base64 导致解析失败）：

```json
{
  "role": "user",
  "content": [
    {"type": "text", "text": "这张图片里有什么？"},
    {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "<BASE64_无前缀>"}}
  ]
}
```

## Prompt Caching (提示缓存)

在 content block 上加 `cache_control: {"type": "ephemeral"}` 即可，不需要额外的 beta 请求头（已转正）。最多 4 个显式缓存断点，`system` 和 `messages` 历史都能用。对多轮对话，最佳实践是把断点放在"最新一条消息之前的最后一条"上，这样每轮只需为新增内容重新计费。默认 5 分钟 TTL，`ttl: "1h"` 可选但按 2 倍基础输入价计费。代码实现见 `src-tauri/src/commands/llm.rs` 的 `build_stream_request_body` anthropic 分支。

## 常用模型

- claude-opus-4-20250514
- claude-sonnet-4-20250514
- claude-3-5-sonnet-20241022
- claude-3-opus-20240229

## 更新日志

- 2026-07-02: 补充图片输入格式 (data 字段为纯 base64，无 data URI 前缀) 与 Prompt Caching 说明
- 2026-04-25: 初始文档
