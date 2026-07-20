# MiniMax API 官方文档

## 官方文档地址

- **开发者平台**: https://platform.minimax.io/
- **API 文档**: https://platform.minimax.io/docs/api-reference

## API 端点

```
https://api.minimax.io/v1/text/chatcompletion_v2
```

⚠️ 域名是 `api.minimax.io`，不是 `api.minimax.chat`（旧文档常见笔误，代码 `PROVIDER_CONFIGS` 用的是 `.io`）。另外 `chatcompletion_v2` 目前官方已标记为 deprecated，推荐迁移路径是 OpenAI 兼容的 `/v1/chat/completions`；`.io` + `chatcompletion_v2` 组合仍可用，但属于过渡期端点，未来可能下线。

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://api.minimax.io/v1/text/chatcompletion_v2 \
  -H "Authorization: Bearer $MINIMAX_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "abab6.5s-chat",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://api.minimax.io/v1/text/chatcompletion_v2 \
  -H "Authorization: Bearer $MINIMAX_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "abab6.5s-chat",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片/视频输入格式 (Vision)

`chatcompletion_v2` 端点虽然路径长得不像标准 OpenAI 路径（`/v1/text/chatcompletion_v2`），但请求体本身是 OpenAI 兼容格式。较新的 MiniMax 模型（如 MiniMax-M3）支持 `image_url` **和** `video_url` 两种 content part：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
{"type": "video_url", "video_url": {"url": "..."}}
```

⚠️ **代码目前没有适配 MiniMax 的视频输入**：`src-tauri/src/commands/llm.rs` 里视频（`ChatMessage.videos`）只有 google 分支会消费，MiniMax 走的通用分支完全不处理 `videos` 字段，用户上传的视频会被静默丢弃。如果要支持 MiniMax 的视频理解，需要单独适配 `video_url` 格式（跟 Gemini 的 `inline_data` 格式不同，不能直接复用）。

## 常用模型

- abab6.5s-chat
- abab6.5g-chat
- MiniMax-M3（多模态，支持图片 + 视频）

## 注意事项

- 需要在 MiniMax 开放平台注册并获取 API Key
- 端点路径 `/v1/text/chatcompletion_v2` 与其他厂商的 `/v1/chat/completions` 不同，但请求/响应体格式仍是 OpenAI 兼容的

## 更新日志

- 2026-07-20: 修正端点域名笔误（`.chat` → `.io`，与代码一致）；补充 `chatcompletion_v2` 已被官方标记 deprecated 的提醒
- 2026-07-02: 补充图片/视频输入格式说明；记录代码尚未适配 MiniMax 视频输入这一缺口
- 2026-04-25: 初始文档
