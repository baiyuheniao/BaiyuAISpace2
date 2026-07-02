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

## 图片输入格式 (Vision)

标准 OpenAI 兼容格式，`image_url` 是对象：

```json
{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,<BASE64>"}}
```

**实测已验证**（2026-07-02，`Qwen/Qwen3.5-9B`，真实图片 + 真实 API Key）：模型正确识别出图片是"蓝色流动感抽象 3D 渲染图（Windows 11 默认壁纸）"，说明代码里通用分支的图片格式对 SiliconFlow 是正确的。注意并非该平台所有模型都支持视觉/都处于可用状态——测试中 `Qwen/Qwen2.5-VL-72B-Instruct` 返回过 `{"code":30003,"message":"Model disabled."}`，这是账号侧模型开通状态问题，不是格式问题。

## 注意事项

- 聚合了多种开源模型的 API
- 价格相对优惠
- 部分模型可能因账号权限被禁用 (`Model disabled`)，与请求格式无关

## 更新日志

- 2026-07-02: 补充图片输入格式，并用真实 API Key + 真实图片完成端到端实测验证
- 2026-04-25: 初始文档
