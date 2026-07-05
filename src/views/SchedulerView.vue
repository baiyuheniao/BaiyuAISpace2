<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, computed, onMounted, onActivated, watch } from "vue";
import { useRoute } from "vue-router";
import {
  NButton, NIcon, NCard, NList, NListItem, NThing, NTag, NEmpty,
  NModal, NForm, NFormItem, NInput, NInputNumber, NSelect,
  NRadioGroup, NRadio, NSwitch, NDatePicker, NTimePicker,
  NSpace, NPopconfirm, useMessage,
} from "naive-ui";
import { Add, TrashOutline } from "@vicons/ionicons5";

import { useSchedulerStore, type CreateScheduleRequest, type Schedule } from "@/stores/scheduler";
import { useWorkspaceStore, type WorkspaceAgent } from "@/stores/workspace";
import { invoke } from "@tauri-apps/api/core";

const route = useRoute();
const scheduler = useSchedulerStore();
const workspace = useWorkspaceStore();
const message = useMessage();

// ============ 工作组 + Agent 列表（用于表单选项） ============

const workspaceOptions = computed(() => [
  { label: "全部（不绑定工作组）", value: "__none__" },
  ...workspace.workspaces.map((w) => ({ label: w.name, value: w.id })),
]);

const filterWorkspaceId = ref<string | null>(null);
const formWorkspaceId = ref<string>("__none__");
const formAgents = ref<WorkspaceAgent[]>([]);

const agentOptions = computed(() => [
  { label: "广播给所有 Agent", value: "__all__" },
  ...formAgents.value.map((a) => ({ label: a.name, value: a.id })),
]);

watch(formWorkspaceId, async (wid) => {
  if (!wid || wid === "__none__") { formAgents.value = []; return; }
  try {
    formAgents.value = await invoke<WorkspaceAgent[]>("workspace_list_agents", { workspaceId: wid });
  } catch { formAgents.value = []; }
});

// ============ 加载调度列表 ============

const loadSchedules = async () => {
  await scheduler.loadSchedules(filterWorkspaceId.value ?? undefined);
};

const syncFromRoute = async () => {
  await workspace.listWorkspaces();
  const wid = route.query.workspace as string | undefined;
  if (wid && wid !== filterWorkspaceId.value) {
    filterWorkspaceId.value = wid;
    formWorkspaceId.value = wid;
  }
  await loadSchedules();
};

// onMounted: 首次加载；onActivated: keep-alive 重新激活时
onMounted(syncFromRoute);
onActivated(syncFromRoute);

watch(filterWorkspaceId, loadSchedules);

// ============ 名称解析辅助 ============

const workspaceName = (id: string | null) => {
  if (!id) return "无";
  return workspace.workspaces.find((w) => w.id === id)?.name ?? id.slice(0, 8);
};

const formatNextRun = (ts: number) => new Date(ts).toLocaleString();
const formatLastRun = (ts: number | null) => (ts ? new Date(ts).toLocaleString() : "从未");

const kindLabel: Record<string, string> = {
  once: "单次",
  interval: "间隔",
  daily: "每天",
  weekly: "每周",
};

const kindTagType = (kind: string): "default" | "info" | "success" | "warning" => {
  if (kind === "once") return "default";
  if (kind === "interval") return "info";
  if (kind === "daily") return "success";
  return "warning";
};

// ============ 新建弹窗 ============

const showCreateModal = ref(false);
const scheduleKindOptions = [
  { label: "单次", value: "once" },
  { label: "间隔", value: "interval" },
  { label: "每天", value: "daily" },
  { label: "每周", value: "weekly" },
];
const weekdayOptions = [
  { label: "周一", value: 0 }, { label: "周二", value: 1 },
  { label: "周三", value: 2 }, { label: "周四", value: 3 },
  { label: "周五", value: 4 }, { label: "周六", value: 5 },
  { label: "周日", value: 6 },
];

const emptyForm = (): CreateScheduleRequest => ({
  name: "",
  workspaceId: filterWorkspaceId.value,
  targetAgentId: null,
  message: "",
  kind: "interval",
  intervalMinutes: 60,
  atTime: null,
  weekday: null,
  onceAt: null,
});
const form = ref<CreateScheduleRequest>(emptyForm());

const openCreateModal = () => {
  form.value = emptyForm();
  form.value.workspaceId = filterWorkspaceId.value;
  formWorkspaceId.value = filterWorkspaceId.value ?? "__none__";
  showCreateModal.value = true;
};

// Keep form.workspaceId in sync with the workspace picker inside the modal
watch(formWorkspaceId, (v) => {
  form.value.workspaceId = v === "__none__" ? null : v;
  form.value.targetAgentId = null;
});

const handleCreate = async () => {
  if (!form.value.name.trim()) { message.error("请填写任务名称"); return; }
  if (!form.value.message.trim()) { message.error("请填写触发消息"); return; }
  try {
    await scheduler.createSchedule(form.value);
    showCreateModal.value = false;
    message.success("定时任务已创建");
    await loadSchedules();
  } catch (e) {
    message.error(`创建失败: ${e}`);
  }
};

// ============ 操作 ============

const handleToggle = async (s: Schedule) => {
  try {
    await scheduler.toggleSchedule(s.id);
  } catch (e) {
    message.error(`操作失败: ${e}`);
  }
};

