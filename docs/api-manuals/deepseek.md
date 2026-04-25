# DeepSeek API 官方文档

## 官方文档地址

- **API 文档**: https://api-docs.deepseek.com/
- **快速开始**: https://api-docs.deepseek.com/quick-start
- **模型列表**: https://api-docs.deepseek.com/models

## API 端点

```
https://api.deepseek.com/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.deepseek.com/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.deepseek.com/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- deepseek-chat (DeepSeek-V3)
- deepseek-reasoner (DeepSeek-R1)

## 更新日志

- 2026-04-25: 初始文档
