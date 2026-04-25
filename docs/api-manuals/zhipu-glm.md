# 智谱 AI (GLM) API 官方文档

## 官方文档地址

- **主文档**: https://docs.bigmodel.cn/
- **快速开始**: https://docs.bigmodel.cn/cn/guide/start/quick-start
- **API 参考**: https://docs.bigmodel.cn/cn/api

## API 端点

```
https://open.bigmodel.cn/api/paas/v4/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://open.bigmodel.cn/api/paas/v4/chat/completions \
  -H "Authorization: Bearer $ZHIPU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "glm-4-plus",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://open.bigmodel.cn/api/paas/v4/chat/completions \
  -H "Authorization: Bearer $ZHIPU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "glm-4-plus",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- glm-4-plus
- glm-4-flash
- glm-4
- glm-3-turbo

## 更新日志

- 2026-04-25: 初始文档
