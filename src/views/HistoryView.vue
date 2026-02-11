<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRouter } from "vue-router";
import { NLayout, NLayoutContent, NEmpty, NList, NListItem, NThing, NTag, NText, NButton, NIcon, NSpin } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { ChatbubblesOutline, TimeOutline, TrashOutline, EnterOutline } from "@vicons/ionicons5";

const router = useRouter();
const chat = useChatStore();

const loading = ref(false);

const loadSessions = async () => {
  loading.value = true;
  await chat.loadSessionsFromDb();
  loading.value = false;
};

const handleSessionClick = async (session: typeof chat.sessions[0]) => {
  await chat.loadSession(session);
  router.push({ name: "Chat" });
};

const handleDelete = async (sessionId: string, event: MouseEvent) => {
  event.stopPropagation();
  await chat.deleteSession(sessionId);
};

const formatDate = (timestamp: number) => {
  const date = new Date(timestamp);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  const days = Math.floor(diff / (1000 * 60 * 60 * 24));

  if (days === 0) {
    return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } else if (days === 1) {
    return "昨天";
  } else if (days < 7) {
    return `${days} 天前`;
  } else {
    return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
  }
};

onMounted(() => {
  loadSessions();
});
</script>

<template>
  <n-layout class="history-view">
    <n-layout-content :native-scrollbar="false" class="history-content">
      <div class="history-container">
        <h1 class="page-title">
          <n-icon :size="28" style="margin-right: 12px;"><TimeOutline /></n-icon>
          历史记录
        </h1>

        <div v-if="loading" class="loading-state">
          <n-spin size="large" />
        </div>

        <div v-else-if="chat.sessions.length === 0" class="empty-state">
          <n-empty description="暂无历史对话">
            <template #icon>
              <n-icon :size="64" depth="3">
                <ChatbubblesOutline />
              </n-icon>
            </template>
            <template #extra>
              <n-text depth="3" style="margin-top: 16px; display: block;">
                开始一个新对话，历史记录将显示在这里
              </n-text>
            </template>
          </n-empty>
        </div>

        <n-list v-else class="history-list" hoverable clickable>
          <n-list-item
            v-for="session in chat.sessions"
            :key="session.id"
            class="history-item"
            @click="handleSessionClick(session)"
          >
            <n-thing>
              <template #header>
                <span class="session-title">{{ session.title }}</span>
              </template>
              <template #description>
                <n-space align="center" :size="12" class="session-meta">
                  <n-tag size="small" type="success" class="provider-tag">
                    {{ session.provider }}
                  </n-tag>
                  <n-text depth="3" class="model-text">{{ session.model }}</n-text>
                  <n-text depth="3" class="message-count">
                    {{ session.messages.length }} 条消息
                  </n-text>
                </n-space>
              </template>
              <template #header-extra>
                <n-space align="center" :size="16">
                  <n-text depth="3" class="time-text">
                    {{ formatDate(session.updatedAt) }}
                  </n-text>
                  <n-button
                    quaternary
                    circle
                    size="small"
                    type="error"
                    class="delete-btn"
                    @click="(e: MouseEvent) => handleDelete(session.id, e)"
                  >
                    <template #icon>
                      <n-icon><TrashOutline /></n-icon>
                    </template>
                  </n-button>
                </n-space>
              </template>
            </n-thing>
            <div class="enter-hint">
              <n-icon :size="14"><EnterOutline /></n-icon>
            </div>
          </n-list-item>
        </n-list>
      </div>
    </n-layout-content>
  </n-layout>
</template>

<style scoped lang="scss">
.history-view {
  height: 100%;
  background: var(--n-color);
}

.history-content {
  height: 100%;
}

.history-container {
  max-width: 800px;
  margin: 0 auto;
  padding: 40px 32px;
}

.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

.loading-state {
  display: flex;
  justify-content: center;
  padding: 80px;
}

.empty-state {
  padding: 80px 40px;
  display: flex;
  justify-content: center;
}

.history-list {
  background: transparent;
}

.history-item {
  margin-bottom: 12px;
  border-radius: 12px;
  background: var(--n-color-embed);
  border: 1px solid transparent;
  transition: all 0.2s ease;
  position: relative;
  overflow: hidden;
}

.history-item:hover {
  border-color: rgba(24, 160, 88, 0.3);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
  transform: translateX(4px);
}

.session-title {
  font-weight: 600;
  font-size: 15px;
  color: var(--n-text-color-1);
}

.session-meta {
  margin-top: 8px;
}

.provider-tag {
  font-size: 11px;
}

.model-text {
  font-size: 13px;
}

.message-count {
  font-size: 13px;
}

.time-text {
  font-size: 13px;
  white-space: nowrap;
}

.delete-btn {
  opacity: 0;
  transition: opacity 0.2s;
}

.history-item:hover .delete-btn {
  opacity: 1;
}

.enter-hint {
  position: absolute;
  right: 16px;
  top: 50%;
  transform: translateY(-50%);
  opacity: 0;
  transition: opacity 0.2s;
  color: var(--n-text-color-3);
}

.history-item:hover .enter-hint {
  opacity: 1;
}
</style>
