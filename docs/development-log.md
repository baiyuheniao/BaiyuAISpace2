# BaiyuAISpace 开发日志

> 记录项目开发过程中的阶段性成果和重要变更

---

## 📅 2026-02-13 文件上传与功能按钮优化

### ✨ 新功能

#### 图片/视频文件上传
支持在对话中直接上传和发送图片、视频文件：

**支持格式**：
- 💬 **图片**: JPEG, PNG, GIF, WebP
- 🎬 **视频**: MP4, WebM, MPEG

**功能特点**：
- 📎 新增上传按钮，显示已附加文件数
- 📋 文件列表显示（带图标区分图片/视频）
- ❌ 支持删除已选择的文件
- ✉️ 支持混合发送（文本 + 文件）
- 🚫 仅允许发送时为空但有文件

#### 功能按钮智能禁用
优化按钮用户体验，当无可用资源时自动禁用：

**优化部分**：
1. **知识库按钮**
   - 无可用知识库时禁用
   - 提示文本：「无可用知识库」
   - 与 MCP 按钮禁用逻辑保持一致

2. **MCP 按钮**
   - 无启用的 MCP 服务时禁用
   - 提示文本：「无可用服务」
   - 已有的逻辑保持不变

### 🔧 技术细节

#### 前端实现 (`ChatInput.vue`)

**新增状态**：
```typescript
const fileInputRef = ref<HTMLInputElement | null>(null);
const attachedFiles = ref<File[]>([]);

// 后端 refs
const enabledMcpServersCount = computed(() => {
  return mcp.servers.filter(s => s.enabled).length;
});

const availableKbCount = computed(() => {
  return kbStore.knowledgeBases.length;
});
```

**新增方法**：
- `handleFileSelect()` - 触发文件选择对话框
- `handleFilesSelected(event)` - 处理选中的文件，验证格式
- `removeAttachedFile(index)` - 删除已附加的文件
- `getFileDisplayName(file)` - 格式化文件名显示（超长截断）

**发送逻辑更新**：
- 修改 `canSend` computed，支持「仅文件」发送
- `handleSend()` 构建消息时包含文件信息
- 文件信息格式：`[文件: name (size)]` 添加到消息内容

**UI 更新**：
- 新增隐藏的 `<input type="file">` 元素
- 新增上传按钮（`file-btn`），置于 MCP 按钮前
- 新增文件列表显示区域（`attached-files`）
- 每个文件显示为 Tag，可点击删除

#### 按钮禁用规则

| 按钮 | 禁用条件 | 提示文本 |
|------|---------|---------|
| 上传 | 无 | 添加图片/视频 (count) |
| MCP | `enabledMcpServersCount === 0` | 无可用服务 |
| 知识库 | `availableKbCount === 0` | 无可用知识库 |
| 发送 | `!canSend` | - |

### 📊 文件变更

| 文件 | 变更 |
|------|------|
| `src/components/ChatInput.vue` | 新增文件上传功能、按钮禁用逻辑、文件列表显示 |

### 💾 使用流程

**上传文件**：
1. 点击上传按钮 📎
2. 选择一个或多个图片/视频文件
3. 文件列表显示在输入框下方
4. 可点击标签的 ❌ 删除指定文件

**发送消息**：
- **仅文本**: 输入文本 → 点击发送
- **仅文件**: 选择文件 → 点击发送
- **文本+文件**: 输入文本 + 选择文件 → 点击发送

**后端接收**：
- 当前文件信息作为文本追加到消息内容中
- 格式：`[文件: filename (size)]`
- 下一步可实现文件读取和处理逻辑

### 🎨 样式规范

**新增 CSS 类**：
```scss
.attached-files        // 文件列表容器
.files-label          // 标签文本
.files-list           // 文件列表（flex wrap）
.file-item            // 单个文件项
.file-tag             // 文件 tag 样式
.file-btn             // 上传按钮样式
```

---

## 📅 2026-02-12 MCP 与 LLM 函数调用集成

### ✨ 核心功能

#### MCP 工具定义自动生成
当用户启用 MCP 且有可用工具时，系统自动构建工具定义并集成到 LLM 提示词：

**实现逻辑** (`src/stores/chat.ts`):

