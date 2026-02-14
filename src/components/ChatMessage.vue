<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed, ref } from "vue";
import { NAvatar, NIcon, NSpin, NAlert, NTooltip } from "naive-ui";
import { marked } from "marked";
import { markedHighlight } from "marked-highlight";
import hljs from "highlight.js";
import "highlight.js/styles/github-dark.css";
import type { Message } from "@/stores/chat";
import { Person, Sparkles, Copy } from "@vicons/ionicons5";

const props = defineProps<{
  message: Message;
}>();

// Configure marked once
let markedConfigured = false;
if (!markedConfigured) {
  marked.use(
    markedHighlight({
      langPrefix: "hljs language-",
      highlight(code, lang) {
        const language = hljs.getLanguage(lang) ? lang : "plaintext";
        return hljs.highlight(code, { language }).value;
      },
    })
  );
  markedConfigured = true;
}

const isUser = computed(() => props.message.role === "user");
const isAssistant = computed(() => props.message.role === "assistant");

const renderedContent = computed(() => {
  if (!props.message.content) return "";
  return marked.parse(props.message.content, { async: false }) as string;
});

const formatTime = (timestamp: number) => {
  return new Date(timestamp).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });
};

// Copy functionality
const copied = ref(false);

const handleCopy = async () => {
  try {
    await navigator.clipboard.writeText(props.message.content);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (err) {
    console.error("Failed to copy:", err);
  }
};
</script>

<template>
  <div class="message-wrapper" :class="{ 'user-message': isUser }">
    <div class="message-avatar">
      <n-avatar 
        round 
        :size="36" 
        class="avatar"
        :class="{ 'user-avatar': isUser, 'ai-avatar': isAssistant }"
      >
        <n-icon :size="18">
          <Person v-if="isUser" />
          <Sparkles v-else />
        </n-icon>
      </n-avatar>
    </div>

    <div class="message-content">
      <div class="message-header">
        <span class="message-author">{{ isUser ? "你" : "AI 助手" }}</span>
        <span class="message-time">{{ formatTime(message.timestamp) }}</span>
      </div>

      <div class="message-body" :class="{ 'user-body': isUser }">
        <div class="markdown-content" v-html="renderedContent" />
        
        <!-- Streaming indicator -->
        <div v-if="message.streaming" class="streaming-indicator">
          <n-spin size="small" />
          <span class="streaming-text">思考中...</span>
        </div>
      </div>

      <!-- Error message -->
      <div v-if="message.error" class="message-error">
        <n-alert type="error" :show-icon="true" :bordered="false">
          {{ message.error }}
        </n-alert>
      </div>

      <!-- Actions -->
      <div v-if="!isUser && !message.streaming" class="message-actions">
        <n-tooltip placement="top" :show="copied">
          <template #trigger>
            <button class="action-btn" title="复制" @click="handleCopy">
              <n-icon :size="14"><Copy /></n-icon>
            </button>
          </template>
          <span>已复制!</span>
        </n-tooltip>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.message-wrapper {
  display: flex;
  gap: 16px;
  padding: 20px 0;
  animation: fadeIn 0.3s ease;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.message-wrapper.user-message {
  flex-direction: row-reverse;

  .message-content {
    align-items: flex-end;
  }

  .message-header {
    flex-direction: row-reverse;
  }
}

.avatar {
  flex-shrink: 0;
}

.user-avatar {
  background: #000000;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}

.ai-avatar {
  background: #333333;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.message-content {
  flex: 1;
  max-width: calc(100% - 80px);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.message-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  padding: 0 4px;
}

.message-author {
  font-weight: 600;
  color: var(--n-text-color-1);
}

.message-time {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.message-body {
  padding: 16px 20px;
  background: var(--n-color-embed);
  border-radius: 18px;
  border-bottom-left-radius: 4px;
  word-break: break-word;
  line-height: 1.7;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
  transition: box-shadow 0.2s;
}

.message-body:hover {
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
}

.message-body.user-body {
  background: rgba(0, 0, 0, 0.05);
  border-bottom-left-radius: 18px;
  border-bottom-right-radius: 4px;
}

.markdown-content {
  color: var(--n-text-color-1);
}

.markdown-content :deep(p) {
  margin: 0 0 12px 0;
}

.markdown-content :deep(p:last-child) {
  margin-bottom: 0;
}

.markdown-content :deep(pre) {
  background: #1e1e2e;
  border-radius: 12px;
  padding: 16px;
  margin: 12px 0;
  overflow-x: auto;
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.markdown-content :deep(code) {
  font-family: "JetBrains Mono", "Fira Code", "Consolas", monospace;
  font-size: 0.9em;
}

.markdown-content :deep(pre code) {
  background: transparent;
  padding: 0;
  color: #cdd6f4;
}

.markdown-content :deep(:not(pre) > code) {
  background: rgba(128, 128, 128, 0.15);
  padding: 3px 6px;
  border-radius: 6px;
  color: var(--n-text-color-1);
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  margin: 12px 0;
  padding-left: 24px;
}

.markdown-content :deep(li) {
  margin: 6px 0;
}

.markdown-content :deep(blockquote) {
  margin: 12px 0;
  padding: 12px 16px;
  border-left: 4px solid #000000;
  background: rgba(0, 0, 0, 0.04);
  border-radius: 0 8px 8px 0;
}

.markdown-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 12px 0;
  border-radius: 8px;
  overflow: hidden;
}

.markdown-content :deep(th),
.markdown-content :deep(td) {
  border: 1px solid var(--n-border-color);
  padding: 10px 14px;
  text-align: left;
}

.markdown-content :deep(th) {
  background: rgba(24, 160, 88, 0.1);
  font-weight: 600;
}

.markdown-content :deep(tr:nth-child(even)) {
  background: var(--n-color-embed);
}

.streaming-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed var(--n-border-color);
  color: var(--n-text-color-3);
  font-size: 13px;
}

.message-error {
  margin-top: 8px;
}

.message-actions {
  display: flex;
  gap: 8px;
  padding: 4px;
  opacity: 0;
  transition: opacity 0.2s;
}

.message-wrapper:hover .message-actions {
  opacity: 1;
}

.action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--n-text-color-3);
  cursor: pointer;
  transition: all 0.2s;
}

.streaming-text {
  color: var(--n-text-color-3);
  font-size: 13px;
}

.action-btn:hover {
  background: var(--n-color-embed);
  color: var(--n-text-color-1);
}
</style>
