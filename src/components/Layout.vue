<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  Layout.vue - 应用布局组件

  功能说明:
  - 应用主布局结构 (左侧边栏 + 主内容区)
  - 编辑设计风格导航: 两位数序号 + 名称, 激活项黑白反色
  - 新建对话按钮 (黑底白字, 悬浮反色)

  设计说明:
  - 侧边栏与主区以 1px 黑色细线分隔
  - 底部装饰: 旋转外框 + 轨道圆点
-->

<script setup lang="ts">
// 导入 Vue 相关功能
import { computed, onMounted, onBeforeUnmount, watch } from "vue";

// 导入 Vue Router 相关功能
import { useRoute, useRouter } from "vue-router";

// 导入 Tauri 窗口 API (全屏切换)
import { getCurrentWindow } from "@tauri-apps/api/window";

// 导入 NaiveUI 通知 API (左下角统一弹窗机制)
import { useNotification } from "naive-ui";

// 导入 Store
import { useSettingsStore } from "@/stores/settings";
import { useChatStore } from "@/stores/chat";
import { useWorkspaceStore } from "@/stores/workspace";

// 导入快捷键匹配工具
import { eventMatchesAccelerator } from "@/utils/hotkey";

// 导入自动更新检测
import { checkForAppUpdate } from "@/utils/updater";

// 导入 Logo 图片
import logoImg from "../../assets/logo.png";

// ============ 响应式数据 ============

// 当前路由信息
const route = useRoute();

// 路由导航
const router = useRouter();

// 设置 Store
const settings = useSettingsStore();

// 聊天 Store
const chat = useChatStore();

// 协作团队 Store：事件监听在这里（App 布局层）注册，而不是等用户第一次打开
// 协作团队页面——否则 Agent 在别的页面提问/申请审批时整个应用毫无反应，
// 后端只能干等 10 分钟超时。
const workspace = useWorkspaceStore();

// 所有工作组加总的待人工处理事项数：侧边栏徽标 + 新增时左下角弹提醒。
const workspacePendingCount = computed(
  () =>
    workspace.proposals.length +
    workspace.sleepRequests.length +
    workspace.roundsRequests.length +
    workspace.questions.length +
    workspace.toolApprovals.length
);

// 当前激活的菜单项
const activeKey = computed(() => route.name as string);

// 导航菜单配置: 序号 + 英文 label + 中文名
const navItems = [
  { key: "Chat", label: "Chat", name: "对话" },
  { key: "Skills", label: "Skill", name: "技能" },
  { key: "KnowledgeBase", label: "RAG", name: "知识库" },
  { key: "MCP", label: "MCP", name: "模型工具" },
  { key: "LocalDeploy", label: "Local", name: "本地部署" },
  { key: "AgentTeam", label: "Agents", name: "协作团队" },
  { key: "Scheduler", label: "Cron", name: "定时任务" },
  { key: "History", label: "History", name: "历史记录" },
  { key: "Settings", label: "Settings", name: "设置" },
];

// 菜单项点击处理
const handleNavClick = (key: string) => {
  router.push({ name: key });
};

// 新建对话按钮点击处理
const handleNewChat = () => {
  if (!settings.activeConfigId) {
    // No API config, redirect to settings
    router.push({ name: "Settings" });
    return;
  }
  chat.createSession(settings.activeConfigId);
  router.push({ name: "Chat" });
};

// 应用内"新建会话"快捷键（在 Settings 里可改）。只在应用窗口获得焦点时
// 生效——不同于托盘唤起快捷键那种要注册进操作系统的全局快捷键，这个
// 纯前端监听即可，逻辑和按钮点击完全一致。
const handleNewSessionHotkey = (e: KeyboardEvent) => {
  if (!eventMatchesAccelerator(e, settings.newSessionHotkey)) return;
  e.preventDefault();
  handleNewChat();
};

