<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  AgentTeamView.vue - Agent Team Mode (Workspace) 视图

  功能说明:
  - 工作组的新建/选择/删除
  - Agent 列表 + 状态展示，手动添加/删除 Agent
  - 单个 Agent 的对话面板（复用 ChatMessage.vue）
  - 整个工作组的活动时间线（消息 + 操作日志合并展示）
  - 主 Agent 提议创建子 Agent / 申请休眠 / 向用户提问 三类待处理事项的确认卡片
-->

<script setup lang="ts">
import { ref, reactive, computed, onMounted, watch } from "vue";
import { useRouter } from "vue-router";
import {
  NButton, NIcon, NSpace, NSelect, NEmpty, NModal, NForm, NFormItem, NInput, NInputNumber,
  NRadioGroup, NRadio, NCheckboxGroup, NCheckbox, NCard, NGrid, NGi, NList, NListItem, NThing,
  NTag, NTimeline, NTimelineItem, NPopconfirm, NSwitch, NTooltip, useMessage,
} from "naive-ui";
import {
  Add, TrashOutline, EnterOutline, AlarmOutline, PencilOutline, MegaphoneOutline,
  PauseOutline, PlayOutline, RefreshOutline,
} from "@vicons/ionicons5";

import ChatMessage from "@/components/ChatMessage.vue";
import {
  useWorkspaceStore, AGENT_GUIDELINES_BASE, AGENT_GUIDELINES_SUB,
  type AgentProposalEvent, type AgentRole, type AgentStatus, type CreateAgentRequest,
  type UpdateAgentRequest, type WorkspaceAgent, type WorkspaceAgentTask, type WorkspaceLogEntry,
  type ToolApprovalEvent,
} from "@/stores/workspace";
import { useSettingsStore } from "@/stores/settings";
import { useMCPStore } from "@/stores/mcp";
import { useKnowledgeBaseStore } from "@/stores/knowledgeBase";
import { useSkillsStore } from "@/stores/skills";

const workspace = useWorkspaceStore();
const settings = useSettingsStore();
const mcp = useMCPStore();
const kb = useKnowledgeBaseStore();
const skills = useSkillsStore();
const router = useRouter();
const message = useMessage();

// ============ 工作组 ============

const showCreateWorkspaceModal = ref(false);
const wsForm = ref({ name: "", description: "", maxAgents: 5 });

const workspaceOptions = computed(() =>
  workspace.workspaces.map((w) => ({ label: w.name, value: w.id }))
);

const handleSelectWorkspace = async (id: string) => {
  selectedAgentId.value = null;
  try {
    await workspace.selectWorkspace(id);
  } catch (e) {
    message.error(`加载工作组失败: ${e}`);
  }
};

