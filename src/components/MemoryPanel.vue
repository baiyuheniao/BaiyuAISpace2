<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  NCard,
  NSwitch,
  NButton,
  NSpace,
  NInput,
  NInputNumber,
  NSelect,
  NModal,
  NFormItem,
  NPopconfirm,
  NEmpty,
  NTag,
} from "naive-ui";
import { useSettingsStore } from "@/stores/settings";
import { useMessage } from "@/composables/useNotify";

interface Model {
  provider: string;
  model: string;
  api_config_id: string;
  base_url: string;
}
interface Settings {
  enabled: boolean;
  capture: boolean;
  inject: boolean;
  auto_compact: boolean;
  context_tokens: number;
  injection_tokens: number;
  stale_days: number;
  dreaming: boolean;
  embedding: Model | null;
  dream_model: Model | null;
}
interface Entry {
  id: string;
  scope: string;
  owner: string;
  kind: string;
  content: string;
  source: string;
  updated_at: number;
  confirmed: boolean;
}
interface Audit {
  action: string;
  id: string;
  at: number;
}
const appSettings = useSettingsStore();
const notify = useMessage();
const config = ref<Settings | null>(null);
const entries = ref<Entry[]>([]);
const audit = ref<Audit[]>([]);
const show = ref(false);
const busy = ref(false);
const query = ref("");
const scope = ref<string | null>(null);
const editing = ref<Entry | null>(null);
const snapshotInput = ref<HTMLInputElement | null>(null);
const pendingSnapshot = ref<Entry[] | null>(null);
const scopes = [
  { label: "用户全局", value: "user" },
  { label: "工作空间", value: "workspace" },
  { label: "会话", value: "session" },
  { label: "后台建议", value: "dream" },
];
const filtered = computed(() =>
  entries.value.filter(
    (e) =>
      (!scope.value || (scope.value === "dream" ? e.kind === "dream" : e.scope === scope.value)) &&
      `${e.content} ${e.source} ${e.owner}`.toLowerCase().includes(query.value.toLowerCase())
  )
);
const modelOptions = computed(() =>
  appSettings.apiConfigs.map((c) => ({ label: `${c.name} · ${c.model}`, value: c.id }))
);
const embeddingOptions = computed(() =>
  appSettings.embeddingApiConfigs.map((c) => ({ label: `${c.name} · ${c.model}`, value: c.id }))
);
function selectModel(id: string | null, embedding: boolean) {
  if (!config.value) return;
  const c = (embedding ? appSettings.embeddingApiConfigs : appSettings.apiConfigs).find(
    (c) => c.id === id
  );
  const model = c
    ? { provider: c.provider, model: c.model, api_config_id: c.id, base_url: c.baseUrl }
    : null;
  if (embedding) config.value.embedding = model;
  else config.value.dream_model = model;
}
async function load() {
  try {
    const result = await invoke<{ settings: Settings; entries: Entry[]; audit: Audit[] }>(
      "memory_overview"
    );
    config.value = result.settings;
    entries.value = result.entries;
    audit.value = result.audit;
  } catch (e) {
    notify.error(`读取记忆失败：${e}`);
  }
}
async function save() {
  busy.value = true;
  try {
    await invoke("memory_save_settings", { settings: config.value });
    await load();
    notify.success("记忆设置已保存并立即生效");
  } catch (e) {
    notify.error(`保存失败：${e}`);
  } finally {
    busy.value = false;
  }
}
async function forget(id: string) {
  try {
    await invoke("memory_forget_entries", { ids: [id] });
    await load();
    notify.success("已遗忘；原始聊天记录仍保留");
  } catch (e) {
    notify.error(`遗忘失败：${e}`);
  }
}
async function review() {
  if (!editing.value) return;
  try {
    await invoke("memory_review_entry", { id: editing.value.id, content: editing.value.content });
    editing.value = null;
    await load();
    notify.success("已保存并确认记忆");
  } catch (e) {
    notify.error(`保存失败：${e}`);
  }
}
function exportSnapshot() {
  // 导出的独立副本由用户保管，后续遗忘不会修改已导出文件。
  const blob = new Blob(
    [JSON.stringify({ version: 1, exported_at: Date.now(), entries: entries.value }, null, 2)],
    { type: "application/json" }
  );
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `记忆快照-${Date.now()}.json`;
  a.click();
  URL.revokeObjectURL(url);
}
async function readSnapshot(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file) return;
  try {
    if (file.size > 16 * 1024 * 1024) throw new Error("快照不能超过 16 MiB");
    const data = JSON.parse(await file.text());
    if (data.version !== 1 || !Array.isArray(data.entries))
      throw new Error("请选择本应用导出的记忆快照");
    pendingSnapshot.value = data.entries;
  } catch (e) {
    notify.error(`读取快照失败：${e}`);
  } finally {
    input.value = "";
  }
}
async function restoreSnapshot() {
  try {
    const count = await invoke<number>("memory_restore_snapshot", {
      entries: pendingSnapshot.value,
    });
    pendingSnapshot.value = null;
    await load();
    notify.success(`已恢复 ${count} 条记忆，现有记录未覆盖`);
  } catch (e) {
    notify.error(`恢复失败：${e}`);
  }
}
onMounted(load);
</script>

