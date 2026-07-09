# 安全政策

BaiyuAISpace 在本地管理用户的 LLM API 密钥并渲染模型返回的富文本内容，
我们非常重视任何可能导致**密钥泄露、任意代码执行或沙箱逃逸**的问题。

## 受支持的版本

Beta 阶段只维护最新发布版本。旧版本的漏洞如在新版本中已修复，不再单独发补丁。

## 如何报告漏洞

**请不要通过公开 Issue 报告安全漏洞。**

请通过以下任一方式私下报告：

1. **GitHub 私密报告（推荐）**：仓库
   [Security → Report a vulnerability](https://github.com/baiyuheniao/BaiyuAISpace2/security/advisories/new)
2. **邮件**：baiyuheniao@gmail.com（标题请以 `[SECURITY]` 开头）

报告时请尽量包含：

- 漏洞类型和影响范围（例如：XSS 读取 localStorage、iframe 沙箱逃逸、密钥明文落盘）
- 复现步骤或 PoC
- 受影响的版本和平台

## 响应承诺

- **72 小时内**确认收到并给出初步评估
- 确认为有效漏洞后，会尽快修复并在下一个版本发布；修复发布前请勿公开细节
- 欢迎在修复发布后公开你的发现，我们会在 Release Notes 中致谢（如你愿意）

## 重点关注的攻击面

- API 密钥存储与传输（`secure_storage.rs` 加密存储链路）
- 聊天消息渲染（Markdown / HTML 预览 iframe 沙箱 / Mermaid / KaTeX / DOMPurify）
- MCP 工具调用与外部进程交互
- 知识库文档解析（PDF / Office 文件解析器）
