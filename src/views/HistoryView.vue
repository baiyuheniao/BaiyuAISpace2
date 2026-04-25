<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  HistoryView.vue - 历史记录视图组件
  
  功能说明:
  - 显示所有历史聊天会话列表
  - 支持点击会话进入聊天界面
  - 支持删除历史会话
  - 显示会话元信息 (模型、消息数最后更新时间)

  主要组成部分:
  - 页面标题区域
  - 加载状态
  - 空状态提示
  - 会话列表 (可点击进入、悬停显示删除按钮)
-->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useRouter } from "vue-router";
import { NLayout, NLayoutContent, NEmpty, NList, NListItem, NThing, NTag, NText, NButton, NIcon, NSpin } from "naive-ui";
import { useChatStore } from "@/stores/chat";
import { ChatbubblesOutline, TimeOutline, TrashOutline, EnterOutline } from "@vicons/ionicons5";

// ============ 路由和状态管理 ============

// Vue Router - 用于导航到聊天页面
const router = useRouter();

// 聊天 Store - 管理会话列表
const chat = useChatStore();

// ============ 本地状态 ============

/** 加载状态 - 显示加载动画 */
const loading = ref(false);

// ============ 方法函数 ============

/**
 * 加载会话列表
 * 从数据库获取所有历史会话
 */
const loadSessions = async () => {
  loading.value = true;
  await chat.loadSessionsFromDb();
  loading.value = false;
};

/**
 * 处理会话点击事件
 * 加载选中的会话并跳转到聊天页面
 * 
 * @param session - 要加载的会话对象
 */
const handleSessionClick = async (session: typeof chat.sessions[0]) => {
  await chat.loadSession(session);
  // 跳转到聊天页面
  router.push({ name: "Chat" });
};

/**
 * 处理删除会话
 * 删除指定 ID 的会话
 * 
 * @param sessionId - 要删除的会话 ID
 * @param event - 鼠标事件对象 (用于阻止冒泡)
 */
const handleDelete = async (sessionId: string, event: MouseEvent) => {
  // 阻止事件冒泡，避免触发会话点击
  event.stopPropagation();
  await chat.deleteSession(sessionId);
};

/**
 * 格式化时间戳为可读字符串
 * 根据时间差返回不同的格式:
 * - 今天: HH:mm
 * - 昨天: 昨天
 * - 7天内: X 天前
 * - 更早: MM月DD日
 * 
 * @param timestamp - Unix 时间戳 (毫秒)
 * @returns 格式化后的时间字符串
 */
const formatDate = (timestamp: number) => {
  const date = new Date(timestamp);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  const days = Math.floor(diff / (1000 * 60 * 60 * 24));

  // 今天 - 显示时间
  if (days === 0) {
    return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } 
  // 昨天
  else if (days === 1) {
    return "昨天";
  } 
  // 7天内
  else if (days < 7) {
    return `${days} 天前`;
  } 
  // 更早
  else {
    return date.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
  }
};

// ============ 生命周期钩子 ============

/**
 * 组件挂载时加载会话列表
 */
onMounted(() => {
  loadSessions();
});
</script>

