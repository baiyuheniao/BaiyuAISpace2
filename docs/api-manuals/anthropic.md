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

## 常用模型

- claude-opus-4-20250514
- claude-sonnet-4-20250514
- claude-3-5-sonnet-20241022
- claude-3-opus-20240229

## 更新日志

- 2026-04-25: 初始文档
