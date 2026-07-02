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

## 图片输入格式 (Vision, 豆包多模态系列)

标准 OpenAI 兼容格式，`image_url` 是对象，`url` 支持 http(s)/tos/s3 链接或 base64 data URI：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

需要用支持视觉的豆包模型（模型名通常带"vision"或走多模态专用 endpoint/模型 ID）。

## 常用模型

- doubao-pro-v1
- doubao-lite-v1

## 注意事项

- 需要在火山引擎方舟平台创建应用
- 需要获取 API Key

## 更新日志

- 2026-07-02: 补充图片输入格式说明
- 2026-04-25: 初始文档
