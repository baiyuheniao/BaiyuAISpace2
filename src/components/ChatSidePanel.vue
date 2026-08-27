<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatSidePanel.vue - 聊天页右侧会话上下文面板

  功能说明:
  - 文件管理: 当前会话工作目录与文件权限
  - 知识库: 选择用于 RAG 的知识库
  - MCP: 启用/停用模型工具
  - Skill: 手动激活 Skill 与自主调用开关

  原先这些状态散在输入框上方的"上下文指示条"和页面顶部的工作目录栏里，
  统一收进右侧面板，作为会话级上下文，与消息流在视觉上分离。
-->

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
  NSelect,
  NSwitch,
  NCheckbox,
  NCheckboxGroup,
  NSpace,
  NText,
  NButton,
  NModal,
  NRadio,
  NRadioGroup,
  NIcon,
} from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { useChatStore } from "@/stores/chat";
import { useKnowledgeBaseStore } from "@/stores/knowledgeBase";
import { useMCPStore } from "@/stores/mcp";
import { useSkillsStore } from "@/stores/skills";
import { FolderOpenOutline, Library, Cube, ExtensionPuzzleOutline } from "@vicons/ionicons5";

const chat = useChatStore();
const kbStore = useKnowledgeBaseStore();
const mcp = useMCPStore();
const skillsStore = useSkillsStore();

// ============ 文件管理 ============

const session = computed(() => chat.currentSession);
const workingDir = computed(() => session.value?.workingDirectory || null);
const fileAccessMode = computed(() => session.value?.fileAccessMode || "none");

const accessModeText = computed(() => {
  if (!workingDir.value) return "尚未选择";
  return fileAccessMode.value === "write" ? "可编辑" : fileAccessMode.value === "read" ? "只读" : "无文件权限";
});

const pendingWorkingDirectory = ref<string | null>(null);
const pendingFileAccessMode = ref<"read" | "write">("write");
const showFileAccessModeModal = ref(false);

const chooseWorkingDirectory = async () => {
  const directory = await open({ directory: true, multiple: false, title: "选择当前聊天的工作目录" });
  if (typeof directory === "string") {
    pendingWorkingDirectory.value = directory;
    pendingFileAccessMode.value = "write";
    showFileAccessModeModal.value = true;
  }
};

const confirmWorkingDirectory = async () => {
  if (!pendingWorkingDirectory.value) return;
  await chat.setWorkingDirectory(pendingWorkingDirectory.value, pendingFileAccessMode.value);
  showFileAccessModeModal.value = false;
  pendingWorkingDirectory.value = null;
};

const cycleFileAccess = async () => {
  if (!workingDir.value) return;
  const mode = fileAccessMode.value === "write" ? "read" : fileAccessMode.value === "read" ? "none" : "write";
  await chat.setWorkingDirectory(workingDir.value, mode);
};

const removeWorkingDirectory = async () => {
  await chat.setWorkingDirectory(null);
};

// ============ 知识库 ============

const kbOptions = computed(() => [
  { label: "不使用知识库", value: "" },
  ...kbStore.knowledgeBases.map((kb) => ({
    label: `${kb.name} (${kb.document_count} 文档)`,
    value: kb.id,
  })),
]);

const handleKbChange = (value: string) => {
  if (value === "") {
    chat.selectKnowledgeBaseForRag(null);
    chat.toggleRag(false);
  } else {
    chat.selectKnowledgeBaseForRag(value);
    chat.toggleRag(true);
  }
};

// ============ MCP ============

const enabledMcpServersCount = computed(() => mcp.servers.filter((s) => s.enabled).length);
const availableMcpToolsCount = computed(() => mcp.availableTools.length);

// ============ Skill ============

const skillOptions = computed(() =>
  skillsStore.enabledSkills.map((s) => ({ label: s.name, value: s.id }))
);

onMounted(() => {
  kbStore.loadKnowledgeBases();
  mcp.loadServers();
  skillsStore.loadSkills();
});
</script>

