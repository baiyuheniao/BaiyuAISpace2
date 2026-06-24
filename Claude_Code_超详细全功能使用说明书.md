
# Claude Code 超详细全功能使用说明书（终端版）

> **适用版本**: Claude Code v2.1.x（截至 2026年6月）  
> **核心定位**: 终端原生 Agentic AI 编程助手 —— 直接在你的代码库中读取、编辑、运行代码，无需离开命令行。

---

## 目录

1. [安装与启动](#1-安装与启动)
2. [核心工作流（生命周期）](#2-核心工作流生命周期)
3. [Slash 命令全参考（90+ 条）](#3-slash-命令全参考)
4. [键盘快捷键](#4-键盘快捷键)
5. [CLI 启动参数与标志](#5-cli-启动参数与标志)
6. [权限与安全系统](#6-权限与安全系统)
7. [模型与性能控制](#7-模型与性能控制)
8. [并行与后台 Agent](#8-并行与后台-agent)
9. [代码审查与发布](#9-代码审查与发布)
10. [MCP 服务器与扩展](#10-mcp-服务器与扩展)
11. [自定义技能与 Hooks](#11-自定义技能与-hooks)
12. [项目配置（CLAUDE.md）](#12-项目配置claudemd)
13. [实用工作流模板](#13-实用工作流模板)
14. [故障排查](#14-故障排查)

---

## 1. 安装与启动

### 安装方式

```bash
# 方式1：npm 全局安装（推荐）
npm install -g @anthropic-ai/claude-code

# 方式2：PowerShell（Windows）
irm https://claude.ai/install.ps1 | iex

# 方式3：curl（macOS/Linux）
curl -fsSL https://claude.ai/install.sh | sh
```

### 更新

```bash
npm update -g @anthropic-ai/claude-code
# 或
claude update
```

### 启动模式

| 命令 | 模式 | 用途 |
|------|------|------|
| `claude` | 交互式 REPL | 日常开发主模式 |
| `claude "prompt"` | 一次性交互 | 快速提问后进入对话 |
| `claude -p "prompt"` | Print（管道）模式 | 脚本/CI/CD，输出后退出 |
| `claude -c` | 继续上次对话 | 恢复最近会话 |
| `claude -r [session-name]` | 恢复指定会话 | 按名称或 ID 恢复 |
| `claude --bg "task"` | 后台 Agent | 后台运行，释放终端 |

---

## 2. 核心工作流（生命周期）

Claude Code 的命令按**会话生命周期**组织，而非字母顺序。以下是标准工作流：

```
初始化 → 工作 → 并行 → 审查 → 会话管理

/init → /plan → /model → /compact → /agents → /diff
/memory    /goal    /effort   /btw       /batch   /code-review
/mcp       /loop    /fast     /context   /bg      /security-review
/permissions

会话间: /clear /resume /rewind /branch
```

### 首次进入仓库的标准初始化

```bash
# 1. 生成项目记忆文件
/init

# 2. 编辑/查看记忆
/memory

# 3. 连接外部工具（GitHub、Slack、数据库等）
/mcp

# 4. 配置子 Agent
/agents

# 5. 设置权限边界
/permissions
```

> 设置 `CLAUDE_CODE_NEW_INIT=1` 环境变量可启用交互式 `/init`，会引导你配置 skills、hooks 和个人记忆。

---

## 3. Slash 命令全参考

> **规则**: 所有 `/` 命令必须出现在消息**开头**。输入 `/` 后按 Tab 或方向键可自动补全。

### 3.1 项目设置与目录

| 命令 | 功能 | 示例 |
|------|------|------|
| `/init` | 扫描项目并生成 `CLAUDE.md` | `/init` |
| `/memory` | 编辑 `CLAUDE.md`，管理自动记忆 | `/memory` |
| `/add-dir <path>` | 添加额外工作目录（不移动会话） | `/add-dir ../shared-lib` |
| `/cd <path>` | 移动会话到新目录（保留 prompt cache） | `/cd ../other-project` |
| `/hooks` | 查看/配置生命周期 hooks | `/hooks` |
| `/permissions` | 管理工具 allow/ask/deny 规则 | `/permissions` |
| `/fewer-permission-prompts` | 扫描历史并自动生成 allowlist | `/fewer-permission-prompts` |
| `/sandbox` | 切换沙盒模式 | `/sandbox` |
| `/install-github-app` | 为仓库安装 Claude GitHub App | `/install-github-app` |
| `/install-slack-app` | 安装 Claude Slack App | `/install-slack-app` |
| `/web-setup` | 通过本地 `gh` CLI 连接 GitHub | `/web-setup` |
| `/team-onboarding` | 根据 30 天使用历史生成团队入职文档 | `/team-onboarding` |
| `/run-skill-generator` | 为 `/run` 和 `/verify` 生成项目 skill | `/run-skill-generator` |

### 3.2 会话与上下文管理

| 命令 | 功能 | 示例 |
|------|------|------|
| `/clear [name]` | 清空上下文，保存旧会话供 `/resume` | `/clear feature-auth` |
| `/compact [instructions]` | 总结并压缩对话历史，释放上下文窗口 | `/compact keep the API design` |
| `/context [all]` | 可视化上下文使用情况（彩色网格） | `/context all` |
| `/btw <question>` | 旁白提问，不增加主对话历史 | `/btw 这个函数是线程安全的吗？` |
| `/copy [N]` | 复制最近（或第 N 条）回复到剪贴板 | `/copy` |
| `/export [filename]` | 导出对话为纯文本 | `/export session.md` |
| `/recap` | 生成当前会话的一句话摘要 | `/recap` |
| `/rename [name]` | 重命名当前会话 | `/rename auth-refactor` |
| `/resume [session]` | 恢复历史会话 | `/resume` 或 `/resume auth-refactor` |
| `/branch [name]` | 在对话当前点分叉（你探索分支） | `/branch experiment` |
| `/fork <directive>` | 后台分叉子 Agent（Claude 在后台工作） | `/fork 优化所有 SQL 查询` |
| `/rewind` | 回滚代码和/或对话到检查点 | `/rewind` |
| `/goal [condition or clear]` | 设置完成条件，Claude 持续工作直到达成 | `/goal all tests pass` |
| `/stop` | 停止当前后台会话（保留记录） | `/stop` |
| `/exit` | 退出 CLI | `/exit` |

### 3.3 模型与性能

| 命令 | 功能 | 示例 |
|------|------|------|
| `/model [model]` | 切换模型；`s` 仅当前会话 | `/model sonnet` |
| `/effort [level or auto]` | 设置推理深度 | `/effort xhigh` |
| `/advisor [model or off]` | 启用顾问工具（第二模型咨询） | `/advisor opus` |
| `/fast [on or off]` | 切换快速模式 | `/fast on` |
| `/usage` | 查看 token 用量、计划限制、活动统计 | `/usage` |
| `/usage-credits` | 配置用量额度（原 `/extra-usage`） | `/usage-credits` |
| `/heapdump` | 生成 JS 堆快照用于内存诊断 | `/heapdump` |

**Effort 级别说明**:
- `low` → `medium` → `high` → `xhigh` → `max` → `ultracode`
- `max` 和 `ultracode` 仅当前会话有效
- `ultracode` = `xhigh` 推理 + 自动工作流编排

### 3.4 并行 Agent 与编排

| 命令 | 功能 | 示例 |
|------|------|------|
| `/agents` | 管理子 Agent 配置 | `/agents` |
| `/tasks` | 查看后台任务（别名 `/bashes`） | `/tasks` |
| `/background [prompt]` | 分离为后台 Agent（别名 `/bg`） | `/bg 重构 utils 目录` |
| `/batch <instruction>` | 分解大型变更，生成并行 worktree 子 Agent | `/batch 迁移到 Vitest` |
| `/loop [interval] [prompt]` | 定时重复执行（别名 `/proactive`） | `/loop 5m check CI status` |
| `/schedule [description]` | 创建/管理云端定时任务（别名 `/routines`） | `/schedule` |
| `/workflows` | 查看、暂停、恢复动态工作流 | `/workflows` |
| `/deep-research <question>` | 并行搜索、验证、生成引用报告 | `/deep-research React 状态管理趋势` |
| `/ultraplan <prompt>` | 浏览器审阅计划 + 远程执行 | `/ultraplan 重构认证模块` |
| `/autofix-pr [prompt]` | 云端会话监控 PR，自动修复 CI | `/autofix-pr` |
| `/remote-env` | 设置云端 Agent 默认环境 | `/remote-env` |

### 3.5 代码审查与发布

| 命令 | 功能 | 示例 |
|------|------|------|
| `/diff` | 交互式 diff 查看器 | `/diff` |
| `/code-review [effort] [--fix] [--comment] [target]` | 代码审查 | `/code-review high --fix` |
| `/simplify [target]` | 仅清理审查（不找 bug） | `/simplify src/` |
| `/security-review` | 安全只读 diff 分析 | `/security-review` |
| `/review [PR]` | 本地 PR 审查 | `/review 123` |
| `/ultrareview [PR]` | 云端多 Agent 深度审查（`/code-review ultra`） | `/ultrareview` |

**审查级别**:
- `low` / `medium` / `high` / `max` / `ultra`
- `--fix`: 审查后自动应用优化建议
- `--comment`: 将发现发布为 GitHub PR 内联评论

### 3.6 记忆、技能与插件

| 命令 | 功能 | 示例 |
|------|------|------|
| `/skills` | 列出技能；`t` 按 token 排序 | `/skills` |
| `/reload-skills` | 不重启会话重新扫描 skill 目录 | `/reload-skills` |
| `/plugin [subcommand]` | 插件管理 | `/plugin list` |
| `/reload-plugins [--force]` | 重新加载插件 | `/reload-plugins` |
| `/claude-api [migrate or managed-agents-onboard]` | API 参考 / 迁移 / 托管 Agent 入职 | `/claude-api managed-agents-onboard` |
| `/run` | 启动并驱动应用验证变更 | `/run` |
| `/verify` | 构建、运行、观察（不仅测试） | `/verify` |
| `/plan [description]` | 进入计划模式 | `/plan 添加速率限制器` |

### 3.7 MCP、IDE 与集成

| 命令 | 功能 | 示例 |
|------|------|------|
| `/mcp [reconnect or enable or disable]` | MCP 服务器管理 | `/mcp reconnect github` |
| `/mcp__<server>__<prompt>` | MCP 暴露的 prompt（动态） | `/mcp__github__review_pr` |
| `/ide` | IDE 集成管理 | `/ide` |
| `/chrome` | Chrome 集成配置 | `/chrome` |
| `/desktop` | 在 Desktop App 中继续（别名 `/app`） | `/desktop` |
| `/teleport` | 将网页会话拉入终端（别名 `/tp`） | `/teleport` |
| `/remote-control` | 启用 claude.ai 远程控制（别名 `/rc`） | `/remote-control` |
| `/mobile` | 显示移动端 App QR 码（`/ios`, `/android`） | `/mobile` |
| `/voice [hold or tap or off]` | 语音输入模式 | `/voice hold` |

### 3.8 设置、UI 与账户

| 命令 | 功能 | 示例 |
|------|------|------|
| `/config [key=value]` | 设置界面（别名 `/settings`） | `/config thinking=false` |
| `/status` | 版本、模型、账户、连接状态 | `/status` |
| `/theme` | 主题选择器 | `/theme` |
| `/tui [default or fullscreen]` | 终端 UI 渲染器 | `/tui fullscreen` |
| `/focus` | 焦点视图（仅最后提示+摘要+回复） | `/focus` |
| `/scroll-speed` | 鼠标滚轮速度（仅全屏） | `/scroll-speed` |
| `/color [color or default]` | 提示栏颜色 | `/color blue` |
| `/statusline` | 配置状态栏 | `/statusline` |
| `/keybindings` | 键盘快捷键配置 | `/keybindings` |
| `/terminal-setup` | 终端快捷键设置 | `/terminal-setup` |
| `/login` / `/logout` | 登录/登出 | `/login` |
| `/upgrade` | 打开升级页面 | `/upgrade` |
| `/privacy-settings` | 隐私设置（Pro/Max） | `/privacy-settings` |
| `/passes` | 分享免费周卡 | `/passes` |
| `/help` | 帮助 | `/help` |
| `/release-notes` | 交互式更新日志 | `/release-notes` |
| `/insights` | 会话分析报告 | `/insights` |
| `/powerup` | 交互式功能教程 | `/powerup` |
| `/radio` | 打开 Claude FM lo-fi 电台 | `/radio` |

### 3.9 诊断与云提供商

| 命令 | 功能 | 示例 |
|------|------|------|
| `/doctor` | 诊断安装；按 `f` 自动修复 | `/doctor` |
| `/debug [description]` | 启用调试日志 | `/debug` |
| `/setup-bedrock` | Amazon Bedrock 配置向导 | `/setup-bedrock` |
| `/setup-vertex` | Google Vertex AI 配置向导 | `/setup-vertex` |

---

## 4. 键盘快捷键

### 输入与编辑

| 快捷键 | 功能 |
|--------|------|
| `Enter` | 发送消息 |
| `Shift+Enter` | 插入换行（不发送） |
| `\` + `Enter` | 插入换行（通用兼容） |
| `Tab` | 自动补全文件路径/命令 |
| `@` | 触发文件路径自动补全 |
| `!` | Bash 模式（直接执行命令） |
| `/` | 显示可用命令和 skills |

### 导航与历史

| 快捷键 | 功能 |
|--------|------|
| `↑` | 浏览输入历史（输入为空时） |
| `Ctrl+R` | 反向搜索命令历史 |
| `Ctrl+S` | 缩小搜索范围到当前项目/会话 |
| `{` / `}` | 跳转到上/下一个用户提示（全屏模式） |

### 控制与中断

| 快捷键 | 功能 |
|--------|------|
| `Escape` | 中断当前操作 / 清除输入 |
| `Esc+Esc` | 回滚或总结对话 |
| `Ctrl+C` | 中断运行中的任务 |
| `Ctrl+D` | 退出会话 |
| `Ctrl+B` | 后台运行任务（tmux 用户按两次） |
| `Ctrl+X Ctrl+K` | 终止所有后台 Agent（3 秒内按两次确认） |

### 输入编辑（readline 兼容）

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+A` | 光标移到行首 |
| `Ctrl+E` | 光标移到行尾 |
| `Ctrl+K` | 删除光标到行尾 |
| `Ctrl+U` | 清除整个输入缓冲区（`Ctrl+Y` 恢复） |
| `Ctrl+W` | 删除前一个词 |
| `Ctrl+Y` | 粘贴删除的文本 |
| `Alt+Y` | 循环粘贴历史 |
| `Alt+B` / `Alt+F` | 光标左/右移动一个词 |

### 视图与显示

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+L` | 清除输入并重绘屏幕 |
| `Ctrl+O` | 切换详细输出 |
| `Ctrl+T` | 切换任务列表可见性 |
| `Ctrl+E` | 切换完整内容显示 |
| `Ctrl+G` | 在外部编辑器中编辑提示 |
| `Ctrl+X Ctrl+E` | 同上（readline 原生别名） |
| `Ctrl+V` | 从剪贴板粘贴图片 |
| `Shift+Tab` | 循环权限模式 |
| `Option+P` | 快速切换模型 |
| `Option+T` | 切换扩展思考 |
| `Option+O` | 切换快速模式 |
| `?` | 切换快捷键帮助面板（全屏模式） |
| `v` | 在 `$VISUAL`/`$EDITOR` 中打开对话 |
| `[` | 将对话写入终端原生滚动缓冲区 |

### 语音

| 快捷键 | 功能 |
|--------|------|
| 长按 `Space` | 按住说话，松开发送 |

---

## 5. CLI 启动参数与标志

### 基础启动

| 参数 | 功能 | 示例 |
|------|------|------|
| `claude` | 交互模式 | `claude` |
| `claude "prompt"` | 一次性提问后进入交互 | `claude "解释这个代码库"` |
| `claude -p "prompt"` | Print 模式（非交互，输出后退出） | `claude -p "生成 API 文档"` |
| `claude -c` | 继续上次会话 | `claude -c` |
| `claude -r [name]` | 恢复指定会话 | `claude -r bugfix-session` |
| `claude --model <model>` | 指定模型 | `claude --model sonnet` |
| `claude --effort <level>` | 设置 effort 级别 | `claude --effort high` |
| `claude --permission-mode <mode>` | 权限模式 | `claude --permission-mode auto` |
| `claude --debug` | 调试模式 | `claude --debug` |
| `claude --verbose` | 详细日志 | `claude --verbose` |

### Print 模式专用（脚本/CI）

| 参数 | 功能 | 示例 |
|------|------|------|
| `--output-format <format>` | 输出格式：`text`/`json`/`stream-json` | `--output-format json` |
| `--json-schema <schema>` | JSON Schema 验证输出 | `--json-schema '{"type":"object"}'` |
| `--max-turns <N>` | 限制 Agent 轮数 | `--max-turns 5` |
| `--max-budget-usd <amount>` | 花费上限 | `--max-budget-usd 5.00` |
| `--input-format <format>` | 输入格式 | `--input-format stream-json` |

### 权限与工具控制

| 参数 | 功能 | 示例 |
|------|------|------|
| `--allowedTools` | 无需提示即可运行的工具 | `--allowedTools "Bash(git log:*),Read"` |
| `--disallowedTools` | 完全禁止的工具 | `--disallowedTools "Bash(rm *),Edit"` |
| `--tools` | 限制可用工具集 | `--tools "Bash,Edit,Read"` |
| `--dangerously-skip-permissions` | 跳过所有权限提示（沙盒专用） | `--dangerously-skip-permissions` |

### 系统提示定制

| 参数 | 功能 | 示例 |
|------|------|------|
| `--system-prompt` | 替换整个系统提示 | `--system-prompt "你是 TS 专家"` |
| `--system-prompt-file` | 从文件加载系统提示 | `--system-prompt-file ./prompt.txt` |
| `--append-system-prompt` | 追加到默认系统提示 | `--append-system-prompt "总是用中文回复"` |
| `--append-system-prompt-file` | 追加文件内容 | `--append-system-prompt-file ./rules.txt` |

### 工作区与目录

| 参数 | 功能 | 示例 |
|------|------|------|
| `--worktree` | 在隔离的 Git worktree 中运行 | `--worktree` |
| `--tmux` | 在 tmux 会话中打开 worktree | `--worktree --tmux` |
| `--add-dir` | 添加额外工作目录 | `--add-dir /path/to/lib` |
| `--cd <path>` | 启动时切换目录 | `--cd ../other` |

### 后台与远程

| 参数 | 功能 | 示例 |
|------|------|------|
| `--bg` | 后台启动 | `--bg "重构 utils"` |
| `--exec` | 作为 PTY 后台作业运行 | `--bg --exec 'pytest -x'` |
| `--remote` | 创建 claude.ai 网页会话 | `--remote "修复 bug"` |
| `--remote-control` | 启用远程控制 | `--remote-control` |
| `--teleport` | 从网页恢复会话 | `--teleport` |

### MCP 与插件

| 参数 | 功能 | 示例 |
|------|------|------|
| `--mcp-config` | 从文件加载 MCP 配置 | `--mcp-config mcp.json` |
| `--strict-mcp-config` | 仅使用指定 MCP 配置 | `--strict-mcp-config` |
| `--plugin-dir` | 从目录加载插件 | `--plugin-dir /path/to/plugin` |
| `--plugin-url` | 从 URL 加载 zip 插件 | `--plugin-url https://.../plugin.zip` |
| `--disable-slash-commands` | 禁用所有 skills | `--disable-slash-commands` |

### 预算与回退

| 参数 | 功能 | 示例 |
|------|------|------|
| `--max-budget-usd` | API 花费上限 | `--max-budget-usd 10` |
| `--fallback-model` | 主模型不可用时的回退 | `--fallback-model sonnet,haiku` |
| `--no-session-persistence` | 临时会话（不保存） | `--no-session-persistence` |

### 其他实用参数

| 参数 | 功能 | 示例 |
|------|------|------|
| `--bare` | 最小模式（跳过 hooks、LSP、插件同步） | `--bare -p "query"` |
| `--safe-mode` | 安全模式（禁用所有自定义配置） | `--safe-mode` |
| `--name` | 设置会话显示名称 | `-n "feature-auth"` |
| `--from-pr` | 从 PR 恢复会话 | `--from-pr 123` |
| `--chrome` / `--no-chrome` | 启用/禁用 Chrome 集成 | `--chrome` |
| `--ide` | 自动连接 IDE | `--ide` |
| `--settings` | 从文件加载设置 | `--settings settings.json` |
| `--debug-file` | 写入调试日志到文件 | `--debug-file /tmp/debug.log` |
| `--betas` | 包含 beta headers | `--betas feature-name` |
| `--agent` | 指定 Agent 配置 | `--agent my-agent` |
| `--agents` | 内联定义自定义 Agent | `--agents '{"reviewer":{...}}'` |
| `--brief` | 启用 Agent-to-User 通信工具 | `--brief` |
| `--channels` | 指定通知 MCP 服务器 | `--channels plugin:notifier` |
| `--teammate-mode` | Agent Team 显示模式 | `--teammate-mode tmux` |
| `--fork-session` | 恢复为新会话 ID | `-c --fork-session` |
| `--session-id` | 指定会话 UUID | `--session-id <uuid>` |
| `--file` | 启动时下载文件资源 | `--file file_abc:doc.txt` |
| `--exclude-dynamic-system-prompt-sections` | 移动动态部分到用户消息 | `--exclude-dynamic-system-prompt-sections` |
| `--include-partial-messages` | 包含部分消息块 | `--include-partial-messages` |
| `--replay-user-messages` | 回显用户消息到 stdout | `--replay-user-messages` |
| `--include-hook-events` | 包含 hook 生命周期事件 | `--include-hook-events` |
| `--prompt-suggestions` | 启用提示建议 | `--prompt-suggestions` |
| `--init` | 运行初始化 hooks 后启动 | `--init` |
| `--init-only` | 仅运行初始化 hooks | `--init-only` |
| `--maintenance` | 运行维护 hooks 后退出 | `--maintenance` |
| `--advisor` | 启用顾问工具 | `--advisor opus` |
| `--ax-screen-reader` | 屏幕阅读器友好输出 | `--ax-screen-reader` |

### 子命令

| 命令 | 功能 | 示例 |
|------|------|------|
| `claude auth login/logout` | 登录/登出 | `claude auth login` |
| `claude agents` | 列出配置 | `claude agents --json` |
| `claude mcp add/list` | MCP 管理 | `claude mcp add github -s user -- npx -y @anthropic-ai/mcp-github` |
| `claude mcp login <name>` | 认证 MCP 服务器 | `claude mcp login server-name --no-browser` |
| `claude mcp logout <name>` | 登出 MCP 服务器 | `claude mcp logout server-name` |
| `claude plugin list` | 列出插件 | `claude plugin list` |
| `claude doctor` | 检查自动更新器 | `claude doctor` |
| `claude update` | 更新 | `claude update` |
| `claude install` | 安装/管理版本 | `claude install` |
| `claude setup-token` | 配置长期认证令牌 | `claude setup-token` |
| `claude auto-mode` | 检查自动模式分类器 | `claude auto-mode` |
| `claude remote-control` | 启动远程控制服务器 | `claude remote-control --name "My Project"` |
| `claude ultrareview` | 非交互式云端审查 | `claude ultrareview 123 --json` |
| `claude project purge` | 删除项目状态 | `claude project purge --dry-run` |

---

## 6. 权限与安全系统

### 权限模式

| 模式 | 说明 |
|------|------|
| `default` | 默认：每次工具调用都提示确认 |
| `acceptEdits` | 自动接受编辑，其他仍提示 |
| `plan` | 计划模式：执行前展示计划 |
| `auto` | 自动模式：Claude 自主执行（需分类器评估） |
| `dontAsk` | 极少提示（危险） |
| `bypassPermissions` | 完全绕过（极度危险） |

### 权限规则语法

```json
{
  "permissions": {
    "allow": ["Bash(git log:*)", "Read", "Grep"],
    "ask": ["Bash(rm *)", "Edit"],
    "deny": ["Bash(sudo *)", "Write(/etc/*)"]
  }
}
```

- `Bash(git log:*)` — 允许所有 `git log` 变体
- `Tool(param:value)` — 匹配工具参数（v2.1.178+）
- `*` 通配符支持
- `Agent(model:opus)` — 阻止 Opus 子 Agent

### 安全增强（v2.1.183+）

- 自动模式现在会阻止破坏性 git 命令（`git reset --hard`、`git checkout -- .`、`git clean -fd`、`git stash drop`）除非你明确要求丢弃本地工作
- `git commit --amend` 被阻止，除非提交是本 session 中 Agent 创建的
- `terraform destroy`/`pulumi destroy`/`cdk destroy` 被阻止，除非你指定了具体 stack

---

## 7. 模型与性能控制

### 可用模型（2026年6月）

| 模型 | 特点 | 适用场景 |
|------|------|----------|
| **Claude Opus 4.8** | 最强推理，默认 high effort | 复杂重构、架构设计 |
| **Claude Sonnet 4.6** | 平衡性能与速度 | 日常开发、多文件编辑 |
| **Claude Haiku 4.5** | 最快、最便宜 | 简单任务、快速查询 |
| **Claude Fable 5** | Mythos-class，超越以往所有通用模型 | 极限推理任务 |

### 上下文窗口

- Sonnet 4.6: 200K tokens
- Opus 4.7/4.8: 1M tokens（Max 订阅）

### 快速切换

```bash
# 交互式选择
/model

# 直接指定
/model sonnet
/model opus
/model fable

# 设置 effort
/effort xhigh
/effort ultracode

# 快速模式
/fast on
```

---

## 8. 并行与后台 Agent

### 后台 Agent

```bash
# 分离当前会话到后台
/background 重构所有测试文件

# 或启动时直接后台
claude --bg "优化数据库查询"

# 查看所有 Agent
claude agents

# 停止特定后台会话
/stop
```

### Agent Teams（研究预览）

- 主 Agent 可以委派多个子 Agent 在并行 worktree 中工作
- 使用 `/batch` 分解大型任务
- 子 Agent 可以嵌套最多 5 层深度（v2.1.172+）

### 动态工作流（v2.1.156+）

```bash
# 触发 ultracode 工作流
"帮我重构整个认证系统"

# 监控工作流进度
/workflows
```

---

## 9. 代码审查与发布

### 审查工作流

```bash
# 1. 查看变更
/diff

# 2. 本地审查（中等深度）
/code-review medium

# 3. 应用修复建议
/code-review --fix

# 4. 安全审查
/security-review

# 5. 云端深度审查（多 Agent 并行）
/ultrareview
# 或
/code-review ultra
```

### PR 自动修复

```bash
# 监控当前分支 PR，自动修复 CI 失败
/autofix-pr

# 限定范围
/autofix-pr only fix lint and type errors
```

---

## 10. MCP 服务器与扩展

### MCP 管理

```bash
# 查看已连接服务器
/mcp

# 重连特定服务器
/mcp reconnect github

# 启用/禁用
/mcp enable github
/mcp disable github

# CLI 添加 MCP 服务器
claude mcp add github -s user -- npx -y @anthropic-ai/mcp-github

# 列出服务器
claude mcp list

# 认证（v2.1.186+）
claude mcp login server-name
claude mcp login server-name --no-browser  # SSH/无头环境
claude mcp logout server-name
```

### MCP Prompt

连接 MCP 服务器后，会自动暴露 `/mcp__<server>__<prompt>` 命令：

```bash
/mcp__github__review_pr
/mcp__slack__send_message
```

---

## 11. 自定义技能与 Hooks

### Skills（推荐方式）

创建 `.claude/skills/<skill-name>/SKILL.md`：

```yaml
---
name: deploy
description: 部署应用到 staging 或 production
---

# 部署工作流

## 预部署检查
1. 运行完整测试: `npm test`
2. 检查未提交变更: `git status`
3. 确认当前分支正确

## 部署步骤
1. 构建生产包: `npm run build`
2. 如有需要运行数据库迁移
3. 执行部署: `./scripts/deploy.sh <environment>`
4. 验证部署健康检查
```

使用：
```bash
/deploy
# 或描述意图让 Claude 自动检测
"部署最新变更到 staging"
```

### Hooks

Hooks 在工具生命周期事件外运行，确定性执行：

- `SessionStart` — 会话开始时
- `PreToolUse` — 工具使用前
- `PostToolUse` — 工具使用后
- `SubagentStart` — 子 Agent 启动时
- `Stop` — 停止时

配置在 `/hooks` 或 `~/.claude/hooks/`。

---

## 12. 项目配置（CLAUDE.md）

### CLAUDE.md 作用

- 会话开始时自动加载
- 持久化跨会话的项目记忆
- 包含目录结构、常用命令、编码规范

### 生成

```bash
/init
```

### 手动编辑

```bash
/memory
```

### 典型结构

```markdown
# 项目概述

## 技术栈
- 前端: React + TypeScript
- 后端: Node.js + Express
- 数据库: PostgreSQL

## 常用命令
- `npm run dev` — 启动开发服务器
- `npm test` — 运行测试
- `npm run build` — 生产构建

## 编码规范
- 使用 TypeScript，禁用 `any`
- 所有函数必须包含 JSDoc
- 使用单引号

## 目录结构
/src
  /components — React 组件
  /utils — 工具函数
  /api — API 路由
```

---

## 13. 实用工作流模板

### 模板 A：新功能开发

```bash
claude                          # 启动
/init                           # 初始化项目记忆
/plan 添加用户认证模块          # 进入计划模式
# ... 审阅计划并确认 ...
/goal tests pass                # 设置完成条件
# Claude 自动执行直到测试通过
/code-review --fix              # 审查并修复
```

### 模板 B：大型重构

```bash
/batch 将 Jest 迁移到 Vitest    # 分解为并行子 Agent
/workflows                      # 监控进度
# 等待完成...
/diff                           # 查看所有变更
/ultrareview                    # 云端深度审查
```

### 模板 C：CI/CD 集成

```bash
# 生成 API 文档
claude -p "生成 src/api/ 的 API 文档" --output-format text > api-docs.md

# 安全审查
claude -p --max-budget-usd 1.00 --max-turns 5   --allowedTools "Read,Grep,Glob"   "审查最近提交的安全问题" --output-format json | jq '.result'

# 自动修复 PR
claude --bg --from-pr 123 "修复 CI 失败"
```

### 模板 D：长期维护

```bash
/loop 30m 检查所有 PR 状态      # 每30分钟检查
/goal 所有 PR 都通过 CI         # 持续工作直到达成
```

### 模板 E：研究与报告

```bash
/deep-research "2026年 React 状态管理最新趋势"
# 自动并行搜索、验证来源、生成引用报告
```

---

## 14. 故障排查

### 常见问题

| 问题 | 解决 |
|------|------|
| 上下文窗口满了 | `/compact` 压缩，或 `/clear` 清空 |
| 成本飙升 | `/cost` 检查，`/compact` 或切换更便宜模型 |
| 权限提示太多 | `/fewer-permission-prompts` 生成 allowlist |
| 配置损坏无法启动 | `claude --safe-mode` 禁用所有自定义配置 |
| 会话丢失 | `claude -r` 恢复 |
| 显示异常 | `Ctrl+L` 重绘，或 `/tui fullscreen` 切换渲染器 |
| 后台 Agent 卡住 | `claude agents` 查看，`Ctrl+X Ctrl+K` 终止 |
| MCP 连接失败 | `/mcp reconnect <server>` 或 `claude mcp login` |
| 安装问题 | `/doctor` 诊断，按 `f` 自动修复 |

### 环境变量速查

| 变量 | 作用 |
|------|------|
| `CLAUDE_CODE_SAFE_MODE=1` | 安全模式启动 |
| `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` | 禁用全屏渲染器 |
| `CLAUDE_CODE_EFFORT_LEVEL` | 默认 effort 级别 |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` | 默认 Opus 模型 |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | 默认 Sonnet 模型 |
| `CLAUDE_CODE_ENABLE_AUTO_MODE=1` | 启用自动模式（Bedrock/Vertex） |
| `CLAUDE_CODE_USE_BEDROCK=1` | 使用 Amazon Bedrock |
| `CLAUDE_CODE_USE_VERTEX=1` | 使用 Google Vertex AI |
| `CLAUDE_CODE_NEW_INIT=1` | 交互式 `/init` |
| `DISABLE_UPDATES=1` | 完全禁用更新 |

---

## 附录：命令类型速查

| 类型 | 含义 | 示例 |
|------|------|------|
| **Built-in** | 核心 CLI 行为 | `/model`, `/clear` |
| **Skill** | 捆绑 prompt，可自动触发 | `/batch`, `/loop`, `/code-review` |
| **Workflow** | 多 Agent 后台编排 | `/deep-research` |
| **MCP Prompt** | 从连接的服务器发现 | `/mcp__github__review_pr` |

---

> 提示: Claude Code 更新频繁，命令可用性因版本、计划和平台而异。输入 `/` 查看当前环境暴露的所有命令，或访问 `code.claude.com/docs` 获取最新官方文档。
