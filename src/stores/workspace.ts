/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * Agent Team Mode (Workspace) Store
 *
 * 管理工作组、Agent、消息、活动日志，以及主 Agent 创建子 Agent 提议 /
 * 休眠审批 / 提问这三类需要用户介入的"待处理事项"。
 *
 * 后端 Workspace 结构体都用 `#[serde(rename_all = "camelCase")]`，所以这里
 * 的字段名直接是 camelCase，不像 MCPServer 那样需要 snake_case。
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AgentRole = "main" | "sub";
export type AgentStatus = "idle" | "running" | "waiting_approval" | "waiting_answer" | "sleeping" | "meeting" | "error";

export interface Workspace {
  id: string;
  name: string;
  description: string;
  maxAgents: number;
  createdAt: number;
  updatedAt: number;
}

export interface WorkspaceAgent {
  id: string;
  workspaceId: string;
  name: string;
  role: AgentRole;
  provider: string;
  model: string;
  baseUrl: string;
  apiConfigId: string;
  systemPrompt: string;
  mcpServerIds: string[];
  knowledgeBaseIds: string[];
  activeSkillIds: string[];
  status: AgentStatus;
  createdAt: number;
  updatedAt: number;
}

export interface WorkspaceMessage {
  id: string;
  workspaceId: string;
  fromAgentId: string;
  toAgentId: string;
  content: string;
  createdAt: number;
}

export interface WorkspaceLogEntry {
  id: string;
  workspaceId: string;
  agentId: string | null;
  kind: string;
  content: string;
  createdAt: number;
}

export interface CreateAgentRequest {
  workspaceId: string;
  name: string;
  role: AgentRole;
  provider: string;
  model: string;
  baseUrl: string;
  apiConfigId: string;
  systemPrompt: string;
  mcpServerIds: string[];
  knowledgeBaseIds: string[];
  activeSkillIds: string[];
}

export interface AgentProposalEvent {
  proposalId: string;
  workspaceId: string;
  proposedByAgentId: string;
  proposedByAgentName: string;
  draft: CreateAgentRequest;
}

export interface SleepRequestEvent {
  requestId: string;
  workspaceId: string;
  agentId: string;
  agentName: string;
  reason: string;
}

export interface QuestionEvent {
  questionId: string;
  workspaceId: string;
  agentId: string;
  agentName: string;
  question: string;
}

interface AgentStatusEvent {
  agentId: string;
  status: AgentStatus;
}

/** 目标 Agent 没有存活的后台任务（多半是重启过应用），消息发了但不会有人处理。 */
export interface AgentInactiveEvent {
  agentId: string;
  agentName: string;
}