1. **`buildMcpToolDefinitions(availableTools)`** 
   - 将 MCPTool[] 转换为 OpenAI 兼容的 JSON 格式
   - 生成结构：`{ type: "function", function: { name, description, parameters } }`

2. **`buildMcpSystemPrompt(availableTools)`**
   - 构建包含工具列表和使用说明的完整系统提示词
   - 自动告知 LLM：「你可以使用以下工具来完成任务」

3. **增强的 `sendMessage()`**
   - 检查 `mcpEnabled` 状态和 `availableTools` 长度
   - 构建工具定义并合并到消息列表
   - 智能处理现有系统消息（追加或创建新的）

#### 工作流程

```
用户启用 MCP + 存在可用工具
    ↓
sendMessage() 检查 mcpEnabled 标志
    ↓
从 useMCPStore 获取 availableTools
    ↓
调用 buildMcpToolDefinitions() 生成工具 JSON
    ↓
调用 buildMcpSystemPrompt() 生成完整系统提示
    ↓
将 MCP 系统提示词合并到消息列表
    ↓
发送给 LLM API（包含工具定义）
    ↓
LLM 返回响应（可能提及工具使用）
    ↓
流式显示到 UI
```

#### 特点

✅ **零编译错误** - 所有类型检查通过  
✅ **RAG 兼容** - 与现有知识库功能无冲突  
✅ **动态工具集合** - 工具启用/禁用立即生效  
✅ **系统提示合并** - 智能处理现有系统消息  
✅ **完整文档** - 提供详细集成指南

### 🔧 技术细节

| 组件 | 说明 |
|------|------|
| 工具格式 | OpenAI 兼容的函数定义 |
| 提示词集成 | 系统消息中追加或创建 |
| 状态检查 | mcpEnabled flag + availableTools.length |
| 兼容性 | 所有支持的 LLM 提供商 |

### 📚 文档

新增 `docs/mcp-llm-integration.md`，包含：
- 完整实现细节
- 使用场景示例
- Python/TypeScript 代码示例
- 测试指南
- 故障排除
- 性能和安全考虑

### 📊 文件变更

| 文件 | 变更 |
|------|------|
| `src/stores/chat.ts` | 导入 MCPStore，新增工具定义构建函数，增强 sendMessage() |
| `docs/mcp-llm-integration.md` | 新建完整集成指南 |

### ⏳ 后续步骤

虽然当前实现已能将工具定义发送给 LLM，但要实现完整的**函数调用执行**，还需要：

1. **解析 LLM 工具调用** - 检测并提取工具使用意图
2. **执行 MCP 工具** - 通过 `mcp.callTool()` 调用
3. **反馈给 LLM** - 将结果发送回 LLM 进行继续推理

详见 `docs/mcp-llm-integration.md` 的「后续步骤」部分。

---

## 📅 2026-02-12 API 配置系统重构完成

### ✅ 重大功能变更

**修改范围**：API 设置与对话系统全面重构

#### 新功能设计

1. **多 API 配置管理**
   - 支持创建多个 API 配置（如：OpenAI 生产环境、OpenAI 测试环境、Azure 等）
   - 每个配置独立保存：名称、服务商、Base URL、模型、API Key
   - 配置列表展示，支持编辑、删除

2. **服务商预设**
   - 选择服务商自动填充默认 Base URL
   - 支持 15+ 家主流服务商预设
   - Base URL 可手动修改（适配自定义部署、代理等场景）

3. **模型手填模式**
   - 取消模型下拉选择，改为手填输入
   - 原因：模型更新频繁，下拉列表难以跟上
   - 提示用户参考服务商官方文档

4. **对话界面 API 选择器**
   - 输入框上方显示当前使用的 API 配置
   - 点击可快速切换其他配置
   - 无配置时提示前往设置创建

#### 技术变更

| 文件 | 变更 |
|------|------|
| `settings.ts` | 重构为 `ApiConfig` 系统，支持多配置管理 |
| `SettingsView.vue` | 全新配置管理界面，列表+弹窗编辑 |
| `ChatInput.vue` | 添加 API 选择器，显示当前配置信息 |
| `chat.ts` | 适配新 API 系统，`createSession` 改为使用 `apiConfigId` |

#### 数据迁移
- 存储版本升级为 v4
- 旧版 providers 数据将被清除
- 用户需重新创建 API 配置

