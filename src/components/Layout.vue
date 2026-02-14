<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed, h } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NLayout, NLayoutSider, NMenu, NButton } from "naive-ui";
import type { MenuOption } from "naive-ui";
import { useSettingsStore } from "@/stores/settings";
import { useChatStore } from "@/stores/chat";
import { Chatbubbles, Time, Settings, Add, Library, Cube } from "@vicons/ionicons5";
import logoImg from "../../assets/logo.png";

const route = useRoute();
const router = useRouter();
const settings = useSettingsStore();
const chat = useChatStore();

const activeKey = computed(() => route.name as string);

const menuOptions: MenuOption[] = [
  {
    label: "Chat/对话",
    key: "Chat",
    icon: () => h(Chatbubbles),
  },
  {
    label: "RAG/知识库",
    key: "KnowledgeBase",
    icon: () => h(Library),
  },
  {
    label: "MCP/模型工具",
    key: "MCP",
    icon: () => h(Cube),
  },
  {
    label: "History/历史记录",
    key: "History",
    icon: () => h(Time),
  },
  {
    label: "Settings/设置",
    key: "Settings",
    icon: () => h(Settings),
  },
];

const handleMenuUpdate = (key: string) => {
  router.push({ name: key });
};

const handleNewChat = () => {
  if (!settings.activeConfigId) {
    // No API config, redirect to settings
    router.push({ name: "Settings" });
    return;
  }
  chat.createSession(settings.activeConfigId);
  router.push({ name: "Chat" });
};
</script>

<template>
  <n-layout has-sider class="layout">
    <!-- Sidebar -->
    <n-layout-sider
      bordered
      collapse-mode="width"
      :collapsed-width="72"
      :width="260"
      :native-scrollbar="false"
      class="sidebar"
    >
      <div class="sidebar-content">
        <!-- Logo -->
        <div class="logo-section">
          <div class="logo">
            <img :src="logoImg" alt="BaiyuAI" class="logo-img" />
            <span class="logo-text">BaiyuAI</span>
          </div>
        </div>

        <!-- New Chat Button -->
        <div class="new-chat-section">
          <n-button
            type="primary"
            size="large"
            block
            round
            class="new-chat-btn"
            @click="handleNewChat"
          >
            <template #icon>
              <n-icon><Add /></n-icon>
            </template>
            新建对话
          </n-button>
        </div>

        <!-- Navigation Menu -->
        <div class="menu-section">
          <n-menu
            :value="activeKey"
            :collapsed-width="72"
            :collapsed-icon-size="22"
            :options="menuOptions"
            :root-indent="18"
            :indent="12"
            @update:value="handleMenuUpdate"
          />
        </div>

        <!-- Bottom Actions -->
        <div class="bottom-section">

          <!-- User Info at bottom -->
          <div class="user-info">
            <n-avatar round :size="32" class="user-avatar">
              <n-icon :size="18"><Settings /></n-icon>
            </n-avatar>
            <div class="user-text">
              <n-text strong>用户</n-text>
              <n-text depth="3" class="user-status">在线</n-text>
            </div>
          </div>
        </div>
      </div>
    </n-layout-sider>

    <!-- Main Content -->
    <n-layout class="main-layout">
      <router-view v-slot="{ Component }">
        <keep-alive>
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </n-layout>
  </n-layout>
</template>

<style scoped lang="scss">
.layout {
  height: 100vh;
  width: 100vw;
}

.sidebar {
  background: linear-gradient(180deg, var(--n-color-embed) 0%, var(--n-color) 100%);
}

.sidebar-content {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 16px;
}

.logo-section {
  margin-bottom: 24px;
}

.logo {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 8px;
}

.logo-img {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  object-fit: contain;
  image-rendering: crisp-edges;
  background: var(--n-color-embed);
  padding: 4px;
}

.logo-text {
  font-size: 22px;
  font-weight: 700;
  color: var(--n-text-color);
}

.new-chat-section {
  margin-bottom: 20px;
  padding: 0 8px;
}

.new-chat-btn {
  background: #000000;
  border-color: #000000;
  color: #ffffff;
  transition: all 0.3s ease;
}

.new-chat-btn:hover {
  transform: translateY(-1px);
  background: #1a1a1a;
  border-color: #1a1a1a;
}

.menu-section {
  flex: 1;
}

.bottom-section {
  margin-top: auto;
  padding-top: 16px;
  border-top: 1px solid var(--n-border-color);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.action-btn {
  width: 100%;
  justify-content: flex-start;
  padding: 10px 16px;
}

.action-text {
  margin-left: 12px;
}

.user-info {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--n-color-embed);
  transition: background 0.2s;
}

.user-info:hover {
  background: var(--n-hover-color);
}

.user-avatar {
  background: #000000;
}

.user-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.user-status {
  font-size: 12px;
}

.main-layout {
  background: var(--n-color);
}
</style>
