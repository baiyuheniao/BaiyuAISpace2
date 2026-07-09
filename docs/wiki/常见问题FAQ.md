# 常见问题（FAQ）

## 安装与更新

### 安装时 Windows 提示「已保护你的电脑」？

安装包暂未做代码签名，属于独立开发者应用的常见情况。点击 **「更多信息」→「仍要运行」** 即可。所有安装包均由 GitHub Actions 从公开源码自动构建，可溯源。详见 [[安装指南]]。

### 怎么升级到新版本？

v0.2.0-beta.3 起应用会在启动时自动检测新版本，左下角弹窗提示，确认后自动下载安装并重启。更早版本请手动到 [Releases](https://github.com/baiyuheniao/BaiyuAISpace2/releases) 下载覆盖安装。

## 使用问题

### 发消息报错 / 没有回复？

按顺序排查：

1. **API Key 是否有效**：到服务商控制台确认密钥状态和余额
2. **模型名称是否正确**：各家模型更新频繁，确认设置里填的模型名在官方文档中存在
3. **网络能否访问该服务商**：部分海外服务商需要相应网络环境
4. 仍不行请**导出日志提 Issue**（见下文）

### 知识库检索很慢？

当前向量检索为全量扫描。单个知识库超过约 1 万个文本分块（约几百份文档）后会明显变慢。建议按主题拆分为多个知识库，对话时只挂接需要的那个。向量索引优化在路线图中（[Issue #18](https://github.com/baiyuheniao/BaiyuAISpace2/issues/18)）。

### MCP 预设安装后启动失败？

Stdio 类 MCP 服务器通常依赖 **Node.js（npx）** 或 **Python（uvx）** 运行环境。缺少依赖时应用会给出提示，按提示安装对应运行环境后重启服务器即可。

### 会议中途重启了 App，Agent 卡在 Meeting 状态？

已知问题：App 重启后 Agent 的 `meeting` 状态不会自动清理，需要手动操作恢复（修复已在计划中）。

### 定时任务没有触发？

定时任务由后端每 30 秒轮询一次，**只在 App 运行期间生效**。App 关闭时错过的任务不会补触发。

## 数据与隐私

### 我的数据存在哪里？会上传吗？

全部在本地，不上传任何服务器：

| 内容 | 位置（Windows） |
|---|---|
| 日志 | `%APPDATA%\BaiyuAISpace2\logs\app_<日期>.log` |
| 会话 / 知识库数据库 | `%APPDATA%\com.baiyu.aispace\` |
| API 密钥 | 系统凭据管理器（加密） |

对话内容只会发给你自己配置的 LLM 服务商。

### 日志在哪里？怎么导出？

- 路径：`%APPDATA%\BaiyuAISpace2\logs\`，按日期一天一个文件
- 也可以在 **Settings（设置）** 页一键导出
- 提 Bug 时**请务必附上日志**，能极大加快定位

## 其他

### 可以免费商用吗？

个人使用、公司内部使用、做插件/二次开发产品都免费；「换皮转卖」或直接架成 SaaS 卖账号需要商业授权。详见 [[许可证说明]]。

### 在哪里反馈问题 / 提建议？

- Bug / 功能建议：[Issue 模板](https://github.com/baiyuheniao/BaiyuAISpace2/issues/new/choose)
- 使用咨询 / 讨论：[Discussions](https://github.com/baiyuheniao/BaiyuAISpace2/discussions)
- 安全漏洞：按 [SECURITY.md](https://github.com/baiyuheniao/BaiyuAISpace2/blob/main/SECURITY.md) 私下报告
