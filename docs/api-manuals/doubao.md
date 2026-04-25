# 字节豆包 API 官方文档

## 官方文档地址

- **主文档**: https://www-doubao.com/apidoc/
- **开发者平台**: https://developer.open-douyin.com/docs/resource/zh-CN/ai-avatar

## API 端点 (火山引擎)

```
https://ark.cn-beijing.volces.com/api/v3/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://ark.cn-beijing.volces.com/api/v3/chat/completions \
  -H "Authorization: Bearer $DOUBAO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "doubao-pro-v1",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://ark.cn-beijing.volces.com/api/v3/chat/completions \
  -H "Authorization: Bearer $DOUBAO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "doubao-pro-v1",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- doubao-pro-v1
- doubao-lite-v1

## 注意事项

- 需要在火山引擎方舟平台创建应用
- 需要获取 API Key

## 更新日志

- 2026-04-25: 初始文档
