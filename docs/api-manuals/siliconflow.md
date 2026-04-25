# 硅基流动 (SiliconFlow) API 官方文档

## 官方文档地址

- **主文档**: https://docs.siliconflow.cn/
- **API 参考**: https://docs.siliconflow.cn/api-reference

## API 端点

```
https://api.siliconflow.cn/v1/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.siliconflow.cn/v1/chat/completions \
  -H "Authorization: Bearer $SILICONFLOW_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Qwen/Qwen2.5-7B-Instruct",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.siliconflow.cn/v1/chat/completions \
  -H "Authorization: Bearer $SILICONFLOW_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Qwen/Qwen2.5-7B-Instruct",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- Qwen/Qwen2.5-7B-Instruct
- Qwen/Qwen2.5-72B-Instruct
- deepseek-ai/DeepSeek-V3
- deepseek-ai/DeepSeek-R1
- meta-llama/Llama-3.1-70B-Instruct

## 注意事项

- 聚合了多种开源模型的 API
- 价格相对优惠

## 更新日志

- 2026-04-25: 初始文档
