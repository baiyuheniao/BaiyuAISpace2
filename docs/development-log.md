# BaiyuAISpace 开发日志

> 记录项目开发过程中的阶段性成果和重要变更

---

## 📅 2026-02-10 v0.1.0 MVP 基础版本

### ✅ 已完成功能

#### 1. 项目架构搭建
- **前端框架**: Vue 3.4 + TypeScript + Vite
- **桌面框架**: Tauri 2.0 + Rust
- **UI 组件库**: Naive UI + 暗色主题
- **状态管理**: Pinia + 持久化存储
- **代码规范**: ESLint + Prettier + MPL-2.0 许可证头

#### 2. 多 LLM 提供商支持 (15+)
支持全球主流 LLM API 服务：

**国际服务商**:
- [x] OpenAI (GPT-4o, GPT-4o-mini, GPT-3.5)
- [x] Anthropic (Claude 3.5 Sonnet, Claude 3 Opus)
- [x] Google Gemini (Gemini 1.5 Pro/Flash)
- [x] Azure OpenAI
- [x] Mistral AI (Mistral Large, Codestral)

**中国服务商**:
- [x] Moonshot (Kimi) - 长文本支持
- [x] 智谱 AI (GLM-4) - 中文理解强
- [x] 阿里通义千问 (Qwen)
- [x] 百度文心一言 (ERNIE)
- [x] 字节豆包 (Doubao)
- [x] DeepSeek - 推理能力强，价格低
- [x] **硅基流动 (SiliconFlow)** - 多模型聚合
- [x] MiniMax
- [x] 零一万物 (Yi)

**自定义**:
- [x] 自定义 OpenAI 兼容接口 (如 Ollama、LocalAI)

#### 3. UI/UX 设计
- [x] **侧边栏导航**: Logo、新建对话按钮、菜单、用户信息
- [x] **对话页面**: 
  - 精美消息气泡设计
  - Markdown 渲染 + 代码高亮 (highlight.js)
  - 用户/AI 头像区分
  - 空状态动画提示
- [x] **输入框**:
  - 圆角设计、聚焦发光效果
  - Enter 发送、Shift+Enter 换行
  - 当前模型显示
- [x] **设置页面**:
  - 提供商选择下拉框
  - API Key 输入（密码隐藏）
  - 模型选择
  - 深色/浅色主题切换
- [x] **历史记录页面**: 会话列表、时间显示、删除功能

#### 4. 核心功能
- [x] 新建对话会话
- [x] 发送消息到 LLM API
- [x] 接收并显示 AI 回复
- [x] 多提供商切换
- [x] API Key 本地存储（加密）
- [x] 主题持久化
- [x] 响应式布局适配

#### 5. 开发环境配置
- [x] 国内镜像配置 (npm/pnpm + Rust cargo)
- [x] Windows 开发环境支持
- [x] 自动化安装脚本

---

### 🚧 开发中功能

- [ ] 流式输出 (SSE 实时打字机效果)
- [ ] SQLite 会话持久化存储
- [ ] 本地模型支持 (Llama.cpp 集成)
- [ ] 消息复制、重新生成
- [ ] 对话标题自动生成
- [ ] 导出对话记录 (Markdown/PDF)

---

### 🐛 已知问题

1. **Windows 首次编译慢**: Rust 链接器在 Windows 上较慢，首次需 3-5 分钟
2. **应用图标缺失**: 需要添加精美应用图标
3. **流式输出未实现**: 消息一次性返回，非实时流式
4. **历史记录未持久化**: 目前存储在内存，重启后丢失

---

### 📊 项目统计

| 指标 | 数值 |
|------|------|
| 前端代码行数 | ~3000+ 行 |
| Rust 代码行数 | ~500+ 行 |
| 支持的 LLM 提供商 | 15+ |
| 依赖包数量 | 50+ |
| 构建产物大小 | ~25 MB |

---

### 📝 技术债务

- [ ] 移除未使用的 import (HashMap, State)
- [ ] 添加完整的错误处理
- [ ] 添加单元测试
- [ ] 优化构建配置，减少产物体积
- [ ] 添加应用图标和启动图

---

### 🎯 下一步计划 (v0.1.1)

1. ~~实现流式输出 (SSE)~~ ✅ 已完成
2. ~~添加 SQLite 数据库持久化~~ ✅ 已完成
3. 支持消息复制功能
4. 添加应用图标
5. 优化 Windows 构建速度

