# Mistral AI API 官方文档

## 官方文档地址

- **主文档**: https://docs.mistral.ai/
- **API 参考**: https://docs.mistral.ai/api/
- **模型列表**: https://docs.mistral.ai/models/

## API 端点

```
https://api.mistral.ai/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.mistral.ai/v1/chat/completions \
  -H "Authorization: Bearer $MISTRAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large-latest",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.mistral.ai/v1/chat/completions \
  -H "Authorization: Bearer $MISTRAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "mistral-large-latest",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision) ⚠️ 与 OpenAI 不同

**`image_url` 字段直接是字符串，不是 `{"url": ...}` 对象**（这是本表所有服务商里唯一的例外，其余全部走 OpenAI 的对象嵌套格式）：

```json
{
  "role": "user",
  "content": [
    {"type": "text", "text": "这张图片里有什么？"},
    {"type": "image_url", "image_url": "data:image/jpeg;base64,<BASE64>"}
  ]
}
```

来源：https://docs.mistral.ai/capabilities/vision （确认于 2026-07-02，`"image_url": "data:image/jpeg;base64,{base64_image}"` 是官方给出的原始示例，没有嵌套 `url` 键）。

Pixtral 系列（如 `pixtral-large-latest`）是 Mistral 的视觉模型。

代码实现见 `src-tauri/src/commands/llm.rs` 的 `build_stream_request_body`，`provider == "mistral"` 时会走这个特殊分支，不套用其他厂商共用的 `{"url": ...}` 格式。

## 常用模型

- mistral-large-latest
- mistral-small-latest
- mistral-medium
- codestral-latest
- pixtral-large-latest（视觉）

## 更新日志

- 2026-07-02: 核实并补充图片输入格式 —— image_url 是裸字符串，不是对象，与 OpenAI 及其余所有服务商不同
- 2026-04-25: 初始文档
