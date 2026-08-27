<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatView.vue - 聊天主视图组件

  功能说明:
  - 显示当前会话的消息列表
  - 处理消息滚动和自动定位
  - 在没有消息时显示空状态引导
  - 右侧为会话上下文面板 (文件/知识库/MCP/Skill)

  布局:
  - 左: 消息流 + 底部输入区
  - 右: ChatSidePanel
-->

<script setup lang="ts">
import { ref, watch, nextTick, onMounted, computed } from "vue";
import { NText } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore } from "@/stores/settings";
import ChatMessage from "@/components/ChatMessage.vue";
import ChatInput from "@/components/ChatInput.vue";
import ChatSidePanel from "@/components/ChatSidePanel.vue";

// ============ 状态管理 ============

const chat = useChatStore();
const settings = useSettingsStore();

// 消息滚动容器 DOM 引用 - 用于滚动定位
const messagesContainer = ref<HTMLDivElement | null>(null);

// ============ 计算属性 ============

/** 判断当前是否有消息，用于切换显示消息列表或空状态 */
const hasMessages = computed(() => {
  return chat.currentSession && chat.currentSession.messages.length > 0;
});

// ============ 方法函数 ============

/** 滚动到消息底部，在新消息到达或组件挂载时调用 */
const scrollToBottom = async () => {
  await nextTick();
  if (messagesContainer.value) {
    messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight;
  }
};

// ============ 响应式监听 ============

watch(
  () => chat.currentSession?.messages.length,
  () => scrollToBottom(),
  { immediate: true }
);

// ============ 生命周期钩子 ============

onMounted(async () => {
  // 从数据库加载所有会话列表
  await chat.loadSessionsFromDb();

  // 如果没有当前选中的会话
  if (!chat.currentSession) {
    if (settings.activeConfigId) {
      await chat.createSession(settings.activeConfigId);
    }
  } else {
    await chat.loadSession(chat.currentSession);
  }
});
</script>

<template>
  <div class="chat-view">
    <!-- 左侧: 消息流 + 输入区 -->
    <div class="chat-main">
      <!-- 消息区域 (滚动容器) -->
      <div
        ref="messagesContainer"
        class="messages-area"
      >
        <div
          v-if="hasMessages"
          class="messages-container"
          :style="{ maxWidth: `${settings.chatContentWidth}px` }"
        >
          <ChatMessage
            v-for="message in chat.currentSession?.messages"
            :key="message.id"
            :message="message"
          />
        </div>

        <!-- 空状态 - 没有消息时显示 -->
        <div
          v-else
          class="empty-state"
        >
          <div class="bg-wireframe">
            <svg
              viewBox="0 0 800 600"
              preserveAspectRatio="xMidYMid slice"
            >
              <circle
                cx="400"
                cy="300"
                r="220"
                fill="none"
                stroke="var(--color-ink)"
                stroke-width="1"
              />
              <circle
                cx="400"
                cy="300"
                r="140"
                fill="none"
                stroke="var(--color-ink)"
                stroke-width="1"
              />
              <rect
                x="180"
                y="120"
                width="440"
                height="360"
                fill="none"
                stroke="var(--color-ink)"
                stroke-width="1"
              />
              <line
                x1="0"
                y1="300"
                x2="800"
                y2="300"
                stroke="var(--color-ink)"
                stroke-width="1"
              />
              <line
                x1="400"
                y1="0"
                x2="400"
                y2="600"
                stroke="var(--color-ink)"
                stroke-width="1"
              />
            </svg>
          </div>

          <div class="empty-content enter-up">
            <span class="eyebrow">New Session</span>
            <h2 class="empty-title">开始新的对话</h2>
            <p class="empty-desc">
              <template v-if="settings.activeConfig">
                使用
                <n-text code>{{ settings.activeConfig.name }}</n-text>
                的
                <n-text code>{{ settings.activeConfig.model }}</n-text>
                模型
              </template>
              <template v-else>请先前往设置创建 API 配置</template>
            </p>
            <div class="empty-orbit orbit-ring" />
          </div>
        </div>
      </div>

      <!-- 输入区域 - 固定在底部 -->
      <footer class="input-area">
        <ChatInput />
      </footer>
    </div>

    <!-- 右侧: 会话上下文面板 -->
    <ChatSidePanel />
  </div>
</template>

<style scoped lang="scss">
.chat-view {
  height: 100%;
  display: flex;
  background: $bg;
}

.chat-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.messages-area {
  flex: 1;
  overflow-y: auto;
  position: relative;
}

.messages-container {
  margin: 0 auto;
  padding: 24px 32px;
}

.empty-state {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
}

.bg-wireframe {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.35;
  pointer-events: none;

  svg {
    width: 100%;
    height: 100%;
  }
}

.empty-content {
  text-align: center;
  padding: 4rem 5rem;
  border: $border;
  background: $bg;
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1rem;
}

.empty-title {
  font-family: $font-serif;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

.empty-desc {
  font-size: 0.95rem;
  line-height: $leading-body;
  color: $ink-soft;
}

.empty-orbit {
  width: 48px;
  height: 48px;
  margin-top: 1.5rem;
  border: 1px solid var(--color-line-faint);
  border-radius: 50%;

  &::after {
    --orbit-radius: 24px;
  }
}

.input-area {
  background: $bg;
  border-top: $border;
  padding: 0;
}
</style>
