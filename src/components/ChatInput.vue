<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatInput.vue - 聊天输入组件

  功能说明:
  - 消息文本输入 (支持多行)
  - 附件文件上传 (图片/视频/文档)
  - API 配置切换
  - 思考模式开关 (仅对支持思考的服务商显示)
  - 消息发送/停止生成
  - 底部显示累计 token 用量与压缩上下文入口

  知识库 / MCP / Skill 的控制与状态已移至右侧 ChatSidePanel。
-->

<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import {
  NButton,
  NIcon,
  NText,
  NTooltip,
  NSelect,
  NTag,
} from "naive-ui";
import { useNotification } from "@/composables/useNotify";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore, PRESET_PROVIDERS } from "@/stores/settings";
import TokenCount from "@/components/TokenCount.vue";
import {
  Send,
  Close,
  ServerOutline,
  ChevronDown,
  BulbOutline,
} from "@vicons/ionicons5";

const chat = useChatStore();
const settings = useSettingsStore();
const notification = useNotification();

// ============ 响应式状态 ============

const inputValue = ref("");
const inputRef = ref<HTMLTextAreaElement | null>(null);
const fileInputRef = ref<HTMLInputElement | null>(null);
const attachedFiles = ref<File[]>([]);
const attachedDocuments = ref<Array<{ name: string; path: string }>>([]);
const showApiSelector = ref(false);

// ============ 计算属性 ============

const hasMessages = computed(() => {
  return chat.currentSession && chat.currentSession.messages.length > 0;
});

const canSend = computed(() => {
  const hasContent = inputValue.value.trim().length > 0;
  const hasFiles = attachedFiles.value.length > 0;
  const hasDocs = attachedDocuments.value.length > 0;
  return (hasContent || hasFiles || hasDocs) && !chat.isLoading && settings.activeConfig;
});

