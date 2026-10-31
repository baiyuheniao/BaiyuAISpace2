<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, computed, onMounted, onActivated, onBeforeUnmount, watch } from "vue";
import { useRoute } from "vue-router";
import {
  NButton, NIcon, NCard, NList, NListItem, NThing, NTag, NEmpty,
  NModal, NForm, NFormItem, NInput, NInputNumber, NSelect,
  NRadioGroup, NRadio, NSwitch, NDatePicker, NTimePicker,
  NSpace, NPopconfirm,
} from "naive-ui";
import { Add, TrashOutline } from "@vicons/ionicons5";

import { useMessage } from "@/composables/useNotify";
import {
  useSchedulerStore, MAX_CONSECUTIVE_CHECK_ERRORS,
  type CreateScheduleRequest, type Schedule, type ScheduleCheckedEvent, type TestScriptResult, type CheckShell,
} from "@/stores/scheduler";
import { useWorkspaceStore, type WorkspaceAgent } from "@/stores/workspace";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

// 条件任务每检测一次，后端就发一个事件：刷新列表里的检测状态；被自动停用时提示用户。
let unlistenChecked: UnlistenFn | null = null;
let unlistenChanged: UnlistenFn | null = null;
onMounted(async () => {
  // Agent 通过工具创建/删除/启停了任务：刷新列表
  unlistenChanged = await listen("scheduler://changed", () => { void loadSchedules(); });
  unlistenChecked = await listen<ScheduleCheckedEvent>("scheduler://checked", async (event) => {
    await loadSchedules();
    if (event.payload.autoDisabled) {
      message.warning(`条件任务「${event.payload.scheduleName}」连续检测出错，已自动停用`);
    }
  });
});
onBeforeUnmount(() => {
  unlistenChecked?.(); unlistenChecked = null;
  unlistenChanged?.(); unlistenChanged = null;
});

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
  condition: "条件",
};

const checkStatusLabel: Record<string, string> = {
  met: "条件成立",
  not_met: "条件不成立",
  error: "检测出错",
};
const checkShellLabel: Record<string, string> = {
  powershell: "PowerShell",
  cmd: "cmd",
  sh: "sh",
  python: "Python",
};

/** 条件任务因连续出错被自动停用（而不是用户手动关闭）。 */
const isAutoDisabled = (s: Schedule) =>
  s.kind === "condition" && !s.enabled && s.consecutiveErrors >= MAX_CONSECUTIVE_CHECK_ERRORS;

const kindTagType = (kind: string): "default" | "info" | "success" | "warning" | "error" => {
  if (kind === "once") return "default";
  if (kind === "interval") return "info";
  if (kind === "daily") return "success";
  if (kind === "condition") return "error";
  return "warning";
};

// ============ 新建弹窗 ============

const showCreateModal = ref(false);
const scheduleKindOptions = [
  { label: "单次", value: "once" },
  { label: "间隔", value: "interval" },
  { label: "每天", value: "daily" },
  { label: "每周", value: "weekly" },
  { label: "条件", value: "condition" },
];
const isWindows = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);
const checkShellOptions = [
  { label: "PowerShell", value: "powershell" },
  { label: "cmd（仅 Windows）", value: "cmd" },
  { label: "sh（macOS / Linux）", value: "sh" },
  { label: "Python", value: "python" },
];
const triggerModeOptions = [
  { label: "条件由不成立变成立时，触发一次", value: "edge" },
  { label: "条件成立期间，按冷却时间重复触发", value: "repeat" },
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
  checkScript: null,
  checkShell: (isWindows ? "powershell" : "sh") as CheckShell,
  triggerMode: "edge",
  cooldownMinutes: 60,
});
const form = ref<CreateScheduleRequest>(emptyForm());

const openCreateModal = () => {
  form.value = emptyForm();
  form.value.workspaceId = filterWorkspaceId.value;
  formWorkspaceId.value = filterWorkspaceId.value ?? "__none__";
  testResult.value = null;
  showCreateModal.value = true;
};

// 条件任务：保存前试运行检测脚本
const testResult = ref<TestScriptResult | null>(null);
const testing = ref(false);
const handleTestScript = async () => {
  const script = (form.value.checkScript ?? "").trim();
  if (!script) { message.error("请先填写检测脚本"); return; }
  testing.value = true;
  testResult.value = null;
  try {
    testResult.value = await scheduler.testScript(form.value.checkShell ?? "sh", script);
  } catch (e) {
    message.error(`试运行失败: ${e}`);
  } finally {
    testing.value = false;
  }
};

// 条件任务的检测间隔默认 5 分钟，间隔任务默认 60 分钟：切换类型时跟着换（用户改过就不动）
watch(() => form.value.kind, (kind, previous) => {
  if (kind === "condition" && form.value.intervalMinutes === 60) form.value.intervalMinutes = 5;
  else if (previous === "condition" && kind === "interval" && form.value.intervalMinutes === 5) form.value.intervalMinutes = 60;
});

// Keep form.workspaceId in sync with the workspace picker inside the modal
watch(formWorkspaceId, (v) => {
  form.value.workspaceId = v === "__none__" ? null : v;
  form.value.targetAgentId = null;
});