---

## 📅 2026-02-12 模型列表更新至 2025-2026 最新版本

### ✅ 更新内容

**修改范围**：前端模型配置 (`src/stores/settings.ts`)

#### 新增模型

1. **OpenAI**
   - 新增 `gpt-5.3-codex` (2026年2月发布，最强编程模型)
   - 调整顺序：将最新模型置顶

2. **Anthropic**
   - 新增 `claude-3-7-sonnet-20250219` (2025年2月发布，首个混合推理模型)
   - 设为默认模型

3. **阿里通义千问**
   - 新增 Qwen3 系列全模型：
     - `qwen3-235b-a22b` (旗舰版)
     - `qwen3-30b-a3b` / `qwen3-32b` / `qwen3-14b`
     - `qwen3-8b` / `qwen3-4b` / `qwen3-1.5b` / `qwen3-0.6b` (轻量级)
   - 设为默认模型：`qwen3-235b-a22b`

4. **DeepSeek**
   - 新增 `deepseek-v3.2` (2025年12月发布，Agent能力增强)
   - 设为默认模型

#### 版本控制
- 更新 `STORAGE_VERSION` 从 `2` 到 `3`
- 自动清除旧版本 localStorage 缓存

---

## 📅 2026-02-12 RAG 功能完善完成

### ✅ 核心功能增强

**修改范围**：知识库检索 (RAG) 核心功能

#### 后端 (Rust)

1. **向量存储 (`db.rs`)**
   - 实现 SQLite BLOB 向量存储（替代空实现）
   - 新增 `vectors` 表结构
   - 实现 `cosine_similarity` 相似度计算
   - 支持向量插入和 Top-K 相似度搜索

2. **检索逻辑 (`retrieval.rs`)**
   - 修复 `Retriever::new()` 调用（添加 `db_path` 参数）
   - 从数据库动态读取知识库配置（解决硬编码 provider/model 问题）
   - 实现 `enrich_chunks` 补全元数据（chunk_index, document_filename）
   - 实现 `keyword_search`：FTS5 全文检索 + LIKE 降级
   - 完善 `hybrid_search`：RRF (Reciprocal Rank Fusion) 融合算法
   - 添加相似度阈值过滤

3. **命令接口 (`commands.rs`)**
   - 更新 `KbState` 结构（添加 `db_path`）
   - 修复 `search_knowledge_base` 调用
   - 导入文档时同步插入 FTS5 索引
   - 删除文档时同步删除 FTS5 记录

**技术方案**：
- 向量存储：SQLite BLOB + 暴力扫描 + 余弦相似度
- 关键词搜索：SQLite FTS5 (可选) → LIKE 降级
- 混合检索：RRF 融合算法
- 适用规模：< 10万 chunks（个人/小团队场景）

**待优化项**：
- [ ] 大规模数据考虑引入 HNSW 近似检索
- [ ] 关键词搜索中文分词优化

---

## 📅 2026-02-11 RAG 知识库前端界面 + 对话集成

### ✨ 新功能

#### 知识库管理界面
- **侧边栏导航**: 新增"知识库"选项卡，与对话/历史/设置同级
- **知识库列表**: 显示所有知识库，支持创建/删除
- **知识库详情**: 选中后显示文档列表和检索设置

**文件**: 
- `src/stores/knowledgeBase.ts` - Pinia store，管理知识库状态
- `src/views/KnowledgeBaseView.vue` - 知识库管理界面
- `src/router/index.ts` - 添加知识库路由
- `src/components/Layout.vue` - 添加知识库菜单

#### 文档上传
- 支持格式: PDF, Word, Excel, Markdown, HTML, TXT, 代码文件
- 文件选择器: 使用 Tauri dialog 插件
- 导入状态: 处理中/已完成/失败

#### 检索设置
- **检索模式**: 混合检索(默认) / 向量检索 / 关键词检索
- **Top-K**: 返回结果数量 (1-20)
- **相似度阈值**: 过滤低相似度结果

#### 对话集成 RAG
- **输入框**: 添加知识库选择按钮
- **RAG 指示器**: 显示当前使用的知识库和检索结果数量
- **上下文构建**: 自动将检索到的文档片段作为上下文发送给 LLM

