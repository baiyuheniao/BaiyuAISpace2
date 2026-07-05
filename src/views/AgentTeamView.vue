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
  NTag, NTimeline, NTimelineItem, NPopconfirm, useMessage,
} from "naive-ui";
import { Add, TrashOutline, EnterOutline, AlarmOutline } from "@vicons/ionicons5";

import ChatMessage from "@/components/ChatMessage.vue";
import { useWorkspaceStore, type AgentProposalEvent, type AgentStatus, type CreateAgentRequest, type WorkspaceLogEntry } from "@/stores/workspace";
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
const emptyAgentForm = (): CreateAgentRequest => ({
  workspaceId: workspace.currentWorkspaceId ?? "",
  name: "",
  role: "sub",
  provider: "",
  model: "",
  baseUrl: "",
  apiConfigId: "",
  systemPrompt: "",
  mcpServerIds: [],
  knowledgeBaseIds: [],
  activeSkillIds: [],
});
const agentForm = ref<CreateAgentRequest>(emptyAgentForm());

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
    });
    showCreateAgentModal.value = false;
    message.success("Agent 已创建");
  } catch (e) {
    message.error(`创建失败: ${e}`);
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

const statusMeta: Record<AgentStatus, { label: string; type: "default" | "success" | "warning" | "error" | "info" }> = {
  idle: { label: "空闲", type: "default" },
  running: { label: "运行中", type: "success" },
  waiting_approval: { label: "等待审批", type: "warning" },
  waiting_answer: { label: "等待回答", type: "warning" },
  sleeping: { label: "已休眠", type: "info" },
  meeting: { label: "会议中", type: "info" },
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
  error: "错误",
};

const logTimelineType = (kind: string): "default" | "success" | "warning" | "error" | "info" => {
  if (kind === "error") return "error";
  if (kind === "agent_created") return "success";
  if (kind === "sleep_request" || kind === "agent_proposal") return "warning";
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

// Agent 的后台循环只存在于内存里，应用重启后不会自动恢复；发消息给这样的
// Agent 不会报错，只是永远没有回复。收到提醒就弹一条警告，然后清空队列。
watch(
  () => workspace.inactiveAgentNotices.length,
  () => {
    while (workspace.inactiveAgentNotices.length > 0) {
      const notice = workspace.inactiveAgentNotices.shift();
      if (notice) {
        message.warning(
          `「${notice.agentName}」当前不在运行状态，消息已发送但暂时不会有回复。请重新添加该 Agent 以恢复其工作能力。`,
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
        proposalEdits[p.proposalId] = { ...p.draft };
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
        <n-popconfirm v-if="workspace.currentWorkspace" @positive-click="handleDeleteWorkspace">
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
      <div v-if="workspace.proposals.length || workspace.sleepRequests.length || workspace.questions.length" class="pending-section">
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
      </div>

      <n-grid :cols="3" :x-gap="16" class="main-grid">
        <n-gi>
          <n-card title="Agent 列表" class="panel-card" :bordered="false">
            <template #header-extra>
              <n-space size="small">
                <n-button size="small" quaternary @click="openSchedulerPage" :disabled="!workspace.currentWorkspace" title="管理定时任务">
                  <template #icon><n-icon><AlarmOutline /></n-icon></template>
                </n-button>
                <n-button size="small" type="primary" @click="openCreateAgentModal">
                  <template #icon><n-icon><Add /></n-icon></template>
                  添加 Agent
                </n-button>
              </n-space>
            </template>
            <n-list hoverable clickable class="agent-list">
              <n-list-item
                v-for="agent in workspace.agents"
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
                    <n-tag size="small" :type="statusMeta[agent.status].type">{{ statusMeta[agent.status].label }}</n-tag>
                    <n-button quaternary circle size="tiny" type="error" @click.stop="handleDeleteAgent(agent.id)">
                      <template #icon><n-icon><TrashOutline /></n-icon></template>
                    </n-button>
                  </n-space>
                </template>
              </n-list-item>
            </n-list>
            <n-empty v-if="workspace.agents.length === 0" description="还没有 Agent" size="small" />
          </n-card>
        </n-gi>

        <n-gi>
          <n-card title="对话" class="panel-card" :bordered="false">
            <n-empty v-if="!selectedAgent" description="选择一个 Agent 查看/发起对话" />
            <template v-else>
              <div class="message-scroll">
                <ChatMessage v-for="m in agentMessages" :key="m.id" :message="m" />
                <n-empty v-if="agentMessages.length === 0" description="还没有消息" size="small" />
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
        <n-form-item label="MCP 工具" v-if="mcp.servers.length > 0">
          <n-checkbox-group v-model:value="agentForm.mcpServerIds">
            <n-space vertical size="small">
              <n-checkbox v-for="s in mcp.servers" :key="s.id" :value="s.id" :label="s.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
        <n-form-item label="知识库" v-if="kb.knowledgeBases.length > 0">
          <n-checkbox-group v-model:value="agentForm.knowledgeBaseIds">
            <n-space vertical size="small">
              <n-checkbox v-for="item in kb.knowledgeBases" :key="item.id" :value="item.id" :label="item.name" />
            </n-space>
          </n-checkbox-group>
        </n-form-item>
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
  height: 640px;
  display: flex;
  flex-direction: column;

  :deep(.n-card__content) {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
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
