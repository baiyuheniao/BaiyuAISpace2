# DeepSeek API 官方文档

## 官方文档地址

- **API 文档**: https://api-docs.deepseek.com/
- **快速开始**: https://api-docs.deepseek.com/quick-start
- **模型列表**: https://api-docs.deepseek.com/models

## API 端点

```
https://api.deepseek.com/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.deepseek.com/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.deepseek.com/chat/completions \
  -H "Authorization: Bearer $DEEPSEEK_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片/视频输入 (Vision)

**DeepSeek 官方 API 目前不支持图片/视频输入**，`deepseek-chat`/`deepseek-reasoner` 都是纯文本模型。代码里如果给 DeepSeek 发带图片的消息，请求会按标准 OpenAI `image_url` 格式发出（走 `build_stream_request_body` 的通用分支），但 DeepSeek 服务端大概率无法识别，不会报格式错误，而是直接忽略图片内容或返回错误 —— 应用层面目前没有针对这种情况做拦截提示。

## 常用模型

- deepseek-chat (DeepSeek-V3)
- deepseek-reasoner (DeepSeek-R1)

## 更新日志

- 2026-07-02: 核实确认官方 API 不支持图片/视频输入
- 2026-04-25: 初始文档