const handleCreateWorkspace = async () => {
  if (!wsForm.value.name.trim()) {
    message.error("请填写工作组名称");
    return;
  }
  try {
    const ws = await workspace.createWorkspace(wsForm.value.name.trim(), wsForm.value.description.trim(), wsForm.value.maxAgents);
    showCreateWorkspaceModal.value = false;
    wsForm.value = { name: "", description: "", maxAgents: 5 };
    await handleSelectWorkspace(ws.id);
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
};

const handleDeleteWorkspace = async () => {
  if (!workspace.currentWorkspaceId) return;
  try {
    await workspace.deleteWorkspace(workspace.currentWorkspaceId);
    message.success("已删除工作组");
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
};

// ============ Agent 列表 / 创建 / 删除 ============

const selectedAgentId = ref<string | null>(null);
const selectedAgent = computed(() => workspace.agents.find((a) => a.id === selectedAgentId.value) ?? null);

const showCreateAgentModal = ref(false);
// 系统提示词预填默认协作行为准则（防止 Agent 间无意义互相唤醒刷 API 调用），
// 用户可见、可改、可删。主管/子 Agent 的准则略有差异（休眠条目子 Agent 才有）。
const roleGuidelines: Record<AgentRole, string> = {
  main: AGENT_GUIDELINES_BASE,
  sub: AGENT_GUIDELINES_SUB,
};
const emptyAgentForm = (): CreateAgentRequest => ({
  workspaceId: workspace.currentWorkspaceId ?? "",
  name: "",
  role: "sub",
  provider: "",
  model: "",
  baseUrl: "",
  apiConfigId: "",
  systemPrompt: AGENT_GUIDELINES_SUB,
  mcpServerIds: [],
  knowledgeBaseIds: [],
  activeSkillIds: [],
  ragTopK: 5,
  ragRetrievalMode: "hybrid",
  ragRerankerConfigId: null,
  ragRerankerBaseUrl: null,
  ragRerankerModel: null,
  ragRerankTopN: null,
  requireToolApproval: true,
  enableThinking: false,
});
const agentForm = ref<CreateAgentRequest>(emptyAgentForm());

const retrievalModeOptions = [
  { label: "混合检索（推荐）", value: "hybrid" },
  { label: "纯向量检索", value: "vector" },
  { label: "纯关键词检索", value: "keyword" },
];

/** 把选中的 reranker 配置 id 解析为 baseUrl/model 一并存到 Agent 上——Rust 后端
 *  只存 id 用于取密钥，base_url/model 得跟主聊天一样由前端 settings store 解析后传过去。 */
const resolveReranker = (configId: string | null) => {
  if (!configId) return { ragRerankerConfigId: null, ragRerankerBaseUrl: null, ragRerankerModel: null };
  const cfg = settings.rerankerApiConfigs.find((c) => c.id === configId);
  return {
    ragRerankerConfigId: configId,
    ragRerankerBaseUrl: cfg?.baseUrl ?? null,
    ragRerankerModel: cfg?.model ?? null,
  };
};

// 切换角色时，如果提示词还是另一个角色的默认准则（用户没改过），跟着换成
// 当前角色的版本；用户已经自己写过内容就不动。
watch(
  () => agentForm.value.role,
  (role, prev) => {
    if (prev && agentForm.value.systemPrompt === roleGuidelines[prev]) {
      agentForm.value.systemPrompt = roleGuidelines[role];
    }
  }
);

const openCreateAgentModal = () => {
  agentForm.value = emptyAgentForm();
  showCreateAgentModal.value = true;
};

const handleCreateAgent = async () => {
  if (!agentForm.value.name.trim()) {
    message.error("请填写 Agent 名称");
    return;
  }
  if (!agentForm.value.apiConfigId) {
    message.error("请选择 API 配置");
    return;
  }
  const config = settings.apiConfigs.find((c) => c.id === agentForm.value.apiConfigId);
  if (!config) {
    message.error("找不到所选的 API 配置");
    return;
  }
  try {
    await workspace.createAgent({
      ...agentForm.value,
      workspaceId: workspace.currentWorkspaceId ?? "",
      provider: config.provider,
      model: config.model,
      baseUrl: config.baseUrl,
      ...resolveReranker(agentForm.value.ragRerankerConfigId ?? null),
    });
    showCreateAgentModal.value = false;
    message.success("Agent 已创建");
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
};

// ============ 编辑 Agent ============

const showEditAgentModal = ref(false);
const editAgentForm = ref<UpdateAgentRequest | null>(null);

const openEditAgentModal = (agent: WorkspaceAgent) => {
  editAgentForm.value = {
    id: agent.id,
    name: agent.name,
    provider: agent.provider,
    model: agent.model,
    baseUrl: agent.baseUrl,
    apiConfigId: agent.apiConfigId,
    systemPrompt: agent.systemPrompt,
    mcpServerIds: [...agent.mcpServerIds],
    knowledgeBaseIds: [...agent.knowledgeBaseIds],
    activeSkillIds: [...agent.activeSkillIds],
    ragTopK: agent.ragTopK,
    ragRetrievalMode: agent.ragRetrievalMode,
    ragRerankerConfigId: agent.ragRerankerConfigId,
    ragRerankerBaseUrl: agent.ragRerankerBaseUrl,
    ragRerankerModel: agent.ragRerankerModel,
    ragRerankTopN: agent.ragRerankTopN,
    requireToolApproval: agent.requireToolApproval,
    enableThinking: agent.enableThinking,
  };
  showEditAgentModal.value = true;
};

const handleUpdateAgent = async () => {
  const form = editAgentForm.value;
  if (!form) return;
  if (!form.name.trim()) {
    message.error("请填写 Agent 名称");
    return;
  }
  if (!form.apiConfigId) {
    message.error("请选择 API 配置");
    return;
  }
  const config = settings.apiConfigs.find((c) => c.id === form.apiConfigId);
  if (!config) {
    message.error("找不到所选的 API 配置");
    return;
  }
  try {
    await workspace.updateAgent({
      ...form,
      provider: config.provider,
      model: config.model,
      baseUrl: config.baseUrl,
      ...resolveReranker(form.ragRerankerConfigId),
    });
    showEditAgentModal.value = false;
    message.success("Agent 已更新");
  } catch (e) {
    message.error(`更新失败: ${e}`);
  }
};

// ============ 广播消息 ============

const showBroadcastModal = ref(false);
const broadcastContent = ref("");

const handleBroadcastSend = async () => {
  const content = broadcastContent.value.trim();
  if (!content) return;
  try {
    await workspace.sendUserMessage("all", content);
    broadcastContent.value = "";
    showBroadcastModal.value = false;
    message.success("已广播给所有 Agent");
  } catch (e) {
    message.error(`发送失败: ${e}`);
  }
};

const handleDeleteAgent = async (agentId: string) => {
  try {
    await workspace.deleteAgent(agentId);
    if (selectedAgentId.value === agentId) selectedAgentId.value = null;
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
};

// ============ 暂停 / 恢复（紧急停止手段） ============

const handlePauseAgent = async (agentId: string) => {
  try {
    await workspace.pauseAgent(agentId);
    message.success("已暂停");
  } catch (e) {
    message.error(`暂停失败: ${e}`);
  }
};

const handleResumeAgent = async (agentId: string) => {
  try {
    await workspace.resumeAgent(agentId);
    message.success("已恢复运行");
  } catch (e) {
    message.error(`恢复失败: ${e}`);
  }
};

// ============ 待处理事项：MCP 工具调用审批 ============

const handleResolveToolApproval = async (t: ToolApprovalEvent, approved: boolean) => {
  try {
    await workspace.resolveToolApproval(t.approvalId, approved);
  } catch (e) {
    message.error(`处理失败: ${e}`);
  }
};

// ============ 选中 Agent 的结构化任务清单 ============

const agentTasks = ref<WorkspaceAgentTask[]>([]);
const loadAgentTasks = async () => {
  if (!selectedAgentId.value) {
    agentTasks.value = [];
    return;
  }
  try {
    agentTasks.value = await workspace.listAgentTasks(selectedAgentId.value);
  } catch (e) {
    message.error(`读取任务清单失败: ${e}`);
  }
};
watch(selectedAgentId, loadAgentTasks, { immediate: true });

const handleToggleTask = async (taskId: string, done: boolean) => {
  try {
    await workspace.setTaskDone(taskId, done);
    await loadAgentTasks();
  } catch (e) {
    message.error(`更新任务状态失败: ${e}`);
  }
};

const statusMeta: Record<AgentStatus, { label: string; type: "default" | "success" | "warning" | "error" | "info" }> = {
  idle: { label: "空闲", type: "default" },
  running: { label: "运行中", type: "success" },
  waiting_approval: { label: "等待审批", type: "warning" },
  waiting_answer: { label: "等待回答", type: "warning" },
  sleeping: { label: "已休眠", type: "info" },
  meeting: { label: "会议中", type: "info" },
  paused: { label: "已暂停", type: "warning" },
  error: { label: "异常", type: "error" },
};

// ============ 单 Agent 对话面板 ============

const newMessageContent = ref("");

const agentMessages = computed(() => {
  if (!selectedAgentId.value) return [];
  const id = selectedAgentId.value;
  return workspace.messages
    .filter((m) => m.fromAgentId === id || m.toAgentId === id || m.toAgentId === "all")
    .map((m) => ({
      id: m.id,
      role: (m.fromAgentId === id ? "assistant" : "user") as "assistant" | "user",
      content: m.fromAgentId === id || m.fromAgentId === "user" ? m.content : `[来自 ${workspace.agentName(m.fromAgentId)}] ${m.content}`,
      timestamp: m.createdAt,
    }));
});

const handleSendMessage = async () => {
  const content = newMessageContent.value.trim();
  if (!selectedAgentId.value || !content) return;
  try {
    await workspace.sendUserMessage(selectedAgentId.value, content);
    newMessageContent.value = "";
  } catch (e) {
    message.error(`发送失败: ${e}`);
  }
};

// ============ 活动时间线（消息 + 日志合并） ============

const logKindLabels: Record<string, string> = {
  agent_created: "创建 Agent",
  tool_call: "调用工具",
  agent_proposal: "提议创建 Agent",
  sleep_request: "申请休眠",
  question: "提问",
  acceptance_review: "验收唤醒",
  scheduled_trigger: "⏰ 定时触发",
  meeting: "会议",
  agent_note: "Agent 备注",
  error: "错误",
  auto_paused: "自动暂停",
  paused: "手动暂停",
  resumed: "恢复运行",
  tool_approval: "工具调用审批",
};

const logTimelineType = (kind: string): "default" | "success" | "warning" | "error" | "info" => {
  if (kind === "error") return "error";
  if (kind === "agent_created" || kind === "resumed") return "success";
  if (kind === "sleep_request" || kind === "agent_proposal" || kind === "auto_paused" || kind === "paused" || kind === "tool_approval") return "warning";
  if (kind === "scheduled_trigger") return "info";
  return "info";
};

const logTitle = (l: WorkspaceLogEntry) => {
  const agent = l.agentId ? workspace.agentName(l.agentId) : "";
  return `${agent ? agent + " · " : ""}${logKindLabels[l.kind] ?? l.kind}`;
};

const timeline = computed(() => {
  const fromMessages = workspace.messages.map((m) => ({
    id: `msg-${m.id}`,
    createdAt: m.createdAt,
    type: "success" as const,
    title: `${workspace.agentName(m.fromAgentId)} → ${workspace.agentName(m.toAgentId)}`,
    content: m.content,
  }));
  const fromLogs = workspace.logs.map((l) => ({
    id: `log-${l.id}`,
    createdAt: l.createdAt,
    type: logTimelineType(l.kind),
    title: logTitle(l),
    content: l.content,
  }));
  return [...fromMessages, ...fromLogs].sort((a, b) => a.createdAt - b.createdAt);
});

const formatTime = (ts: number) => new Date(ts).toLocaleTimeString();

// ============ 定时任务（跳转到独立页面） ============

const openSchedulerPage = () => {
  const wid = workspace.currentWorkspace?.id;
  router.push({ name: "Scheduler", query: wid ? { workspace: wid } : {} });
};

// ============ 提醒：目标 Agent 没有存活的后台任务 ============

// Agent 的后台循环只存在于内存里；应用启动时会自动把每个工作组里的 Agent
// 重新挂回循环，正常情况下不该再看到这个提醒。真出现了多半是极端时序问题
// （比如启动过程中就有消息打进来），收到就弹一条警告，然后清空队列。
watch(
  () => workspace.inactiveAgentNotices.length,
  () => {
    while (workspace.inactiveAgentNotices.length > 0) {
      const notice = workspace.inactiveAgentNotices.shift();
      if (notice) {
        message.warning(
          `「${notice.agentName}」当前没有存活的后台任务，消息已发送但暂时不会有回复。通常重启一下应用就能自动恢复；如果重启后仍然这样，再考虑删除重建。`,
          { duration: 8000 }
        );
      }
    }
  }
);

// ============ 待处理事项：主 Agent 创建子 Agent 提议 ============

const proposalEdits = reactive<Record<string, CreateAgentRequest>>({});
watch(
  () => workspace.proposals,
  (list) => {
    for (const p of list) {
      if (!proposalEdits[p.proposalId]) {
        // 主 Agent 起草的职责说明后面附加默认协作行为准则（新 Agent 都是子
        // Agent），和手动创建路径保持一致；用户在卡片里仍然可改可删。
        proposalEdits[p.proposalId] = {
          ...p.draft,
          systemPrompt: p.draft.systemPrompt
            ? `${p.draft.systemPrompt}\n\n${AGENT_GUIDELINES_SUB}`
            : AGENT_GUIDELINES_SUB,
        };
      }
    }
  },
  { immediate: true, deep: true }
);

const handleApproveProposal = async (p: AgentProposalEvent) => {
  const edit = proposalEdits[p.proposalId];
  if (!edit.apiConfigId) {
    message.error("请先为这个新 Agent 选择 API 配置");
    return;
  }
  const config = settings.apiConfigs.find((c) => c.id === edit.apiConfigId);
  if (!config) {
    message.error("找不到所选的 API 配置");
    return;
  }
  try {
    await workspace.resolveProposal(p.proposalId, true, {
      ...edit,
      provider: config.provider,
      model: config.model,
      baseUrl: config.baseUrl,
    });
    delete proposalEdits[p.proposalId];
    message.success("已批准创建");
  } catch (e) {
    message.error(`处理失败: ${e}`);
  }
};

const handleRejectProposal = async (p: AgentProposalEvent) => {
  try {
    await workspace.resolveProposal(p.proposalId, false);
    delete proposalEdits[p.proposalId];
  } catch (e) {
    message.error(`处理失败: ${e}`);
  }
};

// ============ 待处理事项：休眠审批 / 提问 ============

const handleResolveSleep = async (requestId: string, approved: boolean) => {
  try {
    await workspace.resolveSleepRequest(requestId, approved);
  } catch (e) {
    message.error(`处理失败: ${e}`);
  }
};

const answerDrafts = ref<Record<string, string>>({});
const handleAnswer = async (questionId: string) => {
  const answer = answerDrafts.value[questionId]?.trim();
  if (!answer) {
    message.warning("请输入回答");
    return;
  }
  try {
    await workspace.resolveQuestion(questionId, answer);
    delete answerDrafts.value[questionId];
  } catch (e) {
    message.error(`提交失败: ${e}`);
  }
};

// ============ 初始化 ============

onMounted(async () => {
  // 事件监听只在第一次挂载时注册，且不会在组件卸载时取消 -- 主 Agent 的提议/
  // 休眠申请/提问都是一次性事件，没有"补查历史"的命令，错过了就再也找不回来，
  // 所以让它们在整个 App 生命周期里持续监听，而不是只在这个页面打开时才听。
  await workspace.initListeners();
  await workspace.listWorkspaces();
  await Promise.all([mcp.loadServers(), kb.loadKnowledgeBases(), skills.loadSkills()]);
});
</script>

<template>
  <div class="agent-team-view">
    <div class="page-header enter-up">
      <div class="header-left">
        <span class="eyebrow">Agents</span>
        <h1 class="page-title">协作团队</h1>
      </div>
      <n-space>
        <n-select
          :value="workspace.currentWorkspaceId"
          :options="workspaceOptions"
          placeholder="选择工作组"
          style="width: 220px"
          @update:value="handleSelectWorkspace"
        />
        <n-button type="primary" @click="showCreateWorkspaceModal = true">
          <template #icon><n-icon><Add /></n-icon></template>
          新建工作组
        </n-button>
        <n-popconfirm v-if="workspace.currentWorkspace" positive-text="删除" negative-text="取消" @positive-click="handleDeleteWorkspace">
          <template #trigger>
            <n-button type="error" ghost>
              <template #icon><n-icon><TrashOutline /></n-icon></template>
              删除当前工作组
            </n-button>
          </template>
          确定删除工作组「{{ workspace.currentWorkspace?.name }}」？里面所有 Agent 和消息记录都会被删除。
        </n-popconfirm>
      </n-space>
    </div>

    <n-empty v-if="!workspace.currentWorkspaceId" description="选择或新建一个工作组开始" style="margin-top: 120px" />

    <template v-else>
      <div v-if="workspace.proposals.length || workspace.sleepRequests.length || workspace.questions.length || workspace.toolApprovals.length" class="pending-section">
        <n-card
          v-for="p in workspace.proposals"
          :key="p.proposalId"
          class="pending-card"
          :title="`${p.proposedByAgentName} 提议创建新 Agent`"
        >
          <n-form label-placement="left" label-width="80px" size="small">
            <n-form-item label="名称">
              <n-input v-model:value="proposalEdits[p.proposalId].name" placeholder="给这个 Agent 起个名字" />
            </n-form-item>
            <n-form-item label="职责说明">
              <n-input v-model:value="proposalEdits[p.proposalId].systemPrompt" type="textarea" :rows="2" placeholder="这个 Agent 的职责说明..." />
            </n-form-item>
            <n-form-item label="提议的模型">
              <n-text depth="3">{{ p.draft.provider }} / {{ p.draft.model }}（仅供参考，实际以下方选择的 API 配置为准）</n-text>
            </n-form-item>
            <n-form-item label="API 配置" required>
              <n-select v-model:value="proposalEdits[p.proposalId].apiConfigId" :options="settings.apiConfigOptions" placeholder="选择要使用的 API 配置" />
            </n-form-item>
            <n-form-item label="MCP 工具" v-if="mcp.servers.length > 0">
              <n-checkbox-group v-model:value="proposalEdits[p.proposalId].mcpServerIds">
                <n-space vertical size="small">
                  <n-checkbox v-for="s in mcp.servers" :key="s.id" :value="s.id" :label="s.name" />
                </n-space>
              </n-checkbox-group>
            </n-form-item>
            <n-form-item label="知识库" v-if="kb.knowledgeBases.length > 0">
              <n-checkbox-group v-model:value="proposalEdits[p.proposalId].knowledgeBaseIds">
                <n-space vertical size="small">
                  <n-checkbox v-for="item in kb.knowledgeBases" :key="item.id" :value="item.id" :label="item.name" />
                </n-space>
              </n-checkbox-group>
            </n-form-item>
            <n-form-item label="Skill" v-if="skills.skills.length > 0">
              <n-checkbox-group v-model:value="proposalEdits[p.proposalId].activeSkillIds">
                <n-space vertical size="small">
                  <n-checkbox v-for="sk in skills.skills" :key="sk.id" :value="sk.id" :label="sk.name" />
                </n-space>
              </n-checkbox-group>
            </n-form-item>
          </n-form>
          <n-space justify="end">
            <n-button @click="handleRejectProposal(p)">拒绝</n-button>
            <n-button type="primary" @click="handleApproveProposal(p)">批准并创建</n-button>
          </n-space>
        </n-card>

        <n-card v-for="r in workspace.sleepRequests" :key="r.requestId" class="pending-card" :title="`${r.agentName} 申请休眠`">
          <p>原因：{{ r.reason || "未说明" }}</p>
          <n-space justify="end">
            <n-button @click="handleResolveSleep(r.requestId, false)">拒绝</n-button>
            <n-button type="primary" @click="handleResolveSleep(r.requestId, true)">批准休眠</n-button>
          </n-space>
        </n-card>

        <n-card v-for="q in workspace.questions" :key="q.questionId" class="pending-card" :title="`${q.agentName} 的提问`">
          <p>{{ q.question }}</p>
          <n-space>
            <n-input v-model:value="answerDrafts[q.questionId]" placeholder="输入回答..." style="width: 260px" @keyup.enter="handleAnswer(q.questionId)" />
            <n-button type="primary" @click="handleAnswer(q.questionId)">提交回答</n-button>
          </n-space>
        </n-card>

        <n-card v-for="t in workspace.toolApprovals" :key="t.approvalId" class="pending-card" :title="`${t.agentName} 请求执行工具「${t.toolName}」`">
          <pre class="tool-args">{{ JSON.stringify(t.arguments, null, 2) }}</pre>
          <n-space justify="end">
            <n-button @click="handleResolveToolApproval(t, false)">拒绝</n-button>
            <n-button type="primary" @click="handleResolveToolApproval(t, true)">批准执行</n-button>
          </n-space>
        </n-card>
      </div>

      <n-grid :cols="3" :x-gap="16" class="main-grid">
        <n-gi>
          <n-card title="Agent 列表" class="panel-card" :bordered="false">
            <template #header-extra>
              <n-space size="small">
                <n-button size="small" quaternary @click="openSchedulerPage" :disabled="!workspace.currentWorkspace" title="管理定时任务">
                  <template #icon><n-icon><AlarmOutline /></n-icon></template>
                </n-button>
                <n-button size="small" quaternary @click="showBroadcastModal = true" :disabled="workspace.activeAgents.length === 0" title="广播给所有 Agent">
                  <template #icon><n-icon><MegaphoneOutline /></n-icon></template>
                </n-button>
                <n-button size="small" type="primary" @click="openCreateAgentModal">
                  <template #icon><n-icon><Add /></n-icon></template>
                  添加 Agent
                </n-button>
              </n-space>
            </template>
            <n-list hoverable clickable class="agent-list">
              <n-list-item
                v-for="agent in workspace.activeAgents"
                :key="agent.id"
                :class="{ selected: agent.id === selectedAgentId }"
                @click="selectedAgentId = agent.id"
              >
                <n-thing>
                  <template #header>
                    {{ agent.name }}
                    <n-tag size="small" :type="agent.role === 'main' ? 'warning' : 'default'">{{ agent.role === "main" ? "主管" : "子Agent" }}</n-tag>
                  </template>
                  <template #description>{{ agent.provider }} / {{ agent.model }}</template>
                </n-thing>
                <template #suffix>
                  <n-space vertical align="end" size="small">
                    <n-tooltip v-if="agent.status === 'error'" trigger="hover">
                      <template #trigger>
                        <n-tag size="small" :type="statusMeta[agent.status].type">{{ statusMeta[agent.status].label }}</n-tag>
                      </template>
                      {{ workspace.latestErrorFor(agent.id) ?? "未知错误，请查看活动时间线" }}
                    </n-tooltip>
                    <n-tag v-else size="small" :type="statusMeta[agent.status].type">{{ statusMeta[agent.status].label }}</n-tag>
                    <n-space size="small">
                      <n-button v-if="agent.status === 'paused'" quaternary circle size="tiny" @click.stop="handleResumeAgent(agent.id)" title="恢复运行">
                        <template #icon><n-icon><PlayOutline /></n-icon></template>
                      </n-button>
                      <n-button v-else quaternary circle size="tiny" @click.stop="handlePauseAgent(agent.id)" title="暂停（紧急停止）">
                        <template #icon><n-icon><PauseOutline /></n-icon></template>
                      </n-button>
                      <n-button quaternary circle size="tiny" @click.stop="openEditAgentModal(agent)" title="编辑">
                        <template #icon><n-icon><PencilOutline /></n-icon></template>
                      </n-button>
                      <n-popconfirm positive-text="删除" negative-text="取消" @positive-click="handleDeleteAgent(agent.id)">
                        <template #trigger>
                          <n-button quaternary circle size="tiny" type="error" @click.stop>
                            <template #icon><n-icon><TrashOutline /></n-icon></template>
                          </n-button>
                        </template>
                        确定删除 Agent「{{ agent.name }}」？
                      </n-popconfirm>
                    </n-space>
                  </n-space>
                </template>
              </n-list-item>
            </n-list>
            <n-empty v-if="workspace.activeAgents.length === 0" description="还没有 Agent" size="small" />
          </n-card>
        </n-gi>

        <n-gi>
          <n-card title="对话" class="panel-card" :bordered="false">
            <template #header-extra>
              <n-button v-if="selectedAgent" size="small" quaternary @click="loadAgentTasks" title="刷新任务清单">
                <template #icon><n-icon><RefreshOutline /></n-icon></template>
              </n-button>
            </template>
            <n-empty v-if="!selectedAgent" description="选择一个 Agent 查看/发起对话" />
            <template v-else>
              <div v-if="agentTasks.length > 0" class="task-list">
                <div v-for="t in agentTasks" :key="t.id" class="task-item">
                  <n-checkbox :checked="t.done" @update:checked="(v: boolean) => handleToggleTask(t.id, v)" />
                  <span :class="{ 'task-done': t.done }">{{ t.content }}</span>
                </div>
              </div>
              <div class="message-scroll">
                <n-button v-if="workspace.hasMoreMessages" size="tiny" quaternary block @click="workspace.loadMoreMessages" class="load-more-btn">
                  加载更早的消息
                </n-button>
                <ChatMessage v-for="m in agentMessages" :key="m.id" :message="m" />
                <n-empty v-if="agentMessages.length === 0" description="还没有消息" size="small" />
                <p v-if="selectedAgent.status === 'running'" class="running-indicator">「{{ selectedAgent.name }}」正在处理…</p>
              </div>
              <div class="message-input-row">
                <n-input v-model:value="newMessageContent" placeholder="给这个 Agent 发消息..." @keyup.enter="handleSendMessage" />
                <n-button type="primary" @click="handleSendMessage">
                  <template #icon><n-icon><EnterOutline /></n-icon></template>
                </n-button>
              </div>
            </template>
          </n-card>
        </n-gi>

        <n-gi>
          <n-card title="活动时间线" class="panel-card" :bordered="false">
            <div class="timeline-scroll">
              <n-button v-if="workspace.hasMoreLogs" size="tiny" quaternary block @click="workspace.loadMoreLogs" class="load-more-btn">
                加载更早的记录
              </n-button>
              <n-timeline>
                <n-timeline-item
                  v-for="entry in timeline"
                  :key="entry.id"
                  :type="entry.type"
                  :title="entry.title"
                  :content="entry.content"
                  :time="formatTime(entry.createdAt)"
                />
              </n-timeline>
              <n-empty v-if="timeline.length === 0" description="还没有活动记录" size="small" />
            </div>
          </n-card>
        </n-gi>
      </n-grid>
    </template>

    <!-- 新建工作组 -->
    <n-modal v-model:show="showCreateWorkspaceModal" preset="card" title="新建工作组" style="width: 480px">
      <n-form label-placement="left" label-width="110px">
        <n-form-item label="名称" required>
          <n-input v-model:value="wsForm.name" placeholder="例如：产品文案小组" />
        </n-form-item>
        <n-form-item label="描述">
          <n-input v-model:value="wsForm.description" type="textarea" :rows="2" placeholder="简要描述这个工作组的用途（可选）" />
        </n-form-item>
        <n-form-item label="Agent 数量上限">
          <n-input-number v-model:value="wsForm.maxAgents" :min="1" :max="20" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateWorkspaceModal = false">取消</n-button>
          <n-button type="primary" @click="handleCreateWorkspace">创建</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 添加 Agent -->
    <n-modal v-model:show="showCreateAgentModal" preset="card" title="添加 Agent" style="width: 600px; max-height: 85vh" :content-style="{ overflowY: 'auto' }">
      <n-form label-placement="left" label-width="100px">
        <n-form-item label="名称" required>
          <n-input v-model:value="agentForm.name" placeholder="给这个 Agent 起个名字" />
        </n-form-item>
        <n-form-item label="角色">
          <n-radio-group v-model:value="agentForm.role">
            <n-radio value="main">主管 Agent</n-radio>
            <n-radio value="sub">子 Agent</n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item label="API 配置" required>
          <n-select v-model:value="agentForm.apiConfigId" :options="settings.apiConfigOptions" placeholder="选择已保存的 API 配置" />
        </n-form-item>
        <n-form-item label="系统提示词">
          <n-input v-model:value="agentForm.systemPrompt" type="textarea" :rows="4" placeholder="这个 Agent 的职责说明..." />
        </n-form-item>
        <n-form-item label="思考模式">
          <n-space align="center">
            <n-switch v-model:value="agentForm.enableThinking" />
            <n-text depth="3" style="font-size: 12px">开启后模型会先深度思考再回复，更费时间和 token，复杂任务再开</n-text>
          </n-space>
        </n-form-item>
        <n-form-item label="MCP 工具" v-if="mcp.servers.length > 0">
          <n-checkbox-group v-model:value="agentForm.mcpServerIds">
            <n-space vertical size="small">
              <n-checkbox v-for="s in mcp.servers" :key="s.id" :value="s.id" :label="s.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
        <n-form-item v-if="mcp.servers.length > 0" label="高风险工具需批准">
          <n-space align="center">
            <n-switch v-model:value="agentForm.requireToolApproval" />
            <n-text depth="3" style="font-size: 12px">开启后（默认），删除/写入/执行命令等高风险工具调用前需要你批准，其余工具照常自动放行；关闭则全部自动放行，风险自担</n-text>
          </n-space>
        </n-form-item>
        <n-form-item label="知识库" v-if="kb.knowledgeBases.length > 0">
          <n-checkbox-group v-model:value="agentForm.knowledgeBaseIds">
            <n-space vertical size="small">
              <n-checkbox v-for="item in kb.knowledgeBases" :key="item.id" :value="item.id" :label="item.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
        <template v-if="agentForm.knowledgeBaseIds.length > 0">
          <n-form-item label="检索 top_k">
            <n-input-number v-model:value="agentForm.ragTopK" :min="1" :max="20" />
          </n-form-item>
          <n-form-item label="检索模式">
            <n-select v-model:value="agentForm.ragRetrievalMode" :options="retrievalModeOptions" />
          </n-form-item>
          <n-form-item label="Reranker" v-if="settings.rerankerApiConfigOptions.length > 0">
            <n-select v-model:value="agentForm.ragRerankerConfigId" :options="settings.rerankerApiConfigOptions" clearable placeholder="不启用精排（可选）" />
          </n-form-item>
          <n-form-item label="精排保留条数" v-if="agentForm.ragRerankerConfigId">
            <n-input-number v-model:value="agentForm.ragRerankTopN" :min="1" :max="agentForm.ragTopK" placeholder="默认等于 top_k" />
          </n-form-item>
        </template>
        <n-form-item label="Skill" v-if="skills.skills.length > 0">
          <n-checkbox-group v-model:value="agentForm.activeSkillIds">
            <n-space vertical size="small">
              <n-checkbox v-for="sk in skills.skills" :key="sk.id" :value="sk.id" :label="sk.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateAgentModal = false">取消</n-button>
          <n-button type="primary" @click="handleCreateAgent">添加</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 编辑 Agent -->
    <n-modal v-model:show="showEditAgentModal" preset="card" title="编辑 Agent" style="width: 600px; max-height: 85vh" :content-style="{ overflowY: 'auto' }">
      <n-form v-if="editAgentForm" label-placement="left" label-width="100px">
        <n-form-item label="名称" required>
          <n-input v-model:value="editAgentForm.name" placeholder="给这个 Agent 起个名字" />
        </n-form-item>
        <n-form-item label="API 配置" required>
          <n-select v-model:value="editAgentForm.apiConfigId" :options="settings.apiConfigOptions" placeholder="选择已保存的 API 配置" />
        </n-form-item>
        <n-form-item label="系统提示词">
          <n-input v-model:value="editAgentForm.systemPrompt" type="textarea" :rows="4" placeholder="这个 Agent 的职责说明..." />
        </n-form-item>
        <n-form-item label="思考模式">
          <n-space align="center">
            <n-switch v-model:value="editAgentForm.enableThinking" />
            <n-text depth="3" style="font-size: 12px">开启后模型会先深度思考再回复，更费时间和 token，复杂任务再开</n-text>
          </n-space>
        </n-form-item>
        <n-form-item label="MCP 工具" v-if="mcp.servers.length > 0">
          <n-checkbox-group v-model:value="editAgentForm.mcpServerIds">
            <n-space vertical size="small">
              <n-checkbox v-for="s in mcp.servers" :key="s.id" :value="s.id" :label="s.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
        <n-form-item v-if="mcp.servers.length > 0" label="高风险工具需批准">
          <n-space align="center">
            <n-switch v-model:value="editAgentForm.requireToolApproval" />
            <n-text depth="3" style="font-size: 12px">开启后（默认），删除/写入/执行命令等高风险工具调用前需要你批准，其余工具照常自动放行；关闭则全部自动放行，风险自担</n-text>
          </n-space>
        </n-form-item>
        <n-form-item label="知识库" v-if="kb.knowledgeBases.length > 0">
          <n-checkbox-group v-model:value="editAgentForm.knowledgeBaseIds">
            <n-space vertical size="small">
              <n-checkbox v-for="item in kb.knowledgeBases" :key="item.id" :value="item.id" :label="item.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
        <template v-if="editAgentForm.knowledgeBaseIds.length > 0">
          <n-form-item label="检索 top_k">
            <n-input-number v-model:value="editAgentForm.ragTopK" :min="1" :max="20" />
          </n-form-item>
          <n-form-item label="检索模式">
            <n-select v-model:value="editAgentForm.ragRetrievalMode" :options="retrievalModeOptions" />
          </n-form-item>
          <n-form-item label="Reranker" v-if="settings.rerankerApiConfigOptions.length > 0">
            <n-select v-model:value="editAgentForm.ragRerankerConfigId" :options="settings.rerankerApiConfigOptions" clearable placeholder="不启用精排（可选）" />
          </n-form-item>
          <n-form-item label="精排保留条数" v-if="editAgentForm.ragRerankerConfigId">
            <n-input-number v-model:value="editAgentForm.ragRerankTopN" :min="1" :max="editAgentForm.ragTopK" placeholder="默认等于 top_k" />
          </n-form-item>
        </template>
        <n-form-item label="Skill" v-if="skills.skills.length > 0">
          <n-checkbox-group v-model:value="editAgentForm.activeSkillIds">
            <n-space vertical size="small">
              <n-checkbox v-for="sk in skills.skills" :key="sk.id" :value="sk.id" :label="sk.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showEditAgentModal = false">取消</n-button>
          <n-button type="primary" @click="handleUpdateAgent">保存</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 广播消息 -->
    <n-modal v-model:show="showBroadcastModal" preset="card" title="广播给所有 Agent" style="width: 480px">
      <n-input v-model:value="broadcastContent" type="textarea" :rows="3" placeholder="这条消息会发给工作组里的每一个 Agent..." />
      <template #footer>
        <n-space justify="end">
          <n-button @click="showBroadcastModal = false">取消</n-button>
          <n-button type="primary" @click="handleBroadcastSend">发送</n-button>
        </n-space>
      </template>
    </n-modal>

  </div>
</template>

<style scoped lang="scss">
.agent-team-view {
  height: 100%;
  padding: 3rem 2.5rem 8rem;
  overflow-y: auto;
  background: $bg;
}

.page-header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  margin-bottom: 3rem;
  padding-bottom: 1.5rem;
  border-bottom: $border;
}

.header-left {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.75rem;
}

.page-title {
  font-family: $font-serif;
  font-size: 2rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

.pending-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 20px;
}

.pending-card {
  border-left: 2px solid $ink;
}

.main-grid {
  align-items: stretch;
}

.panel-card {
  height: calc(100vh - 22rem);
  min-height: 26rem;
  display: flex;
  flex-direction: column;

  :deep(.n-card__content) {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  // 卡片标题不能换行——header-extra 里放的图标/按钮一多，flex 布局会把标题
  // 挤到极窄，逐字换行成竖排。标题优先保留完整宽度，header-extra 自己收窄。
  :deep(.n-card-header__main) {
    white-space: nowrap;
    flex-shrink: 0;
  }
  :deep(.n-card-header__extra) {
    min-width: 0;
  }
}

.load-more-btn {
  margin-bottom: 8px;
  color: $ink-faint;
}

.running-indicator {
  padding: 4px 0;
  font-size: $label-size;
  color: $ink-faint;
  transition: opacity $duration $ease;
}

.tool-args {
  font-family: $font-mono;
  font-size: 0.75rem;
  color: $ink-soft;
  background: $surface;
  padding: 8px;
  margin: 0 0 8px;
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-word;
}

.task-list {
  border-bottom: $border-faint;
  padding-bottom: 8px;
  margin-bottom: 8px;
  max-height: 8rem;
  overflow-y: auto;
}

.task-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 0;
  font-size: 0.85rem;
}

.task-done {
  color: $ink-faint;
  text-decoration: line-through;
}

.agent-list {
  overflow-y: auto;
}

.agent-list :deep(.n-list-item.selected) {
  background: $surface;
}

.message-scroll {
  flex: 1;
  overflow-y: auto;
  margin-bottom: 12px;
}

.message-input-row {
  display: flex;
  gap: 8px;
}

.timeline-scroll {
  flex: 1;
  overflow-y: auto;
}
</style>
