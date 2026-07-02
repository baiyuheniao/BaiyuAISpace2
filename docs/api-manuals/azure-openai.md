# Azure OpenAI API 官方文档

## 官方文档地址

- **主文档**: https://learn.microsoft.com/en-us/azure/ai-services/openai/
- **Chat API**: https://learn.microsoft.com/en-us/azure/ai-services/openai/reference#chat-completions
- **模型列表**: https://learn.microsoft.com/en-us/azure/ai-services/openai/concepts/models

## API 端点

```
https://{your-resource}.openai.azure.com/openai/deployments/{deployment-name}/chat/completions?api-version=2023-05-15
```

## 认证方式

```http
api-key: YOUR_API_KEY
```

## 请求示例

```bash
curl https://your-resource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2023-05-15 \
  -H "api-key: $AZURE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://your-resource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2023-05-15 \
  -H "api-key: $AZURE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision)

跟 OpenAI 完全一致（Azure 是官方套壳），`image_url` 是对象：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

## 常用模型

- gpt-4o
- gpt-4-turbo
- gpt-35-turbo

## 注意事项

- 需要在 Azure 门户创建 OpenAI 资源
- 需要部署模型并获取部署名称
- API 版本需要指定
- ⚠️ 待核实：本文档顶部写的认证方式是 `api-key: YOUR_API_KEY`（独立请求头），但代码 `PROVIDER_CONFIGS` 里 azure 目前配的认证类型是 `bearer`（即 `Authorization: Bearer ...`）—— 这两者不一致，如果这次没有确认过 azure 配置能实际跑通，这里可能是个未验证过的坑，这次审查范围是图片格式没有深入这一点

## 更新日志

- 2026-07-02: 补充图片输入格式；记录一处认证头疑似不一致的待核实点
- 2026-04-25: 初始文档
