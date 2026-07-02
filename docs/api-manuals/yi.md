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

## 图片输入格式 (Vision, yi-vision 系列)

官方称接口"OpenAI 兼容"，推测走标准格式（`image_url` 是对象、data URI 前缀），但**没有找到零一万物官方对 image_url 精确 JSON 结构的原始文档确认**，可信度不是 100%，如果实际接入时遇到解析失败，优先怀疑这一点：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

需要用 `yi-vision` 系列模型。

## 常用模型

- yi-large
- yi-medium
- yi-small
- yi-vision（视觉，格式未 100% 核实）

## 注意事项

- 也可通过阿里云百炼平台调用: https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation

## 更新日志

- 2026-07-02: 补充图片输入格式（未找到官方原始示例 100% 确认，标注为待核实）
- 2026-04-25: 初始文档
