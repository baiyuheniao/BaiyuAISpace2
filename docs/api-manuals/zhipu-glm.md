# 智谱 AI (GLM) API 官方文档

## 官方文档地址

- **主文档**: https://docs.bigmodel.cn/
- **快速开始**: https://docs.bigmodel.cn/cn/guide/start/quick-start
- **API 参考**: https://docs.bigmodel.cn/cn/api

## API 端点

```
https://open.bigmodel.cn/api/paas/v4/chat/completions
```

## 认证方式

```http
Authorization: Bearer YOUR_API_KEY
```

## 请求示例

```bash
curl https://open.bigmodel.cn/api/paas/v4/chat/completions \
  -H "Authorization: Bearer $ZHIPU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "glm-4-plus",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://open.bigmodel.cn/api/paas/v4/chat/completions \
  -H "Authorization: Bearer $ZHIPU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "glm-4-plus",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 图片输入格式 (Vision, GLM-4V 系列)

在这个 OpenAI 兼容端点下，`image_url` 是对象，且**需要带 `data:image/jpeg;base64,` 前缀**：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

（注意：智谱**原生**（非 OpenAI 兼容）接口的部分示例代码里 `url` 直接放纯 base64、不带前缀 —— 那是另一套接口，不是本表用的这个 OpenAI 兼容端点，别搞混。core 里走的是本节这个带前缀的格式，已核实与代码一致。）

## 常用模型

- glm-4-plus
- glm-4-flash
- glm-4
- glm-3-turbo
- glm-4v-plus / glm-4.5v / glm-4.6v（视觉）

## 更新日志

- 2026-07-02: 核实并补充图片输入格式（OpenAI 兼容端点下需要 data URI 前缀）
- 2026-04-25: 初始文档
