<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatView.vue - 聊天主视图组件

  功能说明:
  - 显示当前会话的消息列表
  - 处理消息滚动和自动定位
  - 在没有消息时显示空状态引导
  - 提供消息输入区域

  组成部分:
  - 消息列表区域 (messages-area)
  - 空状态提示 (empty-state, 编辑排版风格)
  - 消息输入区域 (input-area)
-->

<script setup lang="ts">
import { ref, watch, nextTick, onMounted, computed } from "vue";
import { NButton, NModal, NRadio, NRadioGroup, NText } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore } from "@/stores/settings";
import ChatMessage from "@/components/ChatMessage.vue";
import ChatInput from "@/components/ChatInput.vue";
import TokenCount from "@/components/TokenCount.vue";

// ============ 状态管理 ============

// 聊天 Store - 管理会话和消息状态
const chat = useChatStore();
const pendingWorkingDirectory = ref<string | null>(null);
const pendingFileAccessMode = ref<"read" | "write">("write");
const showFileAccessModeModal = ref(false);

// 设置 Store - 管理 API 配置
const settings = useSettingsStore();

// 消息滚动容器 DOM 引用 - 用于滚动定位
const messagesContainer = ref<HTMLDivElement | null>(null);

// ============ 计算属性 ============

/**
 * 判断当前是否有消息
 * 用于切换显示消息列表或空状态
 */
const hasMessages = computed(() => {
  return chat.currentSession && chat.currentSession.messages.length > 0;
});

/** 已完成交互由服务端返回的实际总用量；未返回 usage 的旧会话不纳入。 */
const sessionTokenCount = computed(() =>
  (chat.currentSession?.messages ?? []).reduce(
    (total, message) => total + (message.tokenUsage?.totalTokens ?? 0),
    0,
  )
);

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
  const session = chat.currentSession;
  if (!session?.workingDirectory) return;
  const mode = session.fileAccessMode === "write" ? "read" : session.fileAccessMode === "read" ? "none" : "write";
  await chat.setWorkingDirectory(session.workingDirectory, mode);
};

// ============ 方法函数 ============

/**
 * 滚动到消息底部
 * 在新消息到达或组件挂载时调用
 * 使用 nextTick 确保 DOM 更新后再执行滚动
 */
const scrollToBottom = async () => {
  await nextTick();
  if (messagesContainer.value) {
    messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight;
  }
};

// ============ 响应式监听 ============

/**
 * 监听消息数量变化
 * 当消息数量变化时自动滚动到底部
 */
watch(
  () => chat.currentSession?.messages.length,
  () => scrollToBottom(),
  { immediate: true }
);

// ============ 生命周期钩子 ============

/**
 * 组件挂载时的初始化逻辑
 * 1. 从数据库加载历史会话
 * 2. 如果没有当前会话且有活跃配置，创建新会话
 * 3. 如果有当前会话，设置流式响应监听
 */
onMounted(async () => {
  // 从数据库加载所有会话列表
  await chat.loadSessionsFromDb();
  console.log("[ChatView] loadSessionsFromDb done, currentSession:", chat.currentSession?.id, "messages:", chat.currentSession?.messages?.length);

  // 如果没有当前选中的会话
  if (!chat.currentSession) {
    // 检查是否有激活的 API 配置，有则创建新会话
    if (settings.activeConfigId) {
      await chat.createSession(settings.activeConfigId);
    }
  } else {
    // 为已有会话设置流式响应监听器
    await chat.loadSession(chat.currentSession);
  }
  console.log("[ChatView] setup done, currentSession:", chat.currentSession?.id, "messages:", chat.currentSession?.messages?.length);
});
</script>