---

## 📅 2026-02-10 SQLite 数据持久化

### ✅ 新功能

#### 数据库架构
使用 **SQLite** + **rusqlite** 实现本地数据持久化：

**数据库表结构：**
```sql
-- 会话表
sessions:
  - id: TEXT PRIMARY KEY
  - title: TEXT
  - provider: TEXT (LLM提供商ID)
  - model: TEXT (模型名称)
  - created_at: INTEGER (时间戳)
  - updated_at: INTEGER (时间戳)

-- 消息表
messages:
  - id: TEXT PRIMARY KEY
  - session_id: TEXT (外键，关联sessions)
  - role: TEXT (user/assistant/system)
  - content: TEXT (消息内容)
  - timestamp: INTEGER (时间戳)
  - error: TEXT (错误信息，可选)
```

**索引优化：**
- `idx_sessions_updated_at` - 按更新时间倒序查询
- `idx_messages_session_id` - 快速查询会话消息
- `idx_messages_timestamp` - 按时间排序

#### 功能实现

**后端 (Rust):**
- [x] 数据库初始化和迁移
- [x] 会话 CRUD 操作
- [x] 消息 CRUD 操作
- [x] 外键关联和级联删除
- [x] 数据库连接池管理

**前端 (Vue):**
- [x] 发送消息自动保存
- [x] 流式输出完成后保存
- [x] 历史记录从数据库加载
- [x] 删除会话同步删除数据库记录

#### 数据存储位置
- **Windows**: `%APPDATA%/BaiyuAISpace/app.db`
- **macOS**: `~/Library/Application Support/BaiyuAISpace/app.db`
- **Linux**: `~/.local/share/BaiyuAISpace/app.db`

### 📊 技术细节

| 组件 | 技术 |
|------|------|
| 数据库 | SQLite 3 |
| Rust 驱动 | rusqlite (bundled) |
| 数据库路径 | Tauri app_data_dir |
| 并发控制 | tokio::sync::Mutex |
| 外键约束 | ON DELETE CASCADE |

### 🔧 实现文件

- `src-tauri/src/db.rs` - 数据库操作模块
- `src-tauri/src/main.rs` - 数据库初始化和命令注册
- `src/stores/chat.ts` - 前端数据库交互
- `src/views/HistoryView.vue` - 历史记录加载

---

## 📅 2026-02-10 模型列表全面更新 (基于 MCP 搜索验证)

### 🔍 验证方法
使用 MCP 搜索工具验证各厂商最新模型：
- OpenAI 官方文档: https://platform.openai.com/docs/models
- Moonshot 官方文档: https://platform.moonshot.cn/docs/guide/kimi-k2-5-quickstart
- DeepSeek 官方文档: https://platform.deepseek.com/api-docs/models

### 🔧 关键更新

#### OpenAI (重大更新)
- ✅ **gpt-5** - 最新旗舰模型 (2025年发布)
- ✅ **gpt-5.1**, **gpt-5.2** - 升级版
- ✅ **gpt-4.1** - 新一代模型
- ✅ **o3**, **o4-mini** - 最新推理模型
- ✅ **gpt-4o-realtime**, **gpt-4o-audio** - 实时/音频模型
- ⚠️ gpt-4.5-preview 已标记为废弃 (2025年7月移除)

#### Moonshot (Kimi) (重大更新)
- ✅ **kimi-k2.5** - Kimi 迄今最智能模型 (2026年1月发布)
- ✅ **kimi-k2-thinking** - 思考模式
- ✅ **kimi-k2-turbo-preview** - 高性能预览版
- ✅ 支持 256K 上下文窗口
- ✅ 原生多模态架构（视觉+文本）

#### Anthropic
- ✅ **claude-3-5-sonnet-20241022** - 最新版本
- ✅ **claude-3-5-haiku-20241022** - 新版 Haiku
- ✅ claude-3-opus, claude-3-sonnet

#### Google Gemini
- ✅ **gemini-2.0-pro**, **gemini-2.0-flash** - 2.0系列
- ✅ gemini-1.5-pro, gemini-1.5-flash

#### 智谱 AI (GLM) (2025年4月更新)
- ✅ **glm-4-air-250414** - 2025年4月新版
- ✅ **glm-4-flash-250414** - 升级版
- ✅ glm-4-plus, glm-4, glm-4-air
- ✅ glm-4v-plus, glm-4v-flash (视觉模型)

