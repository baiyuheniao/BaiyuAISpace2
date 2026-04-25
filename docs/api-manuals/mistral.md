# Mistral AI API 官方文档

## 官方文档地址

- **主文档**: https://docs.mistral.ai/
- **API 参考**: https://docs.mistral.ai/api/
- **模型列表**: https://docs.mistral.ai/models/

## API 端点

```
https://api.mistral.ai/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.mistral.ai/v1/chat/completions \
  -H "Authorization: Bearer $MISTRAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large-latest",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.mistral.ai/v1/chat/completions \
  -H "Authorization: Bearer $MISTRAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large-latest",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- mistral-large-latest
- mistral-small-latest
- mistral-medium
- codestral-latest

## 更新日志

- 2026-04-25: 初始文档
