# 贡献指南

感谢你愿意为 BaiyuAISpace 出力！本文覆盖从搭环境到 PR 合并的完整流程，
以及几个**外部贡献者几乎必踩的坑**——建议先通读一遍再动手。

## 环境要求

| 工具 | 版本 | 说明 |
|---|---|---|
| Node.js | 22+ | |
| pnpm | 11+ | `npm i -g pnpm` |
| Rust | stable（1.75+） | 通过 [rustup](https://rustup.rs/) 安装 |
| 平台依赖 | 见下 | Tauri 2 的系统依赖 |

- **Windows**（主力平台）：需要 Microsoft Visual Studio C++ Build Tools 和 WebView2（Win11 自带）
- **Linux**：`libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev` 等，
  完整清单见 `.github/workflows/ci.yml`
- **macOS**：Xcode Command Line Tools

## 构建与开发

```bash
pnpm install        # 安装前端依赖
pnpm build          # 前端类型检查 + 构建（vue-tsc --noEmit && vite build）
pnpm tauri dev      # 开发模式（真窗口 + HMR，首次启动约 25 秒）
pnpm tauri build    # 生产构建
pnpm lint           # eslint --fix
pnpm format         # prettier
```

### ⚠️ 坑 1：构建顺序

后端编译期会读取 `dist/` 目录（`tauri.conf.json` 的 `frontendDist`）。
**克隆仓库后必须先跑一次 `pnpm build`，再碰任何 cargo 命令**，
否则 `cargo build` / `cargo check` 直接 panic，报错信息还不太直观。

```bash
pnpm install && pnpm build          # 先这两步
cd src-tauri && cargo build         # 然后才能单独编译后端
```

### ⚠️ 坑 2：没有常规测试套件

本项目目前没有单元测试/集成测试套件（唯一的自动化测试是
`src-tauri/src/workspace_smoke_test.rs` 冒烟测试）。**正确性门禁就是双构建**：

1. `pnpm build` —— 前端类型检查必须通过
2. `cargo build`（在 `src-tauri/` 下）—— 后端编译必须通过

提交 PR 前请确保两者都过；CI 会在 Windows 和 Ubuntu 上跑同样的检查。
涉及功能行为的改动，请在 PR 描述里说明手动验证的步骤和结果。

## 代码规范

### 通用

- 所有新建的 `.rs` / `.ts` / `.vue` 文件必须带 MPL-2.0 头注释（照抄现有文件开头即可）
- UI 文案、注释、报错提示一律用**中文**
- API 密钥等敏感信息只能走 `src-tauri/src/secure_storage.rs` 加密存储，
  禁止写入普通 SQLite 表或 localStorage

### ⚠️ 坑 3：UI 必须遵守黑白编辑设计系统

前端是严格的 monochrome editorial 风格：**无彩色、无圆角**、指定字体与缓动曲线。
样式 token 的唯一权威来源是 `src/styles/variables.scss`，禁止硬编码颜色/圆角/字体。
其他硬性约定：

- 所有提示、报错、警告统一走左下角弹窗机制，不要另起炉灶
- 表单输入框必须写中文 placeholder

### ⚠️ 坑 4：超时策略

**流式响应和大文件下载禁止设置总超时**，只允许读间隔超时（read/idle timeout）。
历史上因为总超时误伤长任务出过五处 Bug。改网络请求相关代码时务必遵守。

### 后端结构速览

- `src-tauri/src/commands/llm.rs` 是全部 15+ 家 LLM 服务商的对接层
  （服务商清单在 `PROVIDER_CONFIGS`）。改任何服务商行为前，
  先读 `docs/api-manuals/` 里对应的 API 手册
- 新增 Tauri command 需要在 `main.rs` 登记两处：`invoke_handler` 和（如有状态）`app.manage()`

## 提交与 PR

- 提交信息格式：`type: 中文摘要`，type 遵循
  [Conventional Commits](https://www.conventionalcommits.org/)
  （`feat:` / `fix:` / `docs:` / `chore:` / `refactor:` 等）
  - ✅ `fix: 修复知识库导入大 PDF 时进度条卡死`
  - ❌ `update code`
- 一个 PR 只做一件事；大改动建议先开 Issue 或 Discussion 对齐方向
- PR 请按模板填写自查清单
- 目标分支为 `main`，合并要求 CI 全绿

## 提问与讨论

- **Bug 报告 / 功能建议**：用 [Issue 模板](https://github.com/baiyuheniao/BaiyuAISpace2/issues/new/choose)
- **使用咨询 / 开放讨论**：[Discussions](https://github.com/baiyuheniao/BaiyuAISpace2/discussions)
- **安全漏洞**：请勿公开发 Issue，按 [SECURITY.md](./SECURITY.md) 私下报告

## 许可证与贡献者协议

本项目采用 MPL-2.0 并附有《BaiyuAISpace 许可证补充条款》。

向本项目提交贡献（PR、补丁等）即表示你同意以下条款：

1. **相同条款发布**：你的贡献以与本项目相同的条款（MPL-2.0 + 补充条款）发布；
2. **再许可授权**：你授予项目版权所有者（Baiyu）一份永久、全球范围、免费、
   不可撤销的授权，允许其将你的贡献作为项目整体的一部分以其他条款再许可
   （包括按补充条款第四条向第三方授予商业授权）；
3. **权利保证**：你确认拥有所提交内容的相应权利（原创，或你有权以上述条款提交）。

**说明**：这不转移你的版权——你始终是自己贡献部分的版权人，以上仅是使用授权。
之所以需要第 2 条，是因为项目对"套壳商用 / 托管服务"场景保留商业授权能力
（见补充条款第二、四条）；若无此授权，包含外部贡献的版本将无法合法地
向第三方发放商业授权。
