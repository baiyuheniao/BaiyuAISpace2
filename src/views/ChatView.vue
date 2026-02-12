<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, watch, nextTick, onMounted, computed } from "vue";
import { NLayout, NLayoutContent, NLayoutFooter, NIcon, NText } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore } from "@/stores/settings";
import ChatMessage from "@/components/ChatMessage.vue";
import ChatInput from "@/components/ChatInput.vue";
import { Sparkles, ChatbubblesOutline } from "@vicons/ionicons5";

const chat = useChatStore();
const settings = useSettingsStore();
const messagesContainer = ref<HTMLDivElement | null>(null);

const hasMessages = computed(() => {
  return chat.currentSession && chat.currentSession.messages.length > 0;
});

const scrollToBottom = async () => {
  await nextTick();
  if (messagesContainer.value) {
    messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight;
  }
};

watch(
  () => chat.currentSession?.messages.length,
  () => scrollToBottom(),
  { immediate: true }
);

onMounted(async () => {
  // Load sessions from database
  await chat.loadSessionsFromDb();
  
  if (!chat.currentSession) {
    // Create session with active API config if available
    if (settings.activeConfigId) {
      await chat.createSession(settings.activeConfigId);
    }
  } else {
    // Setup stream listener for existing session
    await chat.loadSession(chat.currentSession);
  }
});
</script>

<template>
  <n-layout class="chat-view">
    <!-- Messages Area -->
    <n-layout-content class="messages-area" :native-scrollbar="false">
      <div v-if="hasMessages" ref="messagesContainer" class="messages-container">
        <ChatMessage
          v-for="message in chat.currentSession?.messages"
          :key="message.id"
          :message="message"
        />
      </div>

      <!-- Empty State -->
      <div v-else class="empty-state">
        <div class="empty-content">
          <div class="empty-icon">
            <n-icon :size="80" depth="3">
              <Sparkles />
            </n-icon>
          </div>
          <h2 class="empty-title">开始新的对话</h2>
          <p class="empty-desc">
            <template v-if="settings.activeConfig">
              使用 <n-text code>{{ settings.activeConfig.name }}</n-text> 的 
              <n-text code>{{ settings.activeConfig.model }}</n-text> 模型
            </template>
            <template v-else>
              请先前往设置创建 API 配置
            </template>
          </p>
          <div class="empty-tips">
            <div class="tip-item">
              <n-icon><ChatbubblesOutline /></n-icon>
              <span>支持 Markdown 和代码高亮</span>
            </div>
          </div>
        </div>
      </div>
    </n-layout-content>

    <!-- Input Area -->
    <n-layout-footer class="input-area" bordered>
      <ChatInput />
    </n-layout-footer>
  </n-layout>
</template>

<style scoped lang="scss">
.chat-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--n-color);
}

.messages-area {
  flex: 1;
  overflow: hidden;
}

.messages-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 24px 32px;
}

.empty-state {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-content {
  text-align: center;
  padding: 40px;
}

.empty-icon {
  margin-bottom: 24px;
  color: var(--n-text-color-3);
  animation: float 3s ease-in-out infinite;
}

@keyframes float {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-10px); }
}

.empty-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 12px;
  background: linear-gradient(135deg, var(--n-text-color-1) 0%, var(--n-text-color-3) 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.empty-desc {
  font-size: 15px;
  color: var(--n-text-color-3);
  margin-bottom: 32px;
}

.empty-tips {
  display: flex;
  justify-content: center;
  gap: 16px;
}

.tip-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  background: var(--n-color-embed);
  border-radius: 20px;
  font-size: 13px;
  color: var(--n-text-color-2);
  transition: all 0.2s;
}

.tip-item:hover {
  background: var(--n-hover-color);
  transform: translateY(-1px);
}

.input-area {
  background: var(--n-color);
  border-top: 1px solid var(--n-border-color);
  padding: 0;
}
</style>
