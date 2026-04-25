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

## 常用模型

- gpt-4o
- gpt-4-turbo
- gpt-35-turbo

## 注意事项

- 需要在 Azure 门户创建 OpenAI 资源
- 需要部署模型并获取部署名称
- API 版本需要指定

## 更新日志

- 2026-04-25: 初始文档
