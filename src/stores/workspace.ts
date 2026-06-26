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

  const currentWorkspace = computed(() => workspaces.value.find((w) => w.id === currentWorkspaceId.value) ?? null);

  let unlistenFns: UnlistenFn[] = [];

  const initListeners = async () => {
    if (unlistenFns.length > 0) return;
    unlistenFns = await Promise.all([
      listen<WorkspaceMessage>("workspace://message", (e) => {
        if (e.payload.workspaceId === currentWorkspaceId.value) messages.value.push(e.payload);
      }),
      listen<WorkspaceLogEntry>("workspace://log", (e) => {
        if (e.payload.workspaceId === currentWorkspaceId.value) logs.value.push(e.payload);
      }),
      listen<AgentStatusEvent>("workspace://agent-status", (e) => {
        const agent = agents.value.find((a) => a.id === e.payload.agentId);
        if (agent) agent.status = e.payload.status;
      }),
      listen<AgentProposalEvent>("workspace://agent-proposal", (e) => {
        proposals.value.push(e.payload);
      }),
      listen<SleepRequestEvent>("workspace://sleep-request", (e) => {
        sleepRequests.value.push(e.payload);
      }),
      listen<QuestionEvent>("workspace://question", (e) => {
        questions.value.push(e.payload);
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
    const ws = await invoke<Workspace>("workspace_create", { request: { name, description, maxAgents } });
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
    const agent = await invoke<WorkspaceAgent>("workspace_create_agent_manual", { request });
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
    await invoke("workspace_resolve_proposal", { proposalId, approved, request });
    proposals.value = proposals.value.filter((p) => p.proposalId !== proposalId);
  };

  const resolveSleepRequest = async (requestId: string, approved: boolean) => {
    await invoke("workspace_resolve_sleep_request", { requestId, approved });
    sleepRequests.value = sleepRequests.value.filter((r) => r.requestId !== requestId);
  };

  const resolveQuestion = async (questionId: string, answer: string) => {
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
