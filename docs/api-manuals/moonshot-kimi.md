# Moonshot (Kimi) API 官方文档

## 官方文档地址

- **开发者平台**: https://platform.moonshot.cn/
- **API 文档**: https://platform.moonshot.cn/docs/api
- **模型列表**: https://platform.moonshot.cn/pricing

## API 端点

```
https://api.moonshot.cn/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.moonshot.cn/v1/chat/completions \
  -H "Authorization: Bearer $MOONSHOT_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "kimi-k2.5",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.moonshot.cn/v1/chat/completions \
  -H "Authorization: Bearer $MOONSHOT_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "kimi-k2.5",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- kimi-k2.5
- kimi-k2-thinking
- kimi-k1.5
- moonshot-v1-8k

## 更新日志

- 2026-04-25: 初始文档
