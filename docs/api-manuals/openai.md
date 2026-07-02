# OpenAI API 官方文档

## 官方文档地址

- **主文档**: https://platform.openai.com/docs
- **API 参考**: https://platform.openai.com/docs/api-reference
- **模型列表**: https://platform.openai.com/docs/models

## API 端点

```
https://api.openai.com/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision)

`content` 是数组，图片以 `image_url` 类型传入，`image_url` 是**对象**，`url` 字段既可以是 http(s) 链接，也可以是 base64 data URI：

```json
{
  "role": "user",
  "content": [
    {"type": "text", "text": "这张图片里有什么？"},
    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
  ]
}
```

这是本表里其他"OpenAI 兼容"服务商（DeepSeek、SiliconFlow、智谱、阿里云、百度、豆包、Moonshot、MiniMax、Yi 等）默认遵循的基准格式，**唯一已知的例外是 Mistral**（见 [mistral.md](./mistral.md)，`image_url` 直接是字符串，不是对象）。

支持格式通常为 PNG / JPEG / WEBP / GIF（静态帧）。

## 常用模型

- gpt-5, gpt-5-codex, gpt-4.1
- gpt-4o, gpt-4o-mini
- o3, o4-mini

## 更新日志

- 2026-07-02: 补充图片输入 (Vision) 格式说明，标注 Mistral 是唯一的格式例外
- 2026-04-25: 初始文档
