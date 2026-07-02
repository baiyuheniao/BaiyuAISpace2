# 阿里云通义千问 API 官方文档

## 官方文档地址

- **主文档**: https://help.aliyun.com/zh/model-studio/
- **Qwen API**: https://help.aliyun.com/zh/model-studio/developer-reference/completions
- **OpenAI 兼容接口**: https://help.aliyun.com/zh/model-studio/qwen-api-reference

## API 端点 (OpenAI 兼容模式)

```
https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions \
  -H "Authorization: Bearer $DASHSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "qwen-plus",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions \
  -H "Authorization: Bearer $DASHSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "qwen-plus",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision, Qwen-VL 系列)

标准 OpenAI 兼容格式，`image_url` 是对象，支持 http(s) 链接或 base64 data URI：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

视觉模型需要用专门的 VL 系列模型名（如 `qwen3-vl-plus`、`qwen-vl-max`、`qwen2.5-vl-72b-instruct`），普通 `qwen-plus`/`qwen-turbo` 不支持图片输入。

## 常用模型

- qwen-plus
- qwen-turbo
- qwen-max
- qwen-coder-plus
- qwen2.5
- qwen3-vl-plus / qwen-vl-max / qwen2.5-vl-72b-instruct（视觉）

## 注意事项

- 需要在阿里云百炼平台获取 API Key
- 支持 OpenAI 兼容模式

## 更新日志

- 2026-07-02: 核实并补充图片输入格式说明
- 2026-04-25: 初始文档
