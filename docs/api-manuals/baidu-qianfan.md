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

## 图片输入格式 (Vision, ERNIE 系列)

标准 OpenAI 兼容格式，`image_url` 是对象，支持 http(s) 链接或 base64 data URI：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>", "detail": "auto"}}
```

支持 JPG/JPEG/PNG/BMP；单图不超过 10MB，单次请求最多 10 张图，总大小控制在 20MB 内；可选 `detail` 参数（`low`/`high`/`auto`）控制分辨率理解精度和 token 消耗。

## 常用模型

- ernie-4.5-8k
- ernie-4.0-8k
- ernie-3.5-8k
- ernie-bot-turbo

## 注意事项

- access_token 默认有效期 30 天
- 需要使用 API Key + Secret Key 换取 access_token
- ⚠️ 待核实：代码里 `PROVIDER_CONFIGS` 目前把百度归类为 `bearer` 认证、直接用用户填的 API Key 当 Bearer token；如果账号确实要求先用 API Key+Secret Key 走 OAuth2 换 access_token 才能调用，现有代码可能无法直接工作，需要另外排查（这次审查范围是图片/视频参数格式，没有深入这一点）

## 更新日志

- 2026-07-02: 补充图片输入格式说明；记录一处待核实的认证机制疑点
- 2026-04-25: 初始文档