<template>
  <!-- 历史记录主布局容器 -->
  <n-layout class="history-view">
    <!-- 历史记录内容区域 -->
    <n-layout-content
      :native-scrollbar="false"
      class="history-content"
    >
      <div class="history-container">
        <!-- 页面标题 -->
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px;"
          >
            <TimeOutline />
          </n-icon>
          历史记录
        </h1>

        <!-- 加载状态 -->
        <div
          v-if="loading"
          class="loading-state"
        >
          <n-spin size="large" />
        </div>

        <!-- 空状态 - 没有历史记录时显示 -->
        <div
          v-else-if="chat.sessions.length === 0"
          class="empty-state"
        >
          <n-empty description="暂无历史对话">
            <!-- 空状态图标 -->
            <template #icon>
              <n-icon
                :size="64"
                depth="3"
              >
                <ChatbubblesOutline />
              </n-icon>
            </template>
            <!-- 提示文本 -->
            <template #extra>
              <n-text
                depth="3"
                style="margin-top: 16px; display: block;"
              >
                开始一个新对话，历史记录将显示在这里
              </n-text>
            </template>
          </n-empty>
        </div>

        <!-- 会话列表 -->
        <n-list
          v-else
          class="history-list"
          hoverable
          clickable
        >
          <!-- 遍历显示每个会话 -->
          <n-list-item
            v-for="session in chat.sessions"
            :key="session.id"
            class="history-item"
            @click="handleSessionClick(session)"
          >
            <n-thing>
              <!-- 会话标题 -->
              <template #header>
                <span class="session-title">{{ session.title }}</span>
              </template>
              
              <!-- 会话描述 - 显示元信息 -->
              <template #description>
                <n-space
                  align="center"
                  :size="12"
                  class="session-meta"
                >
                  <!-- 提供商标签 -->
                  <n-tag
                    size="small"
                    type="success"
                    class="provider-tag"
                  >
                    {{ session.provider }}
                  </n-tag>
                  <!-- 模型名称 -->
                  <n-text
                    depth="3"
                    class="model-text"
                  >
                    {{ session.model }}
                  </n-text>
                  <!-- 消息数量 -->
                  <n-text
                    depth="3"
                    class="message-count"
                  >
                    {{ session.messages.length }} 条消息
                  </n-text>
                </n-space>
              </template>
              
              <!-- 右侧操作区域 -->
              <template #header-extra>
                <n-space
                  align="center"
                  :size="16"
                >
                  <!-- 更新时间 -->
                  <n-text
                    depth="3"
                    class="time-text"
                  >
                    {{ formatDate(session.updatedAt) }}
                  </n-text>
                  <!-- 删除按钮 (悬停时显示) -->
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
            <!-- 进入提示 (悬停时显示) -->
            <div class="enter-hint">
              <n-icon :size="14">
                <EnterOutline />
              </n-icon>
            </div>
          </n-list-item>
        </n-list>
      </div>
    </n-layout-content>
  </n-layout>
</template>

<style scoped lang="scss">
/* 历史记录主容器 */
.history-view {
  height: 100%;
  background: var(--n-color);
}

/* 内容区域 */
.history-content {
  height: 100%;
}

/* 内容容器 - 限制最大宽度并居中 */
.history-container {
  max-width: 800px;
  margin: 0 auto;
  padding: 40px 32px;
}

/* 页面标题样式 */
.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

/* 加载状态 - 垂直居中 */
.loading-state {
  display: flex;
  justify-content: center;
  padding: 80px;
}

/* 空状态 */
.empty-state {
  padding: 80px 40px;
  display: flex;
  justify-content: center;
}

/* 会话列表背景 */
.history-list {
  background: transparent;
}

/* 会话项样式 */
.history-item {
  margin-bottom: 12px;
  border-radius: 12px;
  background: var(--n-color-embed);
  border: 1px solid transparent;
  // 过渡动画效果
  transition: all 0.2s ease;
  // 相对定位 (用于提示定位)
  position: relative;
  overflow: hidden;
}

/* 会话项悬停效果 */
.history-item:hover {
  border-color: rgba(0, 0, 0, 0.2);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
  // 向右轻微移动
  transform: translateX(4px);
}

/* 会话标题样式 */
.session-title {
  font-weight: 600;
  font-size: 15px;
  color: var(--n-text-color-1);
}

/* 会话元信息间距 */
.session-meta {
  margin-top: 8px;
}

/* 提供商标签字体 */
.provider-tag {
  font-size: 11px;
}

/* 模型名称字体 */
.model-text {
  font-size: 13px;
}

/* 消息数量字体 */
.message-count {
  font-size: 13px;
}

/* 时间文字样式 */
.time-text {
  font-size: 13px;
  // 防止换行
  white-space: nowrap;
}

/* 删除按钮 - 默认隐藏 */
.delete-btn {
  opacity: 0;
  transition: opacity 0.2s;
}

/* 悬停时显示删除按钮 */
.history-item:hover .delete-btn {
  opacity: 1;
}

/* 进入提示 - 悬停时显示 */
.enter-hint {
  position: absolute;
  right: 16px;
  top: 50%;
  transform: translateY(-50%);
  opacity: 0;
  transition: opacity 0.2s;
  color: var(--n-text-color-3);
}

/* 悬停时显示进入提示 */
.history-item:hover .enter-hint {
  opacity: 1;
}
</style>
