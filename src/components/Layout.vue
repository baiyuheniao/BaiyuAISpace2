<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed, h } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NLayout, NLayoutSider, NMenu, NButton, NAvatar, NTooltip } from "naive-ui";
import type { MenuOption } from "naive-ui";
import { useSettingsStore } from "@/stores/settings";
import { useChatStore } from "@/stores/chat";
import { Chatbubbles, Time, Settings, Moon, Sunny, Add, Sparkles } from "@vicons/ionicons5";

const route = useRoute();
const router = useRouter();
const settings = useSettingsStore();
const chat = useChatStore();

const activeKey = computed(() => route.name as string);

const menuOptions: MenuOption[] = [
  {
    label: "对话",
    key: "Chat",
    icon: () => h(Chatbubbles),
  },
  {
    label: "历史记录",
    key: "History",
    icon: () => h(Time),
  },
  {
    label: "设置",
    key: "Settings",
    icon: () => h(Settings),
  },
];

const handleMenuUpdate = (key: string) => {
  router.push({ name: key });
};

const handleNewChat = () => {
  chat.createSession(settings.currentProvider.id, settings.currentProvider.selectedModel);
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
            <n-avatar round :size="40" class="logo-avatar">
              <n-icon :size="24"><Sparkles /></n-icon>
            </n-avatar>
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
          <n-space vertical :size="12">
            <!-- Theme Toggle -->
            <n-tooltip placement="right">
              <template #trigger>
                <n-button
                  quaternary
                  round
                  class="action-btn"
                  @click="settings.toggleTheme"
                >
                  <template #icon>
                    <n-icon :size="20">
                      <Sunny v-if="settings.darkMode" />
                      <Moon v-else />
                    </n-icon>
                  </template>
                  <span class="action-text">
                    {{ settings.darkMode ? '浅色模式' : '深色模式' }}
                  </span>
                </n-button>
              </template>
              {{ settings.darkMode ? '切换到浅色模式' : '切换到深色模式' }}
            </n-tooltip>

            <!-- User Info -->
            <div class="user-info">
              <n-avatar round :size="32" class="user-avatar">
                <n-icon :size="18"><Settings /></n-icon>
              </n-avatar>
              <div class="user-text">
                <n-text strong>用户</n-text>
                <n-text depth="3" class="user-status">在线</n-text>
              </div>
            </div>
          </n-space>
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

.logo-avatar {
  background: linear-gradient(135deg, #18a058 0%, #36ad6a 100%);
  box-shadow: 0 4px 12px rgba(24, 160, 88, 0.3);
}

.logo-text {
  font-size: 22px;
  font-weight: 700;
  background: linear-gradient(135deg, #18a058 0%, #36ad6a 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}

.new-chat-section {
  margin-bottom: 20px;
  padding: 0 8px;
}

.new-chat-btn {
  box-shadow: 0 4px 12px rgba(24, 160, 88, 0.3);
  transition: all 0.3s ease;
}

.new-chat-btn:hover {
  transform: translateY(-1px);
  box-shadow: 0 6px 16px rgba(24, 160, 88, 0.4);
}

.menu-section {
  flex: 1;
}

.bottom-section {
  margin-top: auto;
  padding-top: 16px;
  border-top: 1px solid var(--n-border-color);
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
  background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
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
