# Agent Team Mode 设计文档

## 概述

Agent Team Mode 是 BaiyuAISpace2 的一种工作模式。在该模式下，BaiyuAISpace2 会利用 MCP，模仿人类在现实中的协作，实现多 Agent 之间的交流、沟通，实现 Agent 集群协同。

相较于 Anthropic、Moonshot 等 AI 服务商的 Agent 协同功能，Baiyu 的 Agent 协同功能有几个优点：

1. **配置更自由**——模型、MCP、Prompt 自由更改，不受任何限制
2. **过程更可控**——Agent 进程完全透明，随时可以打断、提问、讨论
3. **初期成本更低**——无需向服务商支付昂贵的订阅费，模型可以用 API 或本地部署
4. **生态更开放**——BaiyuAISpace2 完全开源，MCP 等 Agent 配置也可以导入社区方案

---

## 工作流程

### 1. 任务启动

- User 开启 Agent Team Mode 后，User 可以向主 Agent 传达任务
- 主 Agent 与 User 商议完任务具体细节后，主 Agent 通过 MCP 创建 Workspace
- MCP 返回 Workspace 创建成功后，主 Agent 通过 MCP 向 Workspace 传递子 Agent 的参数，如 Prompt

### 2. Workspace 连接与监控

- Workspace 会连接至 BaiyuAISpace2 的端口，将整个 Workspace 的状态实时反馈给软件，用户在软件中监控 Agent 的工作
- 在主 Agent 传递完子 Agent 的创建参数后，用户在前端确认 Prompt、模型等信息
- 用户确认完成后，Workspace 创建子 Agent，主 Agent 完成工作环境的简单处理后暂时退场（Sleep）
- 具体工作全部交给子 Agent，每一个子 Agent 都拥有独立的上下文与对话窗口，User 可以在软件内任意查看进程

### 3. 子 Agent 工作操作

子 Agent 在工作时，可以进行如下操作：

#### ① 思考、输出、各种工具操作

#### ② workspace_meeting（会议）

- 这是一个 Workspace 提供的 MCP，用于为 Agent 之间提供沟通渠道
- Agent 可以调用此 MCP，呼叫其他的一个/多个 Agent，其他 Agent 的进程会被打断，Workspace 会向与会 Agent 的会话中发送 Meeting 通知，收到 Meeting 通知的 Agent 会调用此 MCP 与会
- Agent 每一次发言后，发言会以 MCP 结果的形式传入各个 Agent 的进程
- Agent 完成思考后，再次调用 MCP 进入会议频道
- 发言的 Agent 将自己的发言传入 MCP，Workspace 检测到所有与会 Agent 都进入会议频道时，再将发言以结果的形式传入其他 Agent 的 MCP，然后不断循环
- 在会议进行到合适阶段时，Agent 们可以提议散会
- 待所有与会 Agent 都在一个循环内向 MCP 传入同意散会参数后，Workspace 将用 MCP 返回会议结束，然后 Agent 各自继续工作
- **Agent 发言顺序**：发起者 → Agent 码小的 → Agent 码大的，以此循环

#### ③ workspace_message（消息）

- 这是一个 Workspace 提供的 MCP，用于为 Agent 之间提供沟通渠道
- Agent 可以调用此 MCP，将想对其他 Agent 说的话传入
- Workspace 收到信息后，会向目标 Agent 的会话中发送消息
- 消息会有发送者的 Agent 码

#### ④ workspace_asks（询问）

- 这是一个 Workspace 提供的 MCP，用于为 Agent 与 User 之间提供沟通渠道
- Agent 可以调用此 MCP，将想对 User 说的问题传入
- Workspace 收到信息后，会在 User 的前端显示问题
- User 输入回答后，Workspace 将回答以结果的形式传入 MCP
- 消息会附带 Agent 的详细信息，并可透显示上下文

#### ⑤ workspace_sleep（休眠）

- 这是一个 Workspace 提供的 MCP，用于为 Agent 提交休眠申请
- 子 Agent 在其认为完成了任务后，可以调用此 MCP，Workspace 收到休眠请求后，会向主 Agent 的会话中发送子 Agent 的休眠申请
- 消息会附带子 Agent 的详细信息
- 主 Agent 审批的结果会以 MCP 结果的形式返回，然后子 Agent 的会话被挂起或者继续

