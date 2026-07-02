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

## 图片输入格式 (Vision, moonshot-v1-vision 系列)

标准 OpenAI 兼容格式，`image_url` 是对象：

```json
{"type": "image_url", "image_url": {"url": "data:image/png;base64,<BASE64>"}}
```

**注意：Kimi Vision 只支持 base64 格式，不支持传 http(s) 图片 URL**，这点跟大部分其他厂商不同。需要用 `moonshot-v1-vision-preview` 之类的视觉专用模型。

## 常用模型

- kimi-k2.5
- kimi-k2-thinking
- kimi-k1.5
- moonshot-v1-8k
- moonshot-v1-vision-preview（视觉，仅支持 base64）

## 更新日志

- 2026-07-02: 补充图片输入格式说明，标注仅支持 base64、不支持 URL
- 2026-04-25: 初始文档