// 应用内"切换全屏"快捷键（默认 F11，在 Settings 里可改）
const handleFullscreenHotkey = async (e: KeyboardEvent) => {
  if (!eventMatchesAccelerator(e, settings.fullscreenHotkey)) return;
  e.preventDefault();
  const win = getCurrentWindow();
  const isFullscreen = await win.isFullscreen();
  await win.setFullscreen(!isFullscreen);
};

// 通知 API (供更新提示弹窗使用)
const notification = useNotification();

// 待处理事项新增、且用户没停在协作团队页面时，左下角弹一条提醒（在页面上
// 时卡片本身就是提醒，不重复弹）。
watch(workspacePendingCount, (count, prev) => {
  if (count > prev && route.name !== "AgentTeam") {
    notification.info({
      title: "协作团队有新的待处理事项",
      content: `当前共有 ${count} 件事项等待你处理（Agent 提议 / 休眠申请 / 提问 / 工具审批）。`,
      duration: 8000,
    });
  }
});

// 会话/消息写入数据库失败时，chat store 拿不到弹窗上下文，把提醒塞进队列，
// 这里全局 watch 后弹出并清空——避免用户以为已保存，其实静默丢失了记录。
watch(
  () => chat.dbSaveErrorNotices.length,
  () => {
    while (chat.dbSaveErrorNotices.length > 0) {
      const msg = chat.dbSaveErrorNotices.shift();
      if (msg) {
        notification.error({ title: "本地保存失败", description: msg, duration: 6000 });
      }
    }
  }
);

// 托盘/快捷键设置同步到后端失败时，同样走队列弹窗，别让用户以为设置已生效。
watch(
  () => settings.syncErrorNotices.length,
  () => {
    while (settings.syncErrorNotices.length > 0) {
      const msg = settings.syncErrorNotices.shift();
      if (msg) {
        notification.warning({ title: "设置未能同步", description: msg, duration: 6000 });
      }
    }
  }
);

onMounted(async () => {
  window.addEventListener("keydown", handleNewSessionHotkey);
  window.addEventListener("keydown", handleFullscreenHotkey);
  // 延迟几秒再检测更新，避免和启动初始化抢时间
  setTimeout(() => {
    void checkForAppUpdate(notification);
  }, 3000);
  // 全局注册协作团队事件监听（幂等，AgentTeamView 里再调用也不会重复注册）
  await workspace.initListeners();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", handleNewSessionHotkey);
  window.removeEventListener("keydown", handleFullscreenHotkey);
});
</script>

<template>
  <div class="layout">
    <!-- Sidebar -->
    <aside class="sidebar">
      <!-- Logo -->
      <div class="logo-section">
        <div class="logo rotating-frame">
          <img
            :src="logoImg"
            alt="BaiyuAI"
            class="logo-img"
          >
        </div>
        <div class="logo-meta">
          <span class="eyebrow">Workspace</span>
          <span class="logo-text">BaiyuAI</span>
        </div>
      </div>

      <!-- New Chat Button -->
      <button
        class="new-chat-btn"
        @click="handleNewChat"
      >
        <span class="new-chat-label">New Session</span>
        <span class="new-chat-text">新建对话</span>
      </button>

      <!-- Navigation -->
      <nav class="nav">
        <button
          v-for="(item, index) in navItems"
          :key="item.key"
          class="nav-item"
          :class="{ 'is-active': activeKey === item.key }"
          @click="handleNavClick(item.key)"
        >
          <span class="nav-index">{{ String(index + 1).padStart(2, "0") }}</span>
          <span class="nav-name">{{ item.name }}</span>
          <span
            v-if="item.key === 'AgentTeam' && workspacePendingCount > 0"
            class="nav-badge"
          >{{ workspacePendingCount > 99 ? "99+" : workspacePendingCount }}</span>
          <span class="nav-label">{{ item.label }}</span>
        </button>
      </nav>

      <!-- Footer decoration -->
      <div class="sidebar-footer">
        <div class="footer-orbit orbit-ring" />
        <span class="footer-note">Baiyu AI Space — Monochrome</span>
      </div>
    </aside>

    <!-- Main Content -->
    <main class="main-area">
      <router-view v-slot="{ Component }">
        <keep-alive>
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
  </div>
