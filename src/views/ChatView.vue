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
  - 空状态提示 (empty-state)
  - 消息输入区域 (input-area)
-->

<script setup lang="ts">
import { ref, watch, nextTick, onMounted, computed } from "vue";
import { NLayout, NLayoutContent, NLayoutFooter, NIcon, NText } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { useSettingsStore } from "@/stores/settings";
import ChatMessage from "@/components/ChatMessage.vue";
import ChatInput from "@/components/ChatInput.vue";
import { Sparkles, ChatbubblesOutline } from "@vicons/ionicons5";

// ============ 状态管理 ============

// 聊天 Store - 管理会话和消息状态
const chat = useChatStore();

// 设置 Store - 管理 API 配置
const settings = useSettingsStore();

// 消息容器 DOM 引用 - 用于滚动定位
const messagesContainer = ref<HTMLDivElement | null>(null);

// ============ 计算属性 ============

/**
 * 判断当前是否有消息
 * 用于切换显示消息列表或空状态
 */
const hasMessages = computed(() => {
  return chat.currentSession && chat.currentSession.messages.length > 0;
});

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
});
</script>

<template>
  <!-- 聊天主布局容器 -->
  <n-layout class="chat-view">
    <!-- 消息区域 -->
    <n-layout-content class="messages-area" :native-scrollbar="false">
      <!-- 有消息时显示消息列表 -->
      <div v-if="hasMessages" ref="messagesContainer" class="messages-container">
        <!-- 遍历渲染每条消息 -->
        <ChatMessage
          v-for="message in chat.currentSession?.messages"
          :key="message.id"
          :message="message"
        />
      </div>

      <!-- 空状态 - 没有消息时显示 -->
      <div v-else class="empty-state">
        <div class="empty-content">
          <!-- 空状态图标 (闪烁动画) -->
          <div class="empty-icon">
            <n-icon :size="80" depth="3">
              <Sparkles />
            </n-icon>
          </div>
          
          <!-- 主标题 -->
          <h2 class="empty-title">开始新的对话</h2>
          
          <!-- 副标题 - 显示当前使用的模型信息 -->
          <p class="empty-desc">
            <!-- 如果有激活配置，显示配置信息 -->
            <template v-if="settings.activeConfig">
              使用 <n-text code>{{ settings.activeConfig.name }}</n-text> 的 
              <n-text code>{{ settings.activeConfig.model }}</n-text> 模型
            </template>
            <!-- 如果没有配置，引导用户去设置 -->
            <template v-else>
              请先前往设置创建 API 配置
            </template>
          </p>
          
          <!-- 功能提示 -->
          <div class="empty-tips">
            <div class="tip-item">
              <n-icon><ChatbubblesOutline /></n-icon>
              <span>支持 Markdown 和代码高亮</span>
            </div>
          </div>
        </div>
      </div>
    </n-layout-content>

    <!-- 输入区域 - 固定在底部 -->
    <n-layout-footer class="input-area" bordered>
      <!-- 消息输入组件 -->
      <ChatInput />
    </n-layout-footer>
  </n-layout>
</template>

<style scoped lang="scss">
/* 聊天主容器样式 */
.chat-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--n-color);
}

/* 消息区域 - 占据剩余空间 */
.messages-area {
  flex: 1;
  overflow: hidden;
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
}

/* 空状态内容区域 */
.empty-content {
  text-align: center;
  padding: 40px;
}

/* 空状态图标样式 */
.empty-icon {
  margin-bottom: 24px;
  color: var(--n-text-color-3);
  // 浮动动画效果
  animation: float 3s ease-in-out infinite;
}

// 浮动关键帧动画
@keyframes float {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-10px); }
}

/* 空状态主标题 */
.empty-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 12px;
  color: var(--n-text-color-1);