export const useWorkspaceStore = defineStore("workspace", () => {
  const workspaces = ref<Workspace[]>([]);
  const currentWorkspaceId = ref<string | null>(null);
  const agents = ref<WorkspaceAgent[]>([]);
  const messages = ref<WorkspaceMessage[]>([]);
  const logs = ref<WorkspaceLogEntry[]>([]);

  // 主 Agent 提议创建子 Agent / 申请休眠 / 向用户提问，都需要用户在前端处理。
  // 不按当前选中的工作组过滤 -- 后端只是触发一次性事件，没有"列出所有待处理事项"
  // 的命令，错过事件就再也找不回来了，所以哪怕用户当下没看着对应工作组，也要留着。
  const proposals = ref<AgentProposalEvent[]>([]);
  const sleepRequests = ref<SleepRequestEvent[]>([]);
  const questions = ref<QuestionEvent[]>([]);
  // 一次性提醒事件的队列，视图层 watch 它、用 message.warning() 弹出后自行清空 --
  // 这里不直接调用 useMessage()，因为 store 不在组件树里，拿不到 NMessageProvider 的上下文。
  const inactiveAgentNotices = ref<AgentInactiveEvent[]>([]);

  const currentWorkspace = computed(() => workspaces.value.find((w) => w.id === currentWorkspaceId.value) ?? null);

  let unlistenFns: UnlistenFn[] = [];

  const initListeners = async () => {
    if (unlistenFns.length > 0) return;
    console.log("[Workspace] 初始化事件监听器");
    unlistenFns = await Promise.all([
      listen<WorkspaceMessage>("workspace://message", (e) => {
        console.log(`[Workspace] 消息: ${e.payload.fromAgentId} → ${e.payload.toAgentId} | ${e.payload.content.slice(0, 60)}`);
        if (e.payload.workspaceId === currentWorkspaceId.value) messages.value.push(e.payload);
      }),
      listen<WorkspaceLogEntry>("workspace://log", (e) => {
        console.debug(`[Workspace] 日志 [${e.payload.kind}]: ${e.payload.content.slice(0, 80)}`);
        if (e.payload.workspaceId !== currentWorkspaceId.value) return;
        logs.value.push(e.payload);
        // 主 Agent 的提议被批准后，子 Agent 是在其后台任务里异步创建的：
        // 这条日志是创建完成的唯一前端信号，createAgent() 那种"invoke 返回值
        // 直接 push 进 agents"的手动创建路径在这里走不通，得靠它触发一次
        // 重新拉取，否则新 Agent 只会进日志时间线，不会出现在 Agent 列表里。
        if (e.payload.kind === "agent_created") loadAgents();
      }),
      listen<AgentStatusEvent>("workspace://agent-status", (e) => {
        const agent = agents.value.find((a) => a.id === e.payload.agentId);
        if (agent) {
          console.log(`[Workspace] Agent「${agent.name}」状态: ${agent.status} → ${e.payload.status}`);
          agent.status = e.payload.status;
        }
      }),
      listen<AgentProposalEvent>("workspace://agent-proposal", (e) => {
        console.log(`[Workspace] Agent 提议创建子 Agent: proposalId=${e.payload.proposalId} by ${e.payload.proposedByAgentName}`);
        proposals.value.push(e.payload);
      }),
      listen<SleepRequestEvent>("workspace://sleep-request", (e) => {
        console.log(`[Workspace] 休眠申请: requestId=${e.payload.requestId} agent=${e.payload.agentName} reason=${e.payload.reason}`);
        sleepRequests.value.push(e.payload);
      }),
      listen<QuestionEvent>("workspace://question", (e) => {
        console.log(`[Workspace] Agent 提问: questionId=${e.payload.questionId} agent=${e.payload.agentName} | ${e.payload.question.slice(0, 60)}`);
        questions.value.push(e.payload);
      }),
      listen<AgentInactiveEvent>("workspace://agent-inactive", (e) => {
        console.log(`[Workspace] Agent 未在运行: agentId=${e.payload.agentId} name=${e.payload.agentName}`);
        inactiveAgentNotices.value.push(e.payload);
      }),
    ]);
  };

  const disposeListeners = () => {
    unlistenFns.forEach((fn) => fn());
    unlistenFns = [];
  };

  const listWorkspaces = async () => {
    workspaces.value = await invoke<Workspace[]>("workspace_list");
  };

  const createWorkspace = async (name: string, description: string, maxAgents: number): Promise<Workspace> => {
    console.log(`[Workspace] 创建工作组: name=${name} maxAgents=${maxAgents}`);
    const ws = await invoke<Workspace>("workspace_create", { request: { name, description, maxAgents } });
    console.log(`[Workspace] 工作组已创建: id=${ws.id}`);
    workspaces.value.unshift(ws);
    return ws;
  };

  const deleteWorkspace = async (workspaceId: string) => {
    await invoke("workspace_delete", { workspaceId });
    workspaces.value = workspaces.value.filter((w) => w.id !== workspaceId);
    if (currentWorkspaceId.value === workspaceId) {
      currentWorkspaceId.value = null;
      agents.value = [];
      messages.value = [];
      logs.value = [];
    }
  };

  const loadAgents = async () => {
    if (!currentWorkspaceId.value) return;
    agents.value = await invoke<WorkspaceAgent[]>("workspace_list_agents", { workspaceId: currentWorkspaceId.value });
  };

  const loadMessages = async () => {
    if (!currentWorkspaceId.value) return;
    messages.value = await invoke<WorkspaceMessage[]>("workspace_list_messages", { workspaceId: currentWorkspaceId.value });
  };

  const loadLogs = async () => {
    if (!currentWorkspaceId.value) return;
    logs.value = await invoke<WorkspaceLogEntry[]>("workspace_list_logs", { workspaceId: currentWorkspaceId.value });
  };

  const selectWorkspace = async (workspaceId: string) => {
    currentWorkspaceId.value = workspaceId;
    await Promise.all([loadAgents(), loadMessages(), loadLogs()]);
  };

  const createAgent = async (request: CreateAgentRequest): Promise<WorkspaceAgent> => {
    console.log(`[Workspace] 创建 Agent: name=${request.name} role=${request.role} model=${request.provider}/${request.model}`);
    const agent = await invoke<WorkspaceAgent>("workspace_create_agent_manual", { request });
    console.log(`[Workspace] Agent 已创建: id=${agent.id}`);
    agents.value.push(agent);
    return agent;
  };

  const deleteAgent = async (agentId: string) => {
    await invoke("workspace_delete_agent", { agentId });
    agents.value = agents.value.filter((a) => a.id !== agentId);
  };

  const sendUserMessage = async (toAgentId: string, content: string) => {
    if (!currentWorkspaceId.value) return;
    await invoke("workspace_send_user_message", { workspaceId: currentWorkspaceId.value, toAgentId, content });
  };

  const resolveProposal = async (proposalId: string, approved: boolean, request?: CreateAgentRequest) => {
    console.log(`[Workspace] 处理 Agent 提议: proposalId=${proposalId} approved=${approved}`);
    await invoke("workspace_resolve_proposal", { proposalId, approved, request });
    proposals.value = proposals.value.filter((p) => p.proposalId !== proposalId);
  };

  const resolveSleepRequest = async (requestId: string, approved: boolean) => {
    console.log(`[Workspace] 处理休眠申请: requestId=${requestId} approved=${approved}`);
    await invoke("workspace_resolve_sleep_request", { requestId, approved });
    sleepRequests.value = sleepRequests.value.filter((r) => r.requestId !== requestId);
  };

  const resolveQuestion = async (questionId: string, answer: string) => {
    console.log(`[Workspace] 回答 Agent 提问: questionId=${questionId} answer=${answer.slice(0, 40)}`);
    await invoke("workspace_resolve_question", { questionId, answer });
    questions.value = questions.value.filter((q) => q.questionId !== questionId);
  };

  /** 把一个 Agent id（或 "user"/"all"/"system"）解析成显示用的名字。 */
  const agentName = (agentId: string): string => {
    if (agentId === "user") return "用户";
    if (agentId === "all") return "所有人";
    if (agentId === "system") return "系统";
    return agents.value.find((a) => a.id === agentId)?.name ?? agentId;
  };

  return {
    workspaces,
    currentWorkspaceId,
    currentWorkspace,
    agents,
    messages,
    logs,
    proposals,
    sleepRequests,
    questions,
    inactiveAgentNotices,
    initListeners,
    disposeListeners,
    listWorkspaces,
    createWorkspace,
    deleteWorkspace,
    selectWorkspace,
    loadAgents,
    loadMessages,
    loadLogs,
    createAgent,
    deleteAgent,
    sendUserMessage,
    resolveProposal,
    resolveSleepRequest,
    resolveQuestion,
    agentName,
  };
});
