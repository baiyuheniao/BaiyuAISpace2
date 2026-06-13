<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  App.vue - 应用根组件
  
  功能说明:
  - 应用全局配置 (主题、NaiveUI 组件)
  - 布局组件加载
  - 初始化设置 (主题、API 密钥加载)
  
  组成部分:
  - n-config-provider: NaiveUI 全局配置 (主题)
  - n-dialog-provider: 对话框服务
  - n-message-provider: 消息提示服务
  - n-notification-provider: 通知服务
  - Layout: 主布局组件
-->

<script setup lang="ts">
// 导入 Vue 相关功能
import { onMounted, computed } from "vue";

// 导入 NaiveUI 组件和类型
import { darkTheme, type GlobalTheme, NConfigProvider, NDialogProvider, NMessageProvider, NNotificationProvider } from "naive-ui";

// 导入 Store
import { useSettingsStore } from "@/stores/settings";

// 导入布局组件
import Layout from "@/components/Layout.vue";

// ============ 响应式数据 ============

// 设置 Store
const settings = useSettingsStore();

// 当前主题 (深色/浅色)
const currentTheme = computed<GlobalTheme | null>(() => {
  return settings.darkMode ? darkTheme : null;
});

// ============ 生命周期钩子 ============

// 组件挂载时的初始化
onMounted(async () => {
  // 初始化主题设置
  settings.initTheme();
  // 从安全存储加载所有 API 密钥
  await settings.loadAllApiKeys();
});
</script>

<template>
  <n-config-provider
    :theme="currentTheme"
    class="full-height"
  >
    <n-dialog-provider>
      <n-message-provider>
        <n-notification-provider>
          <Layout />
        </n-notification-provider>
      </n-message-provider>
    </n-dialog-provider>
  </n-config-provider>
</template>

<style lang="scss">
/* Reset and base styles */
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body, #app {
  height: 100%;
  width: 100%;
  overflow: hidden;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans SC", sans-serif;
}

.full-height {
  height: 100%;
}

/* Scrollbar styles */
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: rgba(128, 128, 128, 0.3);
  border-radius: $radius-sm;
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(128, 128, 128, 0.5);
}

/* Selection color */
::selection {
  background: rgba(0, 0, 0, 0.2);
}
</style>