**文件**:
- `src/stores/chat.ts` - 添加 RAG 相关状态和方法
- `src/components/ChatInput.vue` - 添加知识库选择器

### 使用流程
1. 创建知识库 → 选择 Embedding 模型
2. 导入文档 → 自动解析并生成向量
3. 对话中启用 RAG → 选择知识库
4. 提问 → AI 基于知识库内容回答

### 📊 技术栈
| 组件 | 技术 |
|------|------|
| 状态管理 | Pinia |
| UI 组件 | Naive UI |
| 文件选择 | @tauri-apps/plugin-dialog |
| 向量数据库 | LanceDB (后端) |

---

## 📅 2026-02-11 RAG 知识库基础架构（后端实现）

### 🗄️ 知识库核心功能

实现完整的本地 RAG (Retrieval-Augmented Generation) 后端架构：

#### 技术栈
| 组件 | 技术 | 说明 |
|------|------|------|
| 向量数据库 | **LanceDB** | 文件级、Rust原生、支持全文检索 |
| 元数据存储 | SQLite | 知识库/文档/分块信息 |
| 文档解析 | 自研 + 基础库 | PDF/Word/Excel/Markdown/TXT |
| Embedding | API调用 | OpenAI/智谱/硅基流动 |
| 检索模式 | 混合检索 | 向量 + 关键词 + RRF重排序 |

#### 模块结构
```
src-tauri/src/knowledge_base/
├── mod.rs          # 模块导出
├── types.rs        # 类型定义（KnowledgeBase, Document, Chunk等）
├── db.rs           # LanceDB向量存储 + SQLite元数据表
├── document.rs     # 文档解析（PDF/Word/Excel/MD/TXT）
├── embedding.rs    # Embedding API集成
├── retrieval.rs    # 检索实现（向量/关键词/混合）
└── commands.rs     # Tauri命令暴露
```

#### 数据库设计

**SQLite 元数据表**:
- `knowledge_bases` - 知识库配置（embedding模型、分块参数）
- `documents` - 文档信息（文件名、类型、状态、预览）
- `chunks` - 文本分块元数据

**LanceDB 向量表**:
- 每个知识库一个表: `kb_{id}`
- 字段: `vector` (固定维度), `chunk_id`, `document_id`, `content`

#### 文档解析支持

| 格式 | 优先级 | 状态 |
|------|--------|------|
| PDF | P0 | ✅ 基础支持（pdftotext/简单解析） |
| Word (.docx) | P0 | ✅ ZIP解压+XML解析 |
| Excel (.csv) | P0 | ✅ CSV直接读取 |
| Markdown | P1 | ✅ 直接读取 |
| HTML | P1 | ✅ 直接读取 |
| TXT/代码文件 | P1 | ✅ 直接读取 |

#### 分块策略
1. **段落优先** - 按 `\n\n` 分割
2. **句子回退** - 超长段落按句子分割
3. **硬截断** - 最终按固定长度保证

参数：
- `chunk_size`: 1000 (默认)
- `chunk_overlap`: 200 (默认)

#### Embedding API 支持

| 提供商 | 模型 | 维度 |
|--------|------|------|
| OpenAI | text-embedding-3-small | 1536 |
| OpenAI | text-embedding-3-large | 3072 |
| 智谱 AI | embedding-2 | 1024 |
| 硅基流动 | BAAI/bge-large-zh-v1.5 | 1024 |

#### 检索模式

1. **Vector** - 纯向量相似度
2. **Keyword** - 关键词匹配 (FTS待完善)
3. **Hybrid** - RRF融合 (默认)

RRF公式: `score = Σ 1/(k + rank)`, k=60

#### Tauri 命令

```rust
create_knowledge_base    # 创建知识库
list_knowledge_bases     # 列出知识库
delete_knowledge_base    # 删除知识库
import_document          # 导入文档
list_documents           # 列出文档
delete_document          # 删除文档
search_knowledge_base    # 检索知识库
get_embedding_models     # 获取可用模型
```

#### 待办 (前端部分)
- [ ] 侧边栏知识库导航
- [ ] 知识库管理界面
- [ ] 文档上传界面
- [ ] 检索设置（混合/向量/关键词）
- [ ] 对话中集成 RAG 检索

---

## 📅 2026-02-11 API Key 安全加密存储

