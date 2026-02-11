<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, computed } from "vue";
import { NButton, NIcon, NSpace, NText, NTooltip } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore } from "@/stores/settings";
import { Send, Sparkles } from "@vicons/ionicons5";

const chat = useChatStore();
const settings = useSettingsStore();

const inputValue = ref("");
const inputRef = ref<HTMLTextAreaElement | null>(null);

const canSend = computed(() => {
  return inputValue.value.trim().length > 0 && !chat.isLoading;
});

const handleSend = async () => {
  const content = inputValue.value.trim();
  if (!content || chat.isLoading) return;

  if (!chat.currentSession) {
    chat.createSession(settings.currentProvider.id, settings.currentProvider.selectedModel);
  }

  inputValue.value = "";
  if (inputRef.value) {
    inputRef.value.style.height = "60px";
  }

  await chat.sendMessage(content);
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
</script>

<template>
  <div class="chat-input-wrapper">
    <div class="input-container">
      <div class="input-box">
        <textarea
          ref="inputRef"
          v-model="inputValue"
          class="chat-input"
          placeholder="输入消息，按 Enter 发送..."
          rows="1"
          :disabled="chat.isLoading"
          @keydown="handleKeydown"
          @input="handleInput"
        />
      </div>

      <div class="input-actions">
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              type="primary"
              circle
              size="large"
              :disabled="!canSend"
              :loading="chat.isLoading"
              class="send-btn"
              @click="handleSend"
            >
              <template #icon>
                <n-icon><Send /></n-icon>
              </template>
            </n-button>
          </template>
          发送消息
        </n-tooltip>
      </div>
    </div>

    <div class="input-footer">
      <n-space align="center" :size="16">
        <n-text depth="3" class="hint-text">
          <span style="margin-right: 4px;">⌨️</span>
          Enter 发送 · Shift+Enter 换行
        </n-text>
        <n-text depth="3" class="divider">|</n-text>
        <n-space align="center" :size="6">
          <n-icon :size="14" color="#18a058"><Sparkles /></n-icon>
          <n-text depth="3" class="model-text">
            {{ settings.currentProvider.name }} · {{ settings.currentProvider.selectedModel }}
          </n-text>
        </n-space>
      </n-space>
    </div>
  </div>
</template>

<style scoped lang="scss">
.chat-input-wrapper {
  padding: 20px 32px 24px;
  max-width: 900px;
  margin: 0 auto;
}

.input-container {
  display: flex;
  gap: 12px;
  align-items: flex-end;
  background: var(--n-color-embed);
  border-radius: 20px;
  padding: 12px 16px;
  border: 1px solid var(--n-border-color);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
  transition: all 0.2s ease;
}

.input-container:focus-within {
  border-color: rgba(24, 160, 88, 0.5);
  box-shadow: 0 4px 20px rgba(24, 160, 88, 0.1);
}

.input-box {
  flex: 1;
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
  color: var(--n-text-color-1);
  font-size: 15px;
  line-height: 1.6;
  resize: none;
  font-family: inherit;
  outline: none;
}

.chat-input::placeholder {
  color: var(--n-text-color-3);
}

.chat-input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.input-actions {
  display: flex;
  align-items: center;
  padding-bottom: 2px;
}

.send-btn {
  box-shadow: 0 4px 12px rgba(24, 160, 88, 0.3);
  transition: all 0.2s;
}

.send-btn:not(:disabled):hover {
  transform: scale(1.05);
  box-shadow: 0 6px 16px rgba(24, 160, 88, 0.4);
}

.send-btn:disabled {
  opacity: 0.4;
}

.input-footer {
  display: flex;
  justify-content: center;
  margin-top: 12px;
}

.hint-text {
  font-size: 12px;
  display: flex;
  align-items: center;
}

.divider {
  font-size: 12px;
  opacity: 0.5;
}

.model-text {
  font-size: 12px;
  color: #18a058;
}
</style>
