# Agent Team（协作团队）

Agent Team 是多 Agent 协作工作组：多个各有专长的 Agent 在同一工作组内互发消息、开会讨论、向用户提问，共同完成任务。

## 工作组管理

- 新建 / 删除工作组
- 每个工作组可配置 **最大 Agent 数量上限**

## Agent 生命周期

- **手动添加 / 删除** Agent
- **主 Agent 提议创建子 Agent**：主 Agent 认为需要帮手时可发起创建提议，**需用户审批**，超过 10 分钟未处理自动超时作废
- 每个 Agent **独立配置**：各自绑定 API 配置、系统提示词、MCP 服务器、知识库、Skill——组建一个「前端写代码、后端查资料」的异构团队完全可行

## 四种状态

| 状态 | 含义 |
|---|---|
| **Idle** | 空闲待命 |
| **Running** | 正在处理消息 |
| **Meeting** | 圆桌会议中 |
| **Sleeping** | 休眠（子 Agent 申请休眠需主 Agent 或用户批准） |

## 消息路由

- **点对点**：Agent 之间指定对象互发消息
- **广播**：`to_agent_id = "all"` 时发给全组
- **用户直连**：你可以直接与任意一个 Agent 对话，不必经过主 Agent

## 圆桌会议

任意 Agent 可通过 `workspace_meeting` 发起会议：

- 其余成员**按创建顺序轮流**就议题发言
- 某个成员超时未响应会**自动跳过**，不阻塞全场

> ⚠️ 已知问题：会议进行中重启 App，Agent 的 `meeting` 状态不会自动清理，需手动处理。见 [[常见问题FAQ]]。

## 向用户提问

Agent 遇到需要人类决策的问题时，通过 `workspace_asks` 弹出**提问卡片**，等待你回答后继续。

## 活动时间线

消息与日志合并展示为统一时间线；由定时任务触发的活动（`scheduled_trigger`）有专属条目标识。

## Agent 工具集

| 工具 | 谁能用 | 作用 |
|---|---|---|
| `workspace_message` | 所有 Agent | 发消息（点对点 / 广播） |
| `workspace_agent_list` | 所有 Agent | 查看组内成员 |
| `workspace_asks` | 所有 Agent | 向用户提问 |
| `workspace_log` | 所有 Agent | 写活动日志 |
| `workspace_meeting` | 所有 Agent | 发起圆桌会议 |
| `workspace_create_agent` | 仅主 Agent | 提议创建子 Agent（需用户审批） |
| `workspace_approve_sleep` / `workspace_reject_sleep` | 仅主 Agent | 批准 / 驳回子 Agent 休眠申请 |
| `workspace_sleep` | 仅子 Agent | 申请休眠 |

## 与定时任务联动

工作组页面的**时钟图标**可直接跳转到定时任务页并预填当前工作组筛选——定时唤醒某个 Agent（或广播全组）干活，见 [[定时任务]]。
