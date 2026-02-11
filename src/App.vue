<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { onMounted } from "vue";
import { darkTheme, type GlobalTheme, NConfigProvider, NDialogProvider, NMessageProvider, NNotificationProvider } from "naive-ui";
import { useSettingsStore } from "@/stores/settings";
import Layout from "@/components/Layout.vue";

const settings = useSettingsStore();

const getTheme = (): GlobalTheme | null => {
  return settings.darkMode ? darkTheme : null;
};

onMounted(() => {
  settings.initTheme();
});
</script>

<template>
  <n-config-provider :theme="getTheme()" class="full-height">
    <n-dialog-provider>
      <n-message-provider>
        <n-notification-provider>
          <Layout />
        </n-notification-provider>
      </n-message-provider>
    </n-dialog-provider>
  </n-config-provider>
</template>

<style>
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
  border-radius: 3px;
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(128, 128, 128, 0.5);
}

/* Selection color */
::selection {
  background: rgba(24, 160, 88, 0.3);
}
</style>
