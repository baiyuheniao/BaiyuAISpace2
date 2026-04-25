# Google Gemini API 官方文档

## 官方文档地址

- **主文档**: https://ai.google.dev/gemini-api/docs
- **快速开始**: https://ai.google.dev/docs
- **API 参考**: https://ai.google.dev/docs/api_rest
- **模型列表**: https://ai.google.dev/models/gemini

## API 端点

```
https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent
```

## 认证方式

```http
x-goog-api-key: YOUR_API_KEY
```

## 请求示例

```bash
curl "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=$GOOGLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [{"text": "Hello!"}]
    }]
  }'
```

## 流式请求

```bash
curl "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse&key=$GOOGLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [{"text": "Hello!"}]
    }],
    "generationConfig": {
      "maxOutputTokens": 1024
    }
  }'
```

## 常用模型

- gemini-2.5-pro
- gemini-2.0-flash
- gemini-1.5-pro
- gemini-1.5-flash

## 更新日志

- 2026-04-25: 初始文档
