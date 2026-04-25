# MiniMax API 官方文档

## 官方文档地址

- **开发者平台**: https://platform.minimax.io/
- **API 文档**: https://platform.minimax.io/docs/api-reference

## API 端点

```
https://api.minimax.chat/v1/text/chatcompletion_v2
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.minimax.chat/v1/text/chatcompletion_v2 \
  -H "Authorization: Bearer $MINIMAX_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "abab6.5s-chat",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.minimax.chat/v1/text/chatcompletion_v2 \
  -H "Authorization: Bearer $MINIMAX_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "abab6.5s-chat",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- abab6.5s-chat
- abab6.5g-chat

## 注意事项

- 需要在 MiniMax 开放平台注册并获取 API Key

## 更新日志

- 2026-04-25: 初始文档