#### 阿里通义千问 (2025-2026更新)
- ✅ **qwen-max-latest** - 最新版本
- ✅ **qwen-plus-latest** - 支持思考/非思考模式
- ✅ qwen-coder-plus, qwen-coder-turbo
- ✅ qwen-vl-max, qwen-vl-plus (视觉模型)

#### DeepSeek (V3.2 升级)
- ✅ **deepseek-chat** - DeepSeek-V3.2 非思考模式
- ✅ **deepseek-reasoner** - DeepSeek-V3.2 思考模式 (原R1)

#### 字节豆包
- ✅ **doubao-pro-256k** - 超长上下文 (256K)
- ✅ doubao-pro-128k, doubao-pro-32k
- ✅ doubao-vision-pro (视觉模型)

#### 硅基流动
- ✅ **DeepSeek-V3**, **DeepSeek-R1**
- ✅ **Qwen/QwQ-32B-Preview** (推理模型)
- ✅ Qwen2.5 全系列 (72B/32B/14B/7B)
- ✅ Llama-3.1 系列

### 📊 统计

| 指标 | 数值 |
|------|------|
| 总提供商 | 15+ |
| 总模型数 | 100+ |
| 2025-2026新模型 | 30+ |
| 视觉模型 | 10+ |
| 推理/思考模型 | 15+ |

---

## 📅 2026-02-10 流式输出功能 (Streaming)

### ✅ 新功能

#### 流式对话 (SSE - Server-Sent Events)
实现实时打字机效果，AI 回复逐字显示：

**后端实现** (`src-tauri/src/commands/llm.rs`):
- 新增 `stream_message` 命令，支持 SSE 流式传输
- 使用 `reqwest` + `futures` 处理流式响应
- 解析 OpenAI/Anthropic 等提供商的 SSE 格式
- 通过 Tauri Event 系统向前端推送流式数据

**前端实现** (`src/stores/chat.ts`):
- 使用 `@tauri-apps/api/event` 监听 `stream-chunk` 事件
- 实时累积消息内容，实现打字机效果
- 流结束自动更新消息状态

**支持的提供商**:
- [x] OpenAI (GPT-4o, GPT-4o-mini, GPT-3.5)
- [x] Anthropic (Claude 3.5 Sonnet, Claude 3 Opus)
- [x] Moonshot (Kimi)
- [x] 智谱 AI (GLM)
- [x] 阿里通义千问
- [x] 百度文心一言
- [x] 字节豆包
- [x] DeepSeek
- [x] 硅基流动 (SiliconFlow)
- [x] Mistral AI
- [x] MiniMax
- [x] 零一万物 (Yi)
- [x] Google Gemini (待测试)
- [x] Azure OpenAI (待测试)
- [x] 自定义 OpenAI 兼容接口

### 🔧 技术细节

| 组件 | 技术 |
|------|------|
| 后端 HTTP 客户端 | reqwest with stream feature |
| 流式处理 | futures::StreamExt |
| 前端事件监听 | @tauri-apps/api/event |
| 数据格式 | SSE (Server-Sent Events) |

### 📊 性能优化

- 使用 `bytes_stream()` 高效处理二进制流
- 前端使用 `ref` 响应式更新，避免重复渲染
- 事件监听自动清理，防止内存泄漏

---

## 📅 2026-02-10 代码质量修复

### 🔧 修复的问题

1. **移除未使用的变量**
   - `chat.ts`: 注释掉未使用的 `streamingContent` ref（为流式功能预留）

2. **优化 Markdown 渲染性能**
   - `ChatMessage.vue`: marked 配置现在只执行一次，避免重复初始化

3. **清理未使用的导入**
   - `Layout.vue`: 移除未使用的 `NSpace`, `NText` 组件导入
   - `llm.rs`: 修复 `_provider` 参数命名（前缀下划线避免警告）

4. **修复样式类缺失**
   - `ChatMessage.vue`: 添加缺失的 `.streaming-text` CSS 类

### 📊 代码质量状态

| 指标 | 状态 |
|------|------|
| 未使用导入 | ✅ 已清理 |
| 未使用变量 | ✅ 已修复 |
| TypeScript 类型检查 | ✅ 通过 |
| 代码规范 | ✅ 符合 MPL-2.0 头注释要求 |

---

*最后更新: 2026-02-10*