</template>

<style scoped lang="scss">
.layout {
  height: 100vh;
  width: 100vw;
  display: flex;
  background: $bg;
}

.sidebar {
  width: $sidebar-width;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: $border;
  background: $bg;
  padding: 2rem 0 1.5rem;
  overflow: hidden;
}

// ---------- Logo ----------
.logo-section {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0 1.5rem;
  margin-bottom: 2rem;
}

.logo {
  width: 44px;
  height: 44px;
  flex-shrink: 0;
  border: $border;
  padding: 4px;

  &.rotating-frame::before {
    inset: -8px;
  }
}

.logo-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  // 强制黑白: logo 也纳入单色系统
  filter: grayscale(1) contrast(1.1);
}

.logo-meta {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
}

.logo-text {
  font-family: $font-serif;
  font-size: 1.25rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

// ---------- 新建对话 ----------
.new-chat-btn {
  margin: 0 1.5rem 2rem;
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.1rem;
  cursor: pointer;
  background: $ink;
  color: $bg;
  border: $border;
  text-align: left;
  transition:
    background $duration $ease,
    color $duration $ease;

  &:hover {
    background: $bg;
    color: $ink;
  }
}

.new-chat-label {
  font-size: 0.65rem;
  font-weight: 500;
  letter-spacing: $label-tracking;
  text-transform: uppercase;
  opacity: 0.6;
}

.new-chat-text {
  font-family: $font-serif;
  font-size: 1rem;
  font-weight: 700;
}

// ---------- 导航 ----------
.nav {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  border-top: $border-faint;
}

.nav-item {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
  padding: 0.8rem 1.5rem;
  background: transparent;
  border: none;
  border-bottom: $border-faint;
  cursor: pointer;
  text-align: left;
  color: $ink-soft;
  transition:
    background $duration $ease,
    color $duration $ease,
    padding-left $duration $ease;

  &:hover {
    background: $surface;
    color: $ink;
    padding-left: 1.9rem;
  }

  // 激活项: 黑白反色
  &.is-active {
    background: $ink;
    color: $bg;

    .nav-index,
    .nav-label {
      color: inherit;
      opacity: 0.55;
    }
  }
}

.nav-index {
  font-family: $font-mono;
  font-size: 0.7rem;
  color: $ink-faint;
  transition: color $duration $ease;
}

.nav-name {
  font-family: $font-serif;
  font-size: 0.95rem;
  font-weight: 700;
  flex: 1;
}

.nav-label {
  font-size: 0.65rem;
  font-weight: 500;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: $ink-faint;
  transition: color $duration $ease;
}

// 协作团队待处理事项计数徽标：黑底白字直角小方块，激活项（黑底）上反转。
.nav-badge {
  font-family: $font-mono;
  font-size: 0.65rem;
  line-height: 1;
  padding: 2px 5px;
  background: $ink;
  color: $bg;
  flex-shrink: 0;
}

.nav-item.is-active .nav-badge {
  background: $bg;
  color: $ink;
}

// ---------- 底部装饰 ----------
.sidebar-footer {
  padding: 1.25rem 1.5rem 0;
  border-top: $border-faint;
  display: flex;
  align-items: center;
  gap: 1rem;
}

.footer-orbit {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  border: 1px solid rgba(0, 0, 0, 0.4);
  border-radius: 50%;

  &::after {
    --orbit-radius: 14px;
    width: 4px;
    height: 4px;
    margin: -2px 0 0 -2px;
  }
}

.footer-note {
  font-size: 0.65rem;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: $ink-faint;
}

// ---------- 主内容区 ----------
.main-area {
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: hidden;
  background: $bg;
}
</style>