<template>
  <aside class="chat-side-panel">
    <!-- 文件管理 -->
    <section class="panel-section">
      <header class="section-header">
        <n-icon :size="15"><FolderOpenOutline /></n-icon>
        <span>文件管理</span>
      </header>
      <div class="section-body">
        <div class="dir-path">{{ workingDir || "尚未选择工作目录" }}</div>
        <div class="dir-meta">
          权限：{{ accessModeText }}
        </div>
        <div class="dir-actions">
          <button type="button" @click="chooseWorkingDirectory">{{ workingDir ? "更换目录" : "选择目录" }}</button>
          <button v-if="workingDir" type="button" @click="cycleFileAccess">切换权限</button>
          <button v-if="workingDir" type="button" @click="removeWorkingDirectory">移除</button>
        </div>
      </div>
    </section>

    <!-- 知识库 -->
    <section class="panel-section">
      <header class="section-header">
        <n-icon :size="15"><Library /></n-icon>
        <span>知识库</span>
      </header>
      <div class="section-body">
        <n-select
          :value="chat.selectedKnowledgeBaseId || ''"
          :options="kbOptions"
          placeholder="选择要使用的知识库"
          @update:value="handleKbChange"
        />
        <n-text v-if="kbStore.knowledgeBases.length === 0" depth="3" class="section-hint">
          暂无知识库，请前往知识库页导入文档
        </n-text>
      </div>
    </section>

    <!-- MCP -->
    <section class="panel-section">
      <header class="section-header">
        <n-icon :size="15"><Cube /></n-icon>
        <span>MCP 工具</span>
      </header>
      <div class="section-body">
        <div class="row-between">
          <span class="row-label">{{ chat.mcpEnabled ? "已启用" : "已停用" }}</span>
          <n-switch
            :value="chat.mcpEnabled"
            :disabled="availableMcpToolsCount === 0"
            size="small"
            @update:value="(v: boolean) => (chat.mcpEnabled = v)"
          />
        </div>
        <n-text depth="3" class="section-hint">
          {{ availableMcpToolsCount === 0 ? "无可用工具" : `${enabledMcpServersCount} 个服务 / ${availableMcpToolsCount} 个工具` }}
        </n-text>
      </div>
    </section>

    <!-- Skill -->
    <section class="panel-section">
      <header class="section-header">
        <n-icon :size="15"><ExtensionPuzzleOutline /></n-icon>
        <span>Skill</span>
      </header>
      <div class="section-body">
        <n-checkbox-group v-model:value="chat.activeSkillIds">
          <n-space vertical :size="8">
            <n-checkbox
              v-for="option in skillOptions"
              :key="option.value"
              :value="option.value"
              :label="option.label"
            />
          </n-space>
        </n-checkbox-group>
        <n-text v-if="skillOptions.length === 0" depth="3" class="section-hint">
          暂无已启用的 Skill，请前往 Skill 页创建
        </n-text>
        <div class="row-between autonomy-row">
          <span class="row-label">模型可自主调用 Skill</span>
          <n-switch v-model:value="chat.skillAutonomyEnabled" size="small" />
        </div>
      </div>
    </section>

    <!-- 文件权限确认弹窗 -->
    <n-modal v-model:show="showFileAccessModeModal" preset="card" title="设置文件权限" style="width: 440px">
      <n-text depth="3">{{ pendingWorkingDirectory }}</n-text>
      <n-radio-group v-model:value="pendingFileAccessMode" style="display: flex; margin-top: 18px; gap: 18px">
        <n-radio value="read">只读</n-radio>
        <n-radio value="write">可编辑</n-radio>
      </n-radio-group>
      <template #footer>
        <n-button @click="showFileAccessModeModal = false">取消</n-button>
        <n-button type="primary" @click="confirmWorkingDirectory">确认</n-button>
      </template>
    </n-modal>
  </aside>
</template>

<style scoped lang="scss">
.chat-side-panel {
  width: 260px;
  flex-shrink: 0;
  height: 100%;
  overflow-y: auto;
  border-left: $border;
  background: $bg;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.panel-section {
  border: $border-faint;
  background: $surface;
}

.section-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: $border-faint;
  font-family: $font-serif;
  font-size: 0.9rem;
  font-weight: 700;
  color: $ink;
}

.section-body {
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.dir-path {
  font-family: $font-mono;
  font-size: 12px;
  word-break: break-all;
  color: $ink;
}

.dir-meta {
  font-family: $font-mono;
  font-size: 11px;
  color: $ink-soft;
}

.dir-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;

  button {
    border: $border;
    border-radius: 0;
    background: $bg;
    color: $ink;
    padding: 3px 8px;
    font-family: $font-sans;
    font-size: 12px;
    cursor: pointer;

    &:hover {
      background: $ink;
      color: $bg;
    }
  }
}

.row-between {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.row-label {
  font-size: 13px;
  color: $ink;
}

.autonomy-row {
  margin-top: 4px;
  padding-top: 10px;
  border-top: $border-faint;
}

.section-hint {
  font-size: 12px;
}
</style>
