# 零一万物 (Yi) API 官方文档

## 官方文档地址

- **开发者平台**: https://www.01.ai/
- **API 文档**: https://help.aliyun.com/zh/model-studio/developer-reference/yi-large-llm/

## API 端点

```
https://api.lingyiwanwu.com/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.lingyiwanwu.com/v1/chat/completions \
  -H "Authorization: Bearer $YI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "yi-large",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.lingyiwanwu.com/v1/chat/completions \
  -H "Authorization: Bearer $YI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "yi-large",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- yi-large
- yi-medium
- yi-small

## 注意事项

- 也可通过阿里云百炼平台调用: https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation

## 更新日志

- 2026-04-25: 初始文档