#### ⑥ workspace_agent_list（通讯录）

- 这是一个 Workspace 提供的 MCP，用于让 Agent 知道当前各个 Agent 的状态
- Agent 可以调用此 MCP，获取当前 Agent 列表，上有各 Agent 的详细信息

#### ⑦ workspace_log（日志）

- 这是一个 Workspace 提供的 MCP，用于记录 Agent 的工作
- Agent 可以调用此 MCP，写入或读取工作日志，该日志全 Agent 共享

### 4. 主 Agent 唤醒与任务完成

- 日志会通过 Workspace 实时显示在 User 的前端
- Agent 可以通过 Workspace 实时与主 Agent 或子 Agent 交流
- User 在 Workspace 运行期间随时可以与主 Agent 或子 Agent 交流
- User 还可以创建新的子 Agent，与现有子 Agent 共同协同
- 当 Workspace 检测到所有子 Agent 处于休眠状态时，Workspace 会向主 Agent 发送消息，令其验收成果
- 若主 Agent 认为任务未完成或者有待调整处，则会唤醒或创建合适的子 Agent 继续工作
- Workspace 也可以向 User 发送任务已完成的通知，然后封存自己，等待 User 唤醒
- 若 User 认为任务未完成或者有待调整处，User 可以唤醒主 Agent 或子 Agent
- 至此，一轮精彩的 Agent 协作结束

---

## 待讨论与完善的问题

### 1. 会议冲突仲裁

- **多 Agent 同时发起会议**：Agent A 和 Agent B 同时调用 `workspace_meeting` 的 `initiate`，Workspace 如何仲裁？
  - 先到先服务？
  - Agent 码小的优先？
  - 还是合并成一个大会？

### 2. 会议中的动态加入/退出

- 会议进行中，新 Agent 创建完成，想加入当前会议，如何处理？
- 会议中某个 Agent 被 User 强制唤醒/干预，如何优雅退出会议？

### 3. 消息优先级

- `workspace_message` 是异步的，但如果对方正在开会，消息怎么处理？
  - 立即送达打断？
  - 排队等会议结束？
  - 是否需要区分紧急/普通消息？

### 4. 日志写入权限与冲突

- `workspace_log` 是"全 Agent 共享"，但：
  - 谁可以写？所有 Agent 都能写？
  - 写冲突怎么办（两个 Agent 同时写同一条日志）？
  - 日志有结构化格式要求吗？

### 5. 主 Agent 唤醒策略

- 文档规定"所有子 Agent 休眠后唤醒主 Agent"，但如果某个子 Agent **死循环**或**拒绝休眠**，主 Agent 永远醒不来？
- 是否需要超时机制或 User 强制唤醒？

### 6. BPC 协议融合

- 如何将 BPC 的 P2P 直连和 Hub 中继能力融入 Agent Team Mode？
- Workspace 是本地进程还是网络服务？
- Agent 跨设备时 MCP 如何路由？

### 7. 计算资源分配

- 本地跑多个 Agent，内存/CPU 怎么分配？
- 远程 Agent（通过 BPC）怎么调度？

### 8. Prompt 模板版本化

- Workspace MCP 使用教程（SysPrompt 注入内容）如何做成可版本化的配置？
- 具体格式和更新机制如何设计？

### 9. 会议发起前的 Agent 状态检测机制（已部分讨论）

- Workspace 在发送会议通知前检测与会 Agent 是否正在执行 MCP 操作
- 若 Agent 正在执行瞬时 MCP（如 read_file），等待完成后再通知
- 若 Agent 正在执行长时 MCP（如终端编译），挂起会话（终端后台继续），Agent 去开会
- 若 Agent 正在 `asks_user` 等待用户输入，返回标记值让 Agent 去开会，用户输入暂存 Workspace 缓存，会议结束后送达
- 各场景的说明需写入 Agent 的 SysPrompt

### 10. Workspace MCP 参数格式规范

- 模型需按固定 JSON 格式调用 Workspace MCP
- Workspace 的事件逻辑判定偏向条件式算法
- 需在 Agent 的 SysPrompt 中注入 Workspace MCP 使用教程（含调用格式、参数说明、状态机规则、错误处理等）

---

*文档版本：v0.1*
*最后更新：2026-05-23*
