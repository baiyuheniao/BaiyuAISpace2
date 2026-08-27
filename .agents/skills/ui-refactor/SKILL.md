---
name: ui-refactor
description: BaiyuAISpace2 前端布局/交互重构方法。当要把某个页面的摆放/交互改成符合用户想象的样子，或在推进 TODO.md「前端布局/交互重构」里下一个页面时用。处理的是"按钮放哪、用下拉还是二级页、某操作在哪个页面做"这类用户意图，不是视觉配色（那属 design-system），也不是启动/截图应用（那属 run-baiyuaispace2）。
---

## 背景 / 为什么这么做

前端长期由 Agent 编写，用户只交代了艺术设计规范（黑白单色，见 `design-system`
skill），但"哪个控件放哪、用下拉还是二级页、某操作在哪个页面做"这类布局与
交互流从没被钉死，Agent 各凭感觉填，结果和用户想象错位。**直接重写代码解决
不了这个**——问题不在代码，在规范没钉死，重写完还是会漂。

## 方法（每个页面循环一次）

1. **先出"现状清单"，不是让用户从空白设计**。读该页 `.vue` + 相关组件/stores，
   逐条列出每个控件"现在在哪、什么形态、怎么触发"，编上号，用户能直接引用
   （"第 X 号改成 …"）。
2. **用户标注，不是用户编写**。给用户看清单或截图，用户标"对 / 错 / 挪到哪"。
   标注最高效是直接在截图/运行中的界面里圈出来（用户通常会贴路径）。
3. **只改用户点名的**，别顺手重构没提到的部分——避免把已验证的界面一起改动。
4. **验证**：改完用 `run-baiyuaispace2` 启动应用截图自查排版，再
   `cargo test` + `pnpm build`（CI 同款命令）。**截图收进 `docs/test-evidence/`
   并提交**，别丢临时目录。
5. **沉淀约定**：改的过程中把反复出现的规则写回本 Skill（或一份"布局/交互约定"），
   避免下次再漂。

## 已确立的项目级约定（2026-08-27 Chat 页应用，后续页面照此）

### 设置驱动的个性化

新增可配置项统一走 settings store + SettingsView「通用设置」区。**改 store 时记得
加进 `persist.paths`，否则重启丢失。** 已加入的：

| 设置 | 默认 | 说明 |
|---|---|---|
| `chatContentWidth` | 900px | 消息内容宽度（输入框跟随同宽，内联 style 绑定） |
| `inputFocusLiftEnabled` | true | 输入框聚焦上浮（`.input-container.focus-lift:focus-within`） |
| `messageBorderEnabled` | true | 消息体细边框（`.message-body.no-border` 覆盖 `border-color`） |
| `aiAvatar` / `userAvatar` | "" | 头像；空=用 Logo，可填 http(s) URL 或 data URL |
| `aiName` / `userName` | BAI / 用户 | AI 与用户显示名，别在组件里写死 |

头像本地图片：设置页选文件 → 后端 `read_image_as_data_url(path)` 读成 data URL
（限 1MB、按扩展名判 mime），存进设置；**不要**开 asset 协议或 fs 权限来直接
渲染本地路径。

### 会话上下文收拢（避免双入口）

KB/MCP/Skill 这类"既有状态又有操作"的功能，**不要两处入口**（上面一个状态标签 +
下面一个操作图标）。统一收进右侧 `ChatSidePanel.vue`，四个同级区块：文件管理、
知识库、MCP 工具、Skill。输入区只保留：API 配置选择、上传图片/视频、附加文档、
思考（若支持）、发送/停止、底部用量。工作目录状态（`v-if="workingDir"`）也在
文件管理区块里，别另起顶栏。

### 后端能力判断，前端别硬猜

- **思考按钮**：不按前端写死 provider 名单，走后端 `supports_thinking(provider)`
  命令（与请求构造同在 `llm.rs`，保证同步不漂）。前端在配置切换时查一次；
  不支持就隐藏按钮 + 关掉遗留的 `thinkingEnabled`。
- **图片本地展示**：`read_image_as_data_url` 读成 data URL，避免为头像扩充 asset
  协议/fs 权限这类安全面。

### 度量与文案

- 消息/错误统一左下角弹窗；中文 placeholder；提示说人话，不暴露内部术语
  （这些是 `design-system` skill 的既有约定，改 UI 别违反）。

## 反模式（不要做）

- **全量重写前端**：风险大、打断 Agent Team 主线。按"最常用 / 最不顺眼"排序，
  一页一页改。
- 给同一功能留两处入口（指示条标签 + 工具栏图标）。
- 硬编码颜色/字号/圆角/缓动——读 `variables.scss`，见 design-system skill。
- 发布文案 claim 未实现功能（例：`统计面板` 目前只是 TODO，不是已实现模块）。

## 相关

- `design-system` skill：视觉规范权威源（黑白单色、直角、缓动、排版 token）。
- `run-baiyuaispace2` skill：启动应用 / 截图 / 导航 / 点击的 CDP 驱动。
- `TODO.md`「前端布局/交互重构」节：进度跟踪（Chat 已完，其余页面待办）。
- `AGENTS.md` / `CLAUDE.md`：项目结构与约定入口。
