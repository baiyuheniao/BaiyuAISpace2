# 百度千帆 API 官方文档

## 官方文档地址

- **主文档**: https://cloud.baidu.com/doc/WENXINWORKSHOP/
- **快速开始**: https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Um2wxbaps
- **API 列表**: https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Clntwmv7t
- **OpenAI 兼容**: https://help.aliyun.com/zh/model-studio/developer-reference/yi-large-llm/

## API 端点 (v2 OpenAI 兼容模式)

```
https://qianfan.baidubce.com/v2/chat/completions
```

## 认证方式

**重要**: 百度千帆需要使用 access_token，而非直接的 API Key。

1. 获取 API Key 和 Secret Key: https://console.bce.baidu.com/qianfan/
2. 通过 OAuth2 换取 access_token:

```bash
curl -X POST 'https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id=YOUR_API_KEY&client_secret=YOUR_SECRET_KEY'
```

获取 access_token 后，使用以下认证方式：

```http
Authorization: Bearer YOUR_ACCESS_TOKEN
```

## 请求示例

```bash
curl https://qianfan.baidubce.com/v2/chat/completions \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ernie-4.5-8k",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 流式请求

```bash
curl https://qianfan.baidubce.com/v2/chat/completions \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "ernie-4.5-8k",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

## 常用模型

- ernie-4.5-8k
- ernie-4.0-8k
- ernie-3.5-8k
- ernie-bot-turbo

## 注意事项

- access_token 默认有效期 30 天
- 需要使用 API Key + Secret Key 换取 access_token

## 更新日志

- 2026-04-25: 初始文档