const handleDelete = async (id: string) => {
  try {
    await scheduler.deleteSchedule(id);
    message.success("已删除");
  } catch (e) {
    message.error(`删除失败: ${e}`);
  }
};
</script>

<template>
  <div class="scheduler-view">
    <div class="page-header enter-up">
      <div class="page-title-row">
        <span class="eyebrow">Cron</span>
        <h1 class="page-title">定时任务</h1>
      </div>
      <n-space align="center">
        <n-select
          v-model:value="filterWorkspaceId"
          :options="(workspace.workspaces.map(w => ({ label: w.name, value: w.id })) as any)"
          placeholder="筛选工作组"
          clearable
          style="width: 180px"
        />
        <n-button type="primary" @click="openCreateModal">
          <template #icon><n-icon><Add /></n-icon></template>
          新建定时任务
        </n-button>
      </n-space>
    </div>

    <n-card :bordered="false" class="list-card">
      <n-empty v-if="scheduler.schedules.length === 0" description="还没有定时任务" />
      <n-list v-else hoverable>
        <n-list-item v-for="s in scheduler.schedules" :key="s.id">
          <n-thing>
            <template #header>
              <n-space align="center" size="small">
                <span>{{ s.name }}</span>
                <n-tag size="small" :type="kindTagType(s.kind)">{{ kindLabel[s.kind] }}</n-tag>
                <n-tag v-if="!s.enabled" size="small" type="default">已禁用</n-tag>
              </n-space>
            </template>
            <template #description>
              <n-space size="small" style="margin-top: 4px; flex-wrap: wrap;">
                <span v-if="s.workspaceId" style="opacity: 0.7">工作组：{{ workspaceName(s.workspaceId) }}</span>
                <span v-if="s.kind === 'interval'">每 {{ s.intervalMinutes }} 分钟</span>
                <span v-else-if="s.kind === 'daily'">每天 {{ s.atTime }}</span>
                <span v-else-if="s.kind === 'weekly'">每周{{ ['一','二','三','四','五','六','日'][s.weekday ?? 0] }} {{ s.atTime }}</span>
                <span v-else-if="s.kind === 'once'">单次</span>
                <span>·</span>
                <span>下次：{{ s.enabled ? formatNextRun(s.nextRunAt) : '—' }}</span>
                <span>·</span>
                <span style="opacity: 0.7">上次：{{ formatLastRun(s.lastRunAt) }}</span>
              </n-space>
              <div style="margin-top: 6px; opacity: 0.8; font-size: 12px;">{{ s.message }}</div>
            </template>
          </n-thing>
          <template #suffix>
            <n-space align="center" size="small">
              <n-switch :value="s.enabled" size="small" @update:value="handleToggle(s)" />
              <n-popconfirm @positive-click="handleDelete(s.id)">
                <template #trigger>
                  <n-button quaternary circle size="small" type="error">
                    <template #icon><n-icon><TrashOutline /></n-icon></template>
                  </n-button>
                </template>
                确认删除定时任务「{{ s.name }}」？
              </n-popconfirm>
            </n-space>
          </template>
        </n-list-item>
      </n-list>
    </n-card>

    <!-- 新建弹窗 -->
    <n-modal v-model:show="showCreateModal" preset="card" title="新建定时任务" style="width: 520px">
      <n-form label-placement="left" label-width="90">
        <n-form-item label="任务名称">
          <n-input v-model:value="form.name" placeholder="例：每日进展提醒" />
        </n-form-item>
        <n-form-item label="绑定工作组">
          <n-select v-model:value="formWorkspaceId" :options="workspaceOptions" />
        </n-form-item>
        <n-form-item v-if="formWorkspaceId !== '__none__'" label="目标 Agent">
          <n-select v-model:value="form.targetAgentId" :options="agentOptions" placeholder="广播给所有 Agent" clearable />
        </n-form-item>
        <n-form-item label="类型">
          <n-radio-group v-model:value="form.kind">
            <n-radio v-for="opt in scheduleKindOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item v-if="form.kind === 'interval'" label="间隔（分钟）">
          <n-input-number v-model:value="form.intervalMinutes" :min="1" style="width: 140px" />
        </n-form-item>
        <n-form-item v-if="form.kind === 'daily' || form.kind === 'weekly'" label="触发时间">
          <n-time-picker v-model:formatted-value="form.atTime" format="HH:mm" value-format="HH:mm" style="width: 140px" />
        </n-form-item>
        <n-form-item v-if="form.kind === 'weekly'" label="星期">
          <n-select v-model:value="form.weekday" :options="weekdayOptions" style="width: 140px" />
        </n-form-item>
        <n-form-item v-if="form.kind === 'once'" label="触发时间">
          <n-date-picker v-model:value="form.onceAt" type="datetime" />
        </n-form-item>
        <n-form-item label="触发消息">
          <n-input v-model:value="form.message" type="textarea" :rows="3" placeholder="触发时发送给 Agent 的消息" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateModal = false">取消</n-button>
          <n-button type="primary" @click="handleCreate">创建</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<style scoped lang="scss">
.scheduler-view {
  height: 100%;
  padding: 3rem 2.5rem 8rem;
  box-sizing: border-box;
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

.page-title-row {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.75rem;
}

.page-title {
  margin: 0;
  font-family: $font-serif;
  font-size: 2rem;
  font-weight: 700;
  line-height: $leading-display;
}

.list-card {
  border: $border-soft;

  :deep(.n-card__content) {
    padding: 0;
  }
}
</style>