<template>
  <n-card title="分层记忆" :bordered="false" class="memory-panel">
    <p>自动保留会话线索，按需回忆偏好、决策与经验。支持普通聊天和 Agent Team，无需安装记忆 MCP。</p>
    <n-space align="center">
      <n-tag>{{ config?.enabled ? "已启用" : "已关闭" }}</n-tag>
      <span>{{ entries.length }} 条记忆</span>
      <n-button
        @click="
          show = true;
          load();
        "
        >管理记忆与设置</n-button
      >
    </n-space>
  </n-card>
  <n-modal v-model:show="show" preset="card" title="记忆与隐私" class="memory-dialog">
    <template v-if="config">
      <div class="memory-settings">
        <n-form-item label="启用记忆"><n-switch v-model:value="config.enabled" /></n-form-item>
        <n-form-item label="自动记录会话线索"
          ><n-switch v-model:value="config.capture"
        /></n-form-item>
        <n-form-item label="自动检索并注入相关记忆"
          ><n-switch v-model:value="config.inject"
        /></n-form-item>
        <n-form-item label="接近预算时自动生成上下文摘要"
          ><n-switch v-model:value="config.auto_compact"
        /></n-form-item>
        <n-form-item label="活动上下文预算（估算 token）"
          ><n-input-number
            v-model:value="config.context_tokens"
            :min="4000"
            :max="200000"
            placeholder="请输入模型可承受的上下文预算"
        /></n-form-item>
        <n-form-item label="记忆注入预算（估算 token）"
          ><n-input-number
            v-model:value="config.injection_tokens"
            :min="256"
            :max="8000"
            placeholder="请输入记忆预算"
        /></n-form-item>
        <n-form-item label="过时提醒天数"
          ><n-input-number v-model:value="config.stale_days" :min="1" placeholder="请输入天数"
        /></n-form-item>
        <n-form-item label="语义检索模型（可选）"
          ><n-select
            :value="config.embedding?.api_config_id"
            :options="embeddingOptions"
            clearable
            placeholder="不选择时使用本地中文关键词检索"
            @update:value="selectModel($event, true)"
        /></n-form-item>
        <n-form-item label="空闲 5 分钟后整理后台建议"
          ><n-switch v-model:value="config.dreaming"
        /></n-form-item>
        <n-form-item label="后台整理模型"
          ><n-select
            :value="config.dream_model?.api_config_id"
            :options="modelOptions"
            clearable
            placeholder="请选择已配置的模型"
            @update:value="selectModel($event, false)"
        /></n-form-item>
      </div>
      <p class="memory-help">
        自动摘要会调用当前聊天模型，完整聊天记录不删除。语义检索和后台整理会把经过常见凭据过滤的文本发送给所选模型，并产生相应调用费用。过滤不保证识别所有隐私，请按场景关闭自动记录。关闭记忆会停止采集、检索和记忆工具，已有记录仍可管理。
      </p>
      <p class="memory-help">
        后台建议仅在这里或明确查询时显示，确认后才作为普通记忆使用。用户记忆跨场景共享；普通聊天以所选工作目录区分场景，未选择目录时按会话隔离；Agent
        Team 按工作组隔离。
      </p>
      <n-space
        ><n-button :loading="busy" @click="save">保存设置</n-button
        ><n-button @click="exportSnapshot">导出记忆快照</n-button
        ><n-button @click="snapshotInput?.click()">恢复记忆快照</n-button
        ><n-button @click="load">刷新</n-button></n-space
      >
      <input ref="snapshotInput" type="file" accept=".json" hidden @change="readSnapshot" />
      <div class="memory-filter">
        <n-input v-model:value="query" clearable placeholder="筛选内容、来源或场景" /><n-select
          v-model:value="scope"
          clearable
          :options="scopes"
          placeholder="全部范围"
        />
      </div>
      <n-empty v-if="!filtered.length" description="暂无匹配记忆" />
      <article v-for="entry in filtered" :key="entry.id" class="memory-entry">
        <n-space align="center"
          ><n-tag>{{ scopes.find((s) => s.value === entry.scope)?.label }}</n-tag
          ><n-tag>{{
            entry.kind === "dream" ? "后台建议" : entry.confirmed ? "已确认" : "未确认"
          }}</n-tag
          ><span>{{ new Date(entry.updated_at).toLocaleString() }}</span
          ><span v-if="Date.now() - entry.updated_at > config.stale_days * 86400000"
            >可能过时</span
          ></n-space
        >
        <p class="memory-body">{{ entry.content }}</p>
        <p class="memory-source">来源：{{ entry.source }} · 场景：{{ entry.owner }}</p>
        <n-space
          ><n-button size="small" @click="editing = { ...entry }">编辑并确认</n-button
          ><n-popconfirm @positive-click="forget(entry.id)"
            ><template #trigger><n-button size="small">遗忘</n-button></template
            >删除此记忆及其检索向量？原始聊天和已导出的快照仍保留。</n-popconfirm
          ></n-space
        >
      </article>
      <details class="memory-audit">
        <summary>最近的记忆审计（不含正文）</summary>
        <p v-for="(item, index) in audit" :key="index">
          {{ new Date(item.at).toLocaleString() }} · {{ item.action }} · {{ item.id }}
        </p>
      </details>
    </template>
  </n-modal>
  <n-modal
    :show="!!editing"
    preset="card"
    title="编辑并确认记忆"
    class="memory-dialog"
    @update:show="!$event && (editing = null)"
  >
    <template v-if="editing"
      ><n-input
        v-model:value="editing.content"
        type="textarea"
        :autosize="{ minRows: 6, maxRows: 18 }"
        placeholder="请输入经你确认的事实、偏好或经验"
      /><n-button class="memory-save" @click="review">保存并确认</n-button></template
    >
  </n-modal>
  <n-modal
    :show="!!pendingSnapshot"
    preset="dialog"
    title="恢复记忆快照"
    positive-text="恢复"
    negative-text="取消"
    @positive-click="restoreSnapshot"
    @negative-click="pendingSnapshot = null"
    @update:show="!$event && (pendingSnapshot = null)"
  >
    将补入快照中的
    {{
      pendingSnapshot?.length
    }}
    条记录中当前缺失的部分，包括曾经遗忘的记录。恢复后均标为未确认，不覆盖现有记忆。
  </n-modal>
</template>

<style lang="scss">
@use "@/styles/variables.scss" as *;
.memory-panel {
  margin-bottom: 2rem;
  p {
    color: $ink-soft;
    line-height: $leading-body;
  }
}
.memory-dialog {
  width: min(960px, 92vw);
  max-height: 85vh;
  overflow: auto;
}
.memory-settings {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0 2rem;
}
.memory-help,
.memory-source {
  color: $ink-soft;
  line-height: $leading-body;
  font-size: 0.85rem;
  overflow-wrap: anywhere;
}
.memory-filter {
  display: grid;
  grid-template-columns: 2fr 1fr;
  gap: 1rem;
  margin: 2rem 0;
}
.memory-entry {
  border-top: $border-soft;
  padding: 1.5rem 0;
}
.memory-body {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: $leading-body;
}
.memory-audit {
  border-top: $border-soft;
  padding-top: 1rem;
  font-family: $font-mono;
  font-size: 0.75rem;
  overflow-wrap: anywhere;
}
.memory-save {
  margin-top: 1rem;
}
@media (max-width: 640px) {
  .memory-settings {
    grid-template-columns: 1fr;
  }
}
</style>
