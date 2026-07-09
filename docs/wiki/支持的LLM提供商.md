# 支持的 LLM 提供商

所有服务商通过统一接口接入，在 **Settings（设置）** 页新建 LLM API 配置时选择即可。

| 提供商 | 国家/地区 | 代表模型 | 特点 |
| --- | --- | --- | --- |
| **OpenAI** | 🇺🇸 美国 | gpt-4o, gpt-5 | 有条件而且不知道用啥就这个 |
| **Anthropic** | 🇺🇸 美国 | claude-sonnet-4-6, claude-opus-4.8 | 长文本、代码能力强 |
| **Google** | 🇺🇸 美国 | gemini-3.1-pro | 多模态、上下文长 |
| **Azure OpenAI** | 🇺🇸 美国 | gpt-4o, gpt-4 | 企业级、合规性好 |
| **Mistral AI** | 🇫🇷 法国 | mistral-medium-3.5 | 欧洲开源先锋 |
| **Moonshot (Kimi)** | 🇨🇳 中国 | kimi-k2.6 | 长文本、强中文场景、Agent 能力强 |
| **智谱 AI (GLM)** | 🇨🇳 中国 | glm-5.1 | 中文通用能力强，尤其代码 |
| **阿里（通义）** | 🇨🇳 中国 | qwen3.6-max 等 | 同参数性能好、低成本、多模态不错 |
| **百度（文心）** | 🇨🇳 中国 | ernie-4.0, ernie-4.0-turbo | 中文生态完善 |
| **字节（豆包）** | 🇨🇳 中国 | doubao-pro-256k, doubao-pro-32k | 性价比高、上手简单 |
| **DeepSeek** | 🇨🇳 中国 | deepseek-V4, deepseek-r1 | 价格便宜，编程和推理能力强 |
| **硅基流动 (SiliconFlow)** | 🇨🇳 中国 | Qwen2.5, DeepSeek-V3/R1 | 多模型聚合、价格优惠 |
| **MiniMax** | 🇨🇳 中国 | abab6.5s | 多模态、语音合成、Agent 能力不错 |
| **零一万物 (Yi)** | 🇨🇳 中国 | yi-large, yi-medium | 开源 + 商用 |
| **本地（Ollama）** | 🌐 本地 | Llama 3、Qwen3、Gemma 等 | 完全离线，数据不出设备 |
| **本地（LM Studio）** | 🌐 本地 | 任意 GGUF 模型 | GUI 友好，OpenAI 兼容接口 |
| **自定义** | 🌐 全球 | 任意 OpenAI 兼容接口 | 灵活配置 Base URL |

> 💡 各服务商模型更新频繁，表中模型仅为示例，完整列表请查看各家官方文档。设置中可直接**输入模型名称**添加新模型，不受预置列表限制。

## 能力差异速查

| 能力 | 支持范围 |
|---|---|
| 图片输入 | 多数支持视觉的服务商（JPEG/PNG/GIF/WebP） |
| 视频输入 | 仅 Gemini（MP4/WebM/MPEG） |
| 思考模式 | Anthropic（自适应/预算）、Gemini（thinkingBudget）、SiliconFlow Qwen3 等 |
| Prompt Caching | Anthropic（自动打 cache_control 断点） |
| 函数调用（MCP 工具） | 支持 Function Calling 的服务商 |

## 密钥安全

API Key 通过系统密钥链加密存储（Windows Credential Manager / macOS Keychain / Linux Secret Service），不写入普通数据库表，不上传任何服务器。