const apiConfigOptions = computed(() => {
  return settings.apiConfigs.map((config) => ({
    label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider})`,
    value: config.id,
  }));
});

const currentApiConfig = computed(() => settings.activeConfig);

/** 已完成交互由服务端返回的实际总用量；未返回 usage 的旧会话不纳入。 */
const sessionTokenCount = computed(() =>
  (chat.currentSession?.messages ?? []).reduce(
    (total, message) => total + (message.tokenUsage?.totalTokens ?? 0),
    0
  )
);

// ============ 思考模式支持 ============

const thinkingSupported = ref(false);
const refreshThinkingSupport = async () => {
  const provider = settings.activeConfig?.provider;
  if (!provider) {
    thinkingSupported.value = false;
    return;
  }
  try {
    thinkingSupported.value = await invoke<boolean>("supports_thinking", { provider });
  } catch {
    thinkingSupported.value = false;
  }
  // 切到不支持思考的服务商时，关掉遗留开关，避免状态悬空
  if (!thinkingSupported.value) chat.thinkingEnabled = false;
};

watch(() => settings.activeConfigId, refreshThinkingSupport);

// ============ 压缩上下文 ============

const isCompactingContext = ref(false);
const compactContext = async () => {
  isCompactingContext.value = true;
  try {
    await chat.compactContext();
  } finally {
    isCompactingContext.value = false;
  }
};

// ============ 生命周期 ============

onMounted(() => {
  refreshThinkingSupport();
});

// ============ 方法函数 ============

const readFileAsBase64 = (file: File): Promise<{ data: string; mediaType: string }> => {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = (e) => {
      const dataUrl = e.target?.result as string;
      const commaIdx = dataUrl.indexOf(",");
      if (commaIdx === -1) {
        reject(new Error("Invalid data URL"));
        return;
      }
      const header = dataUrl.slice(0, commaIdx);
      const data = dataUrl.slice(commaIdx + 1);
      const mediaType = header.split(":")[1]?.split(";")[0] ?? file.type;
      resolve({ data, mediaType });
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
};

const handleSend = async () => {
  const content = inputValue.value.trim();
  if ((!content && attachedFiles.value.length === 0) || chat.isLoading) return;

  if (!settings.activeConfig) {
    notification.error({
      title: "未配置 API",
      description: "请先前往设置创建 API 配置",
      duration: 3000,
    });
    return;
  }

  if (!chat.currentSession) {
    await chat.createSession(settings.activeConfig.id);
  }

  const imageFiles = attachedFiles.value.filter((f) => f.type.startsWith("image/"));
  const videoFiles = attachedFiles.value.filter((f) => f.type.startsWith("video/"));
  const otherFiles = attachedFiles.value.filter(
    (f) => !f.type.startsWith("image/") && !f.type.startsWith("video/")
  );

  const images =
    imageFiles.length > 0 ? await Promise.all(imageFiles.map(readFileAsBase64)) : undefined;
  const videos =
    videoFiles.length > 0 ? await Promise.all(videoFiles.map(readFileAsBase64)) : undefined;

  const fileInfo = attachedFiles.value.map((f) => ({ name: f.name, size: f.size }));

  let messageContent = content;
  const mentions = otherFiles.map(
    (f) => `[文件: ${f.name} (${(f.size / 1024 / 1024).toFixed(2)}MB)]`
  );
  if (mentions.length > 0) {
    messageContent = messageContent
      ? `${messageContent}\n${mentions.join(" ")}`
      : mentions.join(" ");
  }

  const docsToLoad = [...attachedDocuments.value];
  const documentContents: Array<{ name: string; content: string }> = [];
  for (const doc of docsToLoad) {
    try {
      const text = await invoke<string>("read_document_for_context", { filePath: doc.path });
      documentContents.push({ name: doc.name, content: text });
    } catch (err) {
      console.error(`Failed to read document ${doc.name}:`, err);
    }
  }

  inputValue.value = "";
  attachedFiles.value = [];
  attachedDocuments.value = [];
  if (inputRef.value) {
    inputRef.value.style.height = "60px";
  }

  try {
    await chat.sendMessage(
      messageContent,
      fileInfo.length > 0 ? fileInfo : undefined,
      images,
      videos,
      documentContents.length > 0 ? documentContents : undefined
    );
  } catch (error) {
    const errorInfo = chat.classifyError(error);
    notification.error({
      title: "发送失败",
      description: errorInfo.message,
      duration: 4000,
    });
  }
};

const handleStop = () => {
  chat.stopStream();
};

const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSend();
  }
};

const handleInput = () => {
  if (inputRef.value) {
    inputRef.value.style.height = "auto";
    inputRef.value.style.height = Math.min(inputRef.value.scrollHeight, 200) + "px";
  }
};

const handleApiChange = (configId: string) => {
  settings.setActiveConfig(configId);
  showApiSelector.value = false;
};

const handleFileSelect = () => {
  fileInputRef.value?.click();
};

const handleFilesSelected = (event: Event) => {
  const target = event.target as HTMLInputElement;
  const files = target.files;
  if (!files) return;

  const supportedFormats = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "video/mp4",
    "video/webm",
    "video/mpeg",
  ];

  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    if (supportedFormats.includes(file.type)) {
      if (!attachedFiles.value.find((f) => f.name === file.name && f.size === file.size)) {
        attachedFiles.value.push(file);
      }
    }
  }

  target.value = "";
};

const removeAttachedFile = (index: number) => {
  attachedFiles.value.splice(index, 1);
};

const getFileDisplayName = (file: File): string => {
  const maxLength = 20;
  return file.name.length > maxLength ? file.name.substring(0, maxLength) + "..." : file.name;
};

const handleDocumentAttach = async () => {
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "Documents",
        extensions: [
          "pdf", "docx", "doc", "xlsx", "xls", "csv", "pptx", "md", "markdown",
          "html", "htm", "txt", "rs", "js", "ts", "py", "java", "c", "cpp", "h", "go",
        ],
      },
    ],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  for (const path of paths) {
    const name = path.split(/[\\/]/).pop() ?? path;
    if (!attachedDocuments.value.find((d) => d.path === path)) {
      attachedDocuments.value.push({ name, path });
    }
  }
};

const removeAttachedDocument = (index: number) => {
  attachedDocuments.value.splice(index, 1);
};

const getDocDisplayName = (name: string): string => {
  const maxLength = 22;
  return name.length > maxLength ? name.substring(0, maxLength) + "..." : name;
};
</script>

<template>
  <div
    class="chat-input-wrapper"
    :style="{ maxWidth: `${settings.chatContentWidth}px` }"
  >
    <div class="input-container" :class="{ 'focus-lift': settings.inputFocusLiftEnabled }">
      <div class="input-box">
        <textarea
          ref="inputRef"
          v-model="inputValue"
          class="chat-input"
          :placeholder="
            !currentApiConfig
              ? '请先前往设置创建 API 配置...'
              : chat.ragEnabled
                ? '输入问题，将基于知识库回答...'
                : '输入消息，按 Enter 发送...'
          "
          rows="1"
          :disabled="chat.isLoading || !currentApiConfig"
          @keydown="handleKeydown"
          @input="handleInput"
        />
      </div>

      <div class="input-actions">
        <!-- 左侧: API 配置 + 附件 -->
        <div class="toolbar-group">
          <n-tooltip placement="top" :show-arrow="false" :delay="150">
            <template #trigger>
              <button
                type="button"
                class="api-selector-btn"
                @click="showApiSelector = !showApiSelector"
              >
                <n-icon :size="15"><ServerOutline /></n-icon>
                <span class="api-selector-name">{{ currentApiConfig ? currentApiConfig.name : "未选择 API" }}</span>
                <n-icon :size="12" class="chevron-icon"><ChevronDown /></n-icon>
              </button>
            </template>
            切换 API 配置
          </n-tooltip>

          <input
            ref="fileInputRef"
            type="file"
            multiple
            accept="image/*,video/mp4,video/webm,video/mpeg"
            style="display: none"
            @change="handleFilesSelected"
          >
          <n-tooltip class="input-tooltip" placement="top" :show-arrow="false" :delay="150">
            <template #trigger>
              <n-button tertiary circle size="large" class="file-btn" @click="handleFileSelect">
                <template #icon>
                  <n-icon>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="17 8 12 3 7 8" />
                      <line x1="12" y1="3" x2="12" y2="15" />
                    </svg>
                  </n-icon>
                </template>
              </n-button>
            </template>
            添加图片/视频 ({{ attachedFiles.length }})
          </n-tooltip>

          <n-tooltip class="input-tooltip" placement="top" :show-arrow="false" :delay="150">
            <template #trigger>
              <n-button
                tertiary
                circle
                size="large"
                class="doc-btn"
                :class="{ 'is-active': attachedDocuments.length > 0 }"
                @click="handleDocumentAttach"
              >
                <template #icon>
                  <n-icon>
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                      <polyline points="14 2 14 8 20 8" />
                      <line x1="12" y1="18" x2="12" y2="12" />
                      <line x1="9" y1="15" x2="15" y2="15" />
                    </svg>
                  </n-icon>
                </template>
              </n-button>
            </template>
            附加文档（注入上下文）{{
              attachedDocuments.length > 0 ? ` (${attachedDocuments.length})` : ""
            }}
          </n-tooltip>
        </div>

        <!-- 右侧: 思考模式 + 发送/停止 -->
        <div class="toolbar-group">
          <n-tooltip
            v-if="thinkingSupported"
            class="input-tooltip"
            placement="top"
            :show-arrow="false"
            :delay="150"
          >
            <template #trigger>
              <n-button
                quaternary
                circle
                size="large"
                class="thinking-btn"
                :class="{ 'is-active': chat.thinkingEnabled }"
                @click="chat.thinkingEnabled = !chat.thinkingEnabled"
              >
                <template #icon>
                  <n-icon><BulbOutline /></n-icon>
                </template>
              </n-button>
            </template>
            {{ chat.thinkingEnabled ? "关闭思考模式" : "开启思考模式" }}
          </n-tooltip>

          <n-tooltip class="input-tooltip" placement="top" :show-arrow="false" :delay="150">
            <template #trigger>
              <n-button
                type="primary"
                circle
                size="large"
                :disabled="!canSend && !chat.isLoading"
                class="send-btn"
                @click="chat.isLoading ? handleStop() : handleSend()"
              >
                <template #icon>
                  <n-icon>
                    <Send v-if="!chat.isLoading" />
                    <svg v-else viewBox="0 0 24 24" fill="currentColor">
                      <rect x="6" y="6" width="12" height="12" rx="2" />
                    </svg>
                  </n-icon>
                </template>
              </n-button>
            </template>
            {{ chat.isLoading ? "停止生成" : "发送消息" }}
          </n-tooltip>
        </div>
      </div>
    </div>

    <!-- 已附加文档显示 -->
    <div v-if="attachedDocuments.length > 0" class="attached-files">
      <div class="files-label">已附加文档（直接注入上下文）：</div>
      <div class="files-list">
        <div v-for="(doc, index) in attachedDocuments" :key="index" class="file-item">
          <n-tag closable type="info" class="file-tag" @close="removeAttachedDocument(index)">
            <template #icon>
              <n-icon :size="14">
                <svg viewBox="0 0 24 24" fill="currentColor">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6zm-1 1.5L18.5 9H13V3.5zM12 18v-5h1v4h1v-3h1v3h1v-5h-5v5h1zm-3-5h1v5H9v-5zm-3 0h1v2H6v1h1v2H6v-5z" />
                </svg>
              </n-icon>
            </template>
            {{ getDocDisplayName(doc.name) }}
          </n-tag>
        </div>
      </div>
    </div>

    <!-- 已附加文件显示 -->
    <div v-if="attachedFiles.length > 0" class="attached-files">
      <div class="files-label">已附加的文件：</div>
      <div class="files-list">
        <div v-for="(file, index) in attachedFiles" :key="index" class="file-item">
          <n-tag closable class="file-tag" @close="removeAttachedFile(index)">
            <template #icon>
              <n-icon :size="14">
                <svg v-if="file.type.startsWith('image/')" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M21 19V5c0-1.1-.9-2-2-2H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2zM8.5 13.5l2.5 3.01L14.5 12l4.5 6H5l3.5-4.5z" />
                </svg>
                <svg v-else-if="file.type.startsWith('video/')" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M18 3H6c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-5 10l-4-3v6l4-3z" />
                </svg>
              </n-icon>
            </template>
            {{ getFileDisplayName(file) }}
          </n-tag>
        </div>
      </div>
    </div>

    <!-- API 选择器 Popover -->
    <div v-if="showApiSelector" class="selector-popover api-selector">
      <div class="selector-header">
        <n-text strong>选择 API 配置</n-text>
        <n-button quaternary circle size="small" @click="showApiSelector = false">
          <template #icon>
            <n-icon><Close /></n-icon>
          </template>
        </n-button>
      </div>
      <n-select
        :value="settings.activeConfigId || ''"
        :options="apiConfigOptions"
        placeholder="选择要使用的 API 配置"
        @update:value="handleApiChange"
      />
      <n-text v-if="apiConfigOptions.length === 0" depth="3" class="selector-hint">
        暂无 API 配置，请前往设置创建
      </n-text>
    </div>

    <!-- 底部: 累计用量 + 压缩上下文 -->
    <div class="input-footer">
      <div class="footer-usage">
        <span class="session-token-eyebrow">Session Usage</span>
        <TokenCount
          label="累计"
          :count="sessionTokenCount"
          :exact="true"
          description="已完成交互的 API 实际 total_tokens 累计"
        />
      </div>
      <button
        class="compact-context-button"
        type="button"
        :disabled="isCompactingContext || !hasMessages"
        @click="compactContext"
      >
        {{ isCompactingContext ? "正在压缩…" : "压缩上下文" }}
      </button>
    </div>
  </div>
</template>

<style scoped lang="scss">
.chat-input-wrapper {
  padding: 16px 32px 24px;
  margin: 0 auto;
  position: relative;
}

.input-container {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: $bg;
  padding: 12px 16px;
  border: $border;
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;
}

.input-container.focus-lift:focus-within {
  transform: translateY(-4px);
  box-shadow: $shadow-hover;
}

.input-box {
  width: 100%;
  min-height: 44px;
  max-height: 200px;
}

.chat-input {
  width: 100%;
  min-height: 44px;
  max-height: 200px;
  padding: 10px 12px;
  border: none;
  background: transparent;
  color: $ink;
  font-size: 15px;
  line-height: $leading-body;
  resize: none;
  font-family: inherit;
  outline: none;
}

.chat-input::placeholder {
  color: $ink-faint;
}

.chat-input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.input-actions {
  display: flex;
  width: 100%;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding-top: 8px;
  border-top: $border-faint;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.api-selector-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border: $border-faint;
  border-radius: 0;
  background: $bg;
  color: $ink;
  font-size: 13px;
  cursor: pointer;
  transition: background $duration-fast $ease, color $duration-fast $ease;

  &:hover {
    background: $ink;
    color: $bg;
  }
}

.api-selector-name {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chevron-icon {
  transition: transform $duration-fast $ease;
}

.input-actions :deep(.n-button) {
  border: 1px solid transparent;
  color: $ink;
  transition:
    background $duration-fast $ease,
    color $duration-fast $ease,
    border-color $duration-fast $ease;
}

.input-actions :deep(.n-button.is-active) {
  background: $ink;
  border-color: $ink;
  color: $bg;
}

.input-actions :deep(.n-button:not(.n-button--disabled):hover) {
  background: $ink;
  border-color: $ink;
  color: $bg;
}

.send-btn {
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;
}

.send-btn:not(:disabled):hover {
  transform: translateY(-4px);
  box-shadow: $shadow-hover;
}

.send-btn:disabled {
  opacity: 0.4;
}

.selector-popover {
  position: absolute;
  bottom: 100%;
  left: 32px;
  right: 32px;
  margin-bottom: 8px;
  background: $bg;
  border: $border;
  padding: 16px;
  box-shadow: $shadow-hover;
  z-index: 100;
  animation: message-enter $duration $ease both;
}

@keyframes message-enter {
  from {
    opacity: 0;
    transform: translateY(40px) scale(0.95);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.api-selector {
  z-index: 101;
}

.selector-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.selector-hint {
  display: block;
  margin-top: 8px;
  font-size: 12px;
}

.input-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 12px;
}

.footer-usage {
  display: flex;
  align-items: center;
  gap: 8px;
}

.session-token-eyebrow {
  color: $ink-faint;
  font-family: $font-sans;
  font-size: 10px;
  font-weight: 500;
  letter-spacing: $label-tracking;
  text-transform: uppercase;
}

.compact-context-button {
  border: $border;
  border-radius: 0;
  background: $bg;
  color: $ink;
  padding: 4px 9px;
  font-family: $font-sans;
  font-size: 11px;
  letter-spacing: 0.04em;
  cursor: pointer;
  transition: background $duration-fast $ease, color $duration-fast $ease;

  &:hover:not(:disabled) {
    background: $ink;
    color: $bg;
  }

  &:disabled {
    color: $ink-faint;
    border-color: var(--color-line-faint);
    cursor: not-allowed;
  }
}

.attached-files {
  margin-top: 10px;
  padding: 0;
}

.files-label {
  font-size: $label-size;
  font-weight: 700;
  letter-spacing: $label-tracking;
  color: $ink-faint;
  margin-bottom: 6px;
}

.files-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.file-item {
  display: inline-block;
}

.file-tag {
  font-size: 12px;
  max-width: 200px;
}

.attached-files :deep(.file-tag) {
  min-height: 30px;
  padding: 5px 8px;
  border: $border;
  border-radius: 0;
  background: $surface;
  color: $ink;
  font-size: 12px;
}

.attached-files :deep(.file-tag .n-tag__close) {
  color: $ink;
}

.file-btn,
.doc-btn {
  transition: background $duration-fast $ease;
}

.file-btn:hover,
.doc-btn:hover {
  background: var(--color-surface);
}

:global(.input-tooltip.n-popover) {
  padding: 7px 9px;
  border: $border;
  border-radius: 0;
  background: $ink;
  box-shadow: none;
  color: $bg;
  font-family: $font-sans;
  font-size: 12px;
  line-height: 1.4;
}

:global(.input-tooltip .n-popover__content) {
  color: inherit;
}

@media (max-width: 640px) {
  .chat-input-wrapper {
    padding: 12px 16px 20px;
  }

  .input-actions {
    gap: 6px;
  }
}
</style>
