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

## 核心设计决策

### 1. 会议中 Agent 卡死处理机制

**问题**：单个 Agent 在会议发言环节卡住（Token 流停止），会导致整个会议陷入僵局。

**解决方案**：双层检测 + 智能延期

**实现细节**：

#### 检测层
- Workspace 监听 Agent 对应模型的 Token 流输出
- 如果 Token 流**彻底断掉且超过 30 秒无新 Token**，判定该 Agent 异常
- 不依赖心跳、不强制打断 Agent，完全被动观察

#### 异常处理
- 该 Agent 在本轮会议中**标记为缺席**
- 会议继续轮询其他 Agent，不等待该 Agent
- 该 Agent 恢复后可以从会议记录里读到错过的发言（catch-up 机制）
- 在之后的轮次正常插入发言序列继续轮询

#### 时间延期（不异常情况）
- 如果 Token 流还在产生但距离**预估完成时间已经很近**，自动延期
- 延期时长计算方式：观测最近 5 秒的 Token 生成速率，基于剩余待发言内容估算所需时间，再加 buffer
- 避免"发言还在生成就被强制截断"的尴尬

#### User 干预
- Workspace 向 User 发送"某 Agent 在会议中异常"的告警（可选）
- User 可以选择：
  - 等待 Agent 恢复（不干预）
  - 强制踢出该 Agent
  - 强制散会

**优势**：
- 完全自动化，Agent 无感知
- 同时处理长发言（慢模型）和真正的卡死
- 会议不被单个 Agent 拖累

---

### 2. Workspace MCP 教程的 Token 成本优化

**问题**：不能把所有 Workspace MCP 教程都放在 SysPrompt，这样每个 Agent 启动都要额外消耗数千个 Token，且不支持任意模型（Prompt Caching 只对 Claude API 有效）。

**解决方案**：分层工具 + 按需查询

**架构设计**：

#### 第 1 层：工具 Schema 自解释（无额外成本）
将所有 Workspace MCP 作为标准工具暴露给 Agent，每个工具都有清晰的 JSON Schema：

```json
{
  "name": "workspace_meeting",
  "description": "发起或加入多 Agent 会议。Agent 轮流发言，可选散会。",
  "inputSchema": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["initiate", "join", "speak", "leave"],
        "description": "initiate: 发起新会议; join: 加入已有会议; speak: 发言; leave: 离开会议"
      },
      "meeting_id": {
        "type": "string",
        "description": "会议 ID（join/speak/leave 时必需）"
      },
      "content": {
        "type": "string",
        "description": "发言内容（speak 时必需）"
      }
    }
  }
}
```

Schema 本身就是文档，Agent 可以从工具名称、描述和参数推断出使用方法。

#### 第 2 层：SysPrompt 中的极简引导（~100 token）
```
你现在在一个 Workspace 协作环境中工作。

系统为你提供了以下 MCP 工具：
- workspace_meeting：发起或加入多 Agent 会议，轮流发言
- workspace_message：向其他 Agent 发送点对点消息
- workspace_asks：向用户提问
- workspace_sleep：任务完成后申请休眠
- workspace_agent_list：查看其他 Agent 的状态
- workspace_log：写入/读取共享工作日志
- workspace_help：查询任何工具的完整文档和使用示例

根据每个工具的名称、描述和 JSON Schema，你可以推断出基本用法。
如果需要详细说明或不确定如何使用，调用 workspace_help("workspace_meeting") 等查询。
```

#### 第 3 层：按需查询（workspace_help MCP）
Agent 在不确定时可以调用：
```json
{
  "name": "workspace_help",
  "description": "查询 Workspace MCP 的完整文档、使用示例和常见问题",
  "inputSchema": {
    "type": "object",
    "properties": {
      "topic": {
        "type": "string",
        "description": "查询主题，如 'workspace_meeting'、'how_to_recover_from_conference'、'catch_up_mechanism' 等"
      }
    }
  }
}
```

Workspace 返回该话题的详细文档（包含完整 API 说明、状态机规则、错误处理等）。

**成本分析**：
- SysPrompt 固定成本：~100 token（vs 原方案的 2000-5000 token）
- 按需查询成本：仅在 Agent 调用 `workspace_help` 时产生，且单个对话内缓存复用
- 总体降低 80-95% 的 SysPrompt 成本

**模型兼容性**：
- 完全模型无关，不依赖任何特定模型的特性
- Schema 是标准的 OpenAI Function Calling 格式，所有现代 LLM 都支持
- 本地模型、远程 API、闭源模型都能正常使用

**扩展性**：
- 新增 Workspace MCP 工具时，只需定义 Schema，无需改 SysPrompt
- `workspace_help` 的文档库可以热更新，无需重启 Agent

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

*文档版本：v0.2*
*最后更新：2026-06-24*

**v0.2 更新**：补充"核心设计决策"章节，包含会议 Agent 卡死处理机制和 Workspace MCP 教程成本优化方案。