<template>
  <!-- 聊天主布局容器 -->
  <div class="chat-view">
    <div v-if="chat.currentSession" class="working-directory-bar">
      <span>工作目录：{{ chat.currentSession.workingDirectory || "尚未选择" }}</span>
      <span v-if="chat.currentSession.workingDirectory">· {{ chat.currentSession.fileAccessMode === "write" ? "可编辑" : chat.currentSession.fileAccessMode === "read" ? "只读" : "无文件权限" }}</span>
      <button type="button" @click="chooseWorkingDirectory">{{ chat.currentSession.workingDirectory ? "更换目录" : "选择目录" }}</button>
      <button v-if="chat.currentSession.workingDirectory" type="button" @click="cycleFileAccess">切换权限</button>
      <button v-if="chat.currentSession.workingDirectory" type="button" @click="chat.setWorkingDirectory(null)">移除</button>
    </div>
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
    <div
      v-if="chat.currentSession"
      class="session-token-bar"
    >
      <span class="session-token-eyebrow">Session Usage</span>
      <TokenCount
        label="累计"
        :count="sessionTokenCount"
        :exact="true"
        description="已完成交互的 API 实际 total_tokens 累计；未返回 Usage 的交互不计入"
      />
    </div>

    <!-- 消息区域 (滚动容器) -->
    <div
      ref="messagesContainer"
      class="messages-area"
    >
      <!-- 有消息时显示消息列表 -->
      <div
        v-if="hasMessages"
        class="messages-container"
      >
        <!-- 遍历渲染每条消息 -->
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
        <!-- SVG 线框背景层 -->
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
          <!-- 区块前缀 label -->
          <span class="eyebrow">New Session</span>

          <!-- 主标题 -->
          <h2 class="empty-title">
            开始新的对话
          </h2>

          <!-- 副标题 - 显示当前使用的模型信息 -->
          <p class="empty-desc">
            <!-- 如果有激活配置，显示配置信息 -->
            <template v-if="settings.activeConfig">
              使用 <n-text code>
                {{ settings.activeConfig.name }}
              </n-text> 的
              <n-text code>
                {{ settings.activeConfig.model }}
              </n-text> 模型
            </template>
            <!-- 如果没有配置，引导用户去设置 -->
            <template v-else>
              请先前往设置创建 API 配置
            </template>
          </p>

          <!-- 几何装饰: 轨道圆点 -->
          <div class="empty-orbit orbit-ring" />
        </div>
      </div>
    </div>

    <!-- 输入区域 - 固定在底部 -->
    <footer class="input-area">
      <!-- 消息输入组件 -->
      <ChatInput />
    </footer>
  </div>
</template>

<style scoped lang="scss">
/* 聊天主容器样式 */
.chat-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: $bg;
}

.session-token-bar {
  min-height: 36px;
  padding: 0 32px;
  border-bottom: $border-faint;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  background: $bg;
}

.working-directory-bar {
  min-height: 36px;
  padding: 0 32px;
  border-bottom: $border-faint;
  display: flex;
  align-items: center;
  gap: 10px;
  font-family: $font-mono;
  font-size: 12px;

  button {
    border: $border;
    border-radius: 0;
    background: $bg;
    color: $ink;
    padding: 3px 8px;
    font: inherit;
    cursor: pointer;
    &:hover { background: $ink; color: $bg; }
  }
}

.session-token-eyebrow {
  color: $ink-faint;
  font-family: $font-sans;
  font-size: 10px;
  font-weight: 500;
  letter-spacing: $label-tracking;
  text-transform: uppercase;
}

/* 消息区域 - 占据剩余空间并承担滚动 */
.messages-area {
  flex: 1;
  overflow-y: auto;
  position: relative;
}

/* 消息容器 - 限制最大宽度并居中 */
.messages-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 24px 32px;
}

/* 空状态容器 - 垂直水平居中 */
.empty-state {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
}

/* 空状态内容区域 */
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

/* 空状态主标题 */
.empty-title {
  font-family: $font-serif;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

/* 空状态描述 */
.empty-desc {
  font-size: 0.95rem;
  line-height: $leading-body;
  color: $ink-soft;
}

/* 轨道圆点装饰 */
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

/* 输入区域样式 */
.input-area {
  background: $bg;
  border-top: $border;
  padding: 0;
}
</style>