const handleCreate = async () => {
  if (!form.value.name.trim()) { message.error("请填写任务名称"); return; }
  if (!form.value.message.trim()) { message.error("请填写触发消息"); return; }
  if (form.value.kind === "condition") {
    if (!form.value.workspaceId) { message.error("条件触发任务需要绑定工作组"); return; }
    if (!(form.value.checkScript ?? "").trim()) { message.error("请填写检测脚本"); return; }
  }
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
                <n-tag v-if="s.createdBy?.startsWith('agent:')" size="small" type="default">Agent 创建：{{ s.createdBy.split(':').slice(2).join(':') }}</n-tag>
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
                <span v-else-if="s.kind === 'condition'">
                  每 {{ s.intervalMinutes }} 分钟检测（{{ checkShellLabel[s.checkShell ?? 'sh'] }}，{{ s.triggerMode === 'repeat' ? `成立期间每 ${s.cooldownMinutes} 分钟重复触发` : '成立时触发一次' }}）
                </span>
                <span>·</span>
                <span>{{ s.kind === 'condition' ? '下次检测' : '下次' }}：{{ s.enabled ? formatNextRun(s.nextRunAt) : '—' }}</span>
                <span>·</span>
                <span style="opacity: 0.7">上次：{{ formatLastRun(s.lastRunAt) }}</span>
              </n-space>
              <div style="margin-top: 6px; opacity: 0.8; font-size: 12px;">{{ s.message }}</div>
              <div v-if="s.kind === 'condition'" class="check-status">
                <n-space size="small" align="center">
                  <span class="check-label">上次检测：{{ s.lastCheckAt ? formatNextRun(s.lastCheckAt) : '尚未检测' }}</span>
                  <n-tag v-if="s.lastCheckStatus" size="small" :type="s.lastCheckStatus === 'met' ? 'success' : s.lastCheckStatus === 'error' ? 'error' : 'default'">
                    {{ checkStatusLabel[s.lastCheckStatus] }}
                  </n-tag>
                  <span v-if="s.consecutiveErrors > 0" class="check-label">连续出错 {{ s.consecutiveErrors }} 次</span>
                </n-space>
                <pre v-if="s.lastCheckOutput" class="check-output">{{ s.lastCheckOutput }}</pre>
                <div v-if="isAutoDisabled(s)" class="check-warning">连续检测出错，已自动停用。修好脚本后重新启用即可。</div>
              </div>
            </template>
          </n-thing>
          <template #suffix>
            <n-space align="center" size="small">
              <n-switch :value="s.enabled" size="small" @update:value="handleToggle(s)" />
              <n-popconfirm positive-text="删除" negative-text="取消" @positive-click="handleDelete(s.id)">
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
    <n-modal v-model:show="showCreateModal" preset="card" title="新建定时任务" style="width: 620px">
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
        <template v-if="form.kind === 'condition'">
          <n-form-item label="检测间隔">
            <n-input-number v-model:value="form.intervalMinutes" :min="1" style="width: 140px">
              <template #suffix>分钟</template>
            </n-input-number>
          </n-form-item>
          <n-form-item label="脚本解释器">
            <n-select v-model:value="form.checkShell" :options="checkShellOptions" style="width: 220px" />
          </n-form-item>
          <n-form-item label="检测脚本">
            <div class="script-field">
              <n-input
                v-model:value="form.checkScript"
                type="textarea"
                :rows="6"
                class="script-input"
                placeholder="例：检查收件目录里有没有新文件，有就 exit 0，没有就 exit 1"
              />
              <div class="field-hint">退出码为 0 表示条件成立，非 0 表示不成立；脚本输出会附在触发消息后面，单次最长运行 30 秒。</div>
              <div class="script-actions">
                <n-button size="small" :loading="testing" @click="handleTestScript">试运行</n-button>
              </div>
              <div v-if="testResult" class="test-result">
                <n-space size="small" align="center">
                  <n-tag size="small" :type="testResult.status === 'met' ? 'success' : testResult.status === 'error' ? 'error' : 'default'">
                    {{ checkStatusLabel[testResult.status] }}
                  </n-tag>
                  <span class="check-label">
                    <template v-if="testResult.exitCode !== null">退出码 {{ testResult.exitCode }} · </template>耗时 {{ testResult.durationMs }} 毫秒
                  </span>
                </n-space>
                <pre v-if="testResult.output" class="check-output">{{ testResult.output }}</pre>
                <div v-else class="check-label">（脚本没有输出）</div>
              </div>
            </div>
          </n-form-item>
          <n-form-item label="触发方式">
            <n-radio-group v-model:value="form.triggerMode" vertical>
              <n-radio v-for="opt in triggerModeOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</n-radio>
            </n-radio-group>
          </n-form-item>
          <n-form-item v-if="form.triggerMode === 'repeat'" label="冷却时间">
            <n-input-number v-model:value="form.cooldownMinutes" :min="1" style="width: 140px">
              <template #suffix>分钟</template>
            </n-input-number>
          </n-form-item>
        </template>
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
.check-status {
  margin-top: 8px;
  font-size: 12px;
}

.check-label {
  opacity: 0.7;
}

.check-output {
  margin: 6px 0 0;
  padding: 8px 10px;
  max-height: 96px;
  overflow: auto;
  border: $border-faint;
  background: transparent;
  font-family: $font-mono;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}

.check-warning {
  margin-top: 6px;
  color: $ink;
  font-weight: 600;
}

.script-field {
  width: 100%;
}

.script-input :deep(textarea) {
  font-family: $font-mono;
  font-size: 12px;
}

.field-hint {
  margin-top: 6px;
  font-size: 12px;
  color: $ink-faint;
}

.script-actions {
  margin-top: 8px;
}

.test-result {
  margin-top: 8px;
  font-size: 12px;
}

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