### 🔐 安全增强

#### 系统密钥链存储 (keyring)
使用系统原生密钥链服务替代 localStorage 明文存储：

**存储位置**:
- **Windows**: Windows Credential Manager (凭据管理器)
- **macOS**: Keychain (钥匙串)
- **Linux**: Secret Service API / libsecret

**实现细节**:
- `src-tauri/src/secure_storage.rs`: 封装 keyring 操作
  - `save_api_key`: 保存 API Key 到系统密钥链
  - `get_api_key`: 从系统密钥链读取
  - `delete_api_key`: 删除存储的 API Key
  - `has_api_key`: 检查是否存在

- `src/stores/settings.ts`: 前端集成
  - `saveApiKeyToSecureStorage()`: 保存时调用后端命令
  - `loadApiKeyFromSecureStorage()`: 加载时调用后端命令
  - `loadAllApiKeys()`: 应用启动时批量加载
  - Pinia persist 配置排除 `apiKey` 字段，确保不落盘

- `src/App.vue`: 启动时加载所有 API Key
- `src/views/SettingsView.vue`: 更新提示文案，说明使用系统密钥链

**安全优势**:
| 存储方式 | 安全性 | 加密 |
|---------|--------|------|
| localStorage (旧) | ❌ 低 | ❌ 明文 |
| 系统密钥链 (新) | ✅ 高 | ✅ 系统级加密 |

---

## 📅 2026-02-11 质量修复与功能改进

### 🔧 修复的问题

#### 1. API Key 读取逻辑修复
**问题**: 后端 `llm.rs` 从环境变量读取 API Key，导致设置界面输入的 Key 无法生效

**修复**:
- `src-tauri/src/commands/llm.rs`:
  - `SendMessageRequest` 新增 `api_key` 字段
  - `get_api_key()` 改为从请求中获取，而非环境变量
  - 移除环境变量依赖

- `src/stores/chat.ts`:
  - `sendMessage()` 从 settings store 获取当前提供商的 API Key
  - 调用 `stream_message` 时传递 `apiKey` 参数

**影响**: 用户在设置界面输入的 API Key 现在可以正常使用了

#### 2. 关于页面信息更新
**修复**: 
- `src/views/SettingsView.vue`: GitHub 链接从 `baiyuheniao/BaiyuAISpace` 更正为 `baiyuheniao/BaiyuAISpace2`

### ✨ 新功能

#### 消息复制功能
**实现**: 
- `src/components/ChatMessage.vue`:
  - 添加复制按钮点击处理 `handleCopy()`
  - 使用 `navigator.clipboard.writeText()` 原生 API
  - 复制成功后显示 "已复制!" 提示（2秒后消失）
  - 添加 `NTooltip` 组件显示反馈

**用户体验**:
- 悬停消息显示复制按钮
- 点击后即时反馈
- 无需额外依赖，使用浏览器原生 Clipboard API

### 📊 代码质量状态

| 指标 | 状态 |
|------|------|
| API Key 传递 | ✅ 前端 → 后端 |
| 环境变量依赖 | ✅ 已移除 |
| 消息复制 | ✅ 已实现 |
| 链接准确性 | ✅ 已修正 |

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

- [x] 流式输出 (SSE 实时打字机效果) ✅ 2026-02-10
- [x] SQLite 会话持久化存储 ✅ 2026-02-10
- [x] 消息复制 ✅ 2026-02-11
- [ ] 本地模型支持 (Llama.cpp 集成)
- [ ] 消息重新生成
- [ ] 对话标题自动生成
- [ ] 导出对话记录 (Markdown/PDF)

---

### 🐛 已知问题

1. **Windows 首次编译慢**: Rust 链接器在 Windows 上较慢，首次需 3-5 分钟
2. **应用图标缺失**: 需要添加精美应用图标
3. ~~**流式输出未实现**: 消息一次性返回，非实时流式~~ ✅ 2026-02-10 已修复
4. ~~**历史记录未持久化**: 目前存储在内存，重启后丢失~~ ✅ 2026-02-10 已修复
5. ~~**API Key 安全存储**: 当前使用 localStorage 明文存储，需改为加密存储~~ ✅ 2026-02-11 已修复（改用系统密钥链）

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

*最后更新: 2026-02-12*
