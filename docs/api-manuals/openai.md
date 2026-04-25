# OpenAI API 官方文档

## 官方文档地址

- **主文档**: https://platform.openai.com/docs
- **API 参考**: https://platform.openai.com/docs/api-reference
- **模型列表**: https://platform.openai.com/docs/models

## API 端点

```
https://api.openai.com/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- gpt-5, gpt-5-codex, gpt-4.1
- gpt-4o, gpt-4o-mini
- o3, o4-mini

## 更新日志

- 2026-04-25: 初始文档
