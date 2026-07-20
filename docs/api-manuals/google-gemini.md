# Google Gemini API 官方文档

## 官方文档地址

- **主文档**: https://ai.google.dev/gemini-api/docs
- **快速开始**: https://ai.google.dev/docs
- **API 参考**: https://ai.google.dev/docs/api_rest
- **模型列表**: https://ai.google.dev/models/gemini

## API 端点

```
https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent
```

## 认证方式

```http
x-goog-api-key: YOUR_API_KEY
```

## 请求示例

```bash
curl "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=$GOOGLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [{"text": "Hello!"}]
    }]
  }'
```

## 流式请求

```bash
curl "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse&key=$GOOGLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [{"text": "Hello!"}]
    }],
    "generationConfig": {
      "maxOutputTokens": 1024
    }
  }'
```

## 图片/视频输入格式 (Vision)

图片和视频用同一种 `inline_data` part，`mime_type` + 纯 base64（不带 data URI 前缀）：

```json
{
  "contents": [{
    "role": "user",
    "parts": [
      {"text": "这段内容里有什么？"},
      {"inline_data": {"mime_type": "image/jpeg", "data": "<BASE64_无前缀>"}},
      {"inline_data": {"mime_type": "video/mp4", "data": "<BASE64_无前缀>"}}
    ]
  }]
}
```

Gemini 是本表里目前唯一同时支持图片和视频输入的服务商（代码里 `ChatMessage.videos` 字段目前也只有 google 分支会消费；其余 provider 会忽略视频附件，详见 `src-tauri/src/commands/llm.rs` 顶部的字段注释）。

## Context Caching (上下文缓存)

Gemini 2.5+ 模型的**隐式缓存 (implicit caching)** 是服务端默认自动开启的，重复的历史前缀会自动打 9 折左右，**不需要客户端做任何代码改动**。显式缓存 (`cachedContent` 字段) 需要额外调用独立的缓存管理 API 创建/维护缓存对象，且门槛是 32768 tokens 起，对常规聊天历史场景用不上，目前没有适配，也没有必要适配。

## 常用模型

（模型名是用户自由填写的字符串，代码不做硬编码校验，以下为 2026-07 时点的当前情况，会持续变化）

- gemini-3.1-pro（当前旗舰）
- gemini-2.5-pro（GA 稳定版，官方计划支持到 2026-10-16）
- gemini-2.0 系列已下线，不要再作为默认推荐

## 更新日志

- 2026-07-20: 更新常用模型列表，标注 2.0 系列已下线、2.5 GA 支持截止日期
- 2026-07-02: 补充图片/视频输入格式，以及 Context Caching (隐式缓存自动生效，无需适配) 说明
- 2026-04-25: 初始文档
