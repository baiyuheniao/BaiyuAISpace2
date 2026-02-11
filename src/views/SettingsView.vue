<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed } from "vue";
import { NLayout, NLayoutContent, NCard, NForm, NFormItem, NSelect, NInput, NSwitch, NTag, NText, NIcon } from "naive-ui";
import { useSettingsStore } from "@/stores/settings";
import { ServerOutline, KeyOutline, ColorPaletteOutline, InformationCircleOutline, CheckmarkCircle } from "@vicons/ionicons5";

const settings = useSettingsStore();
const currentProvider = computed(() => settings.currentProvider);

const handleProviderChange = (providerId: string) => {
  settings.setActiveProvider(providerId);
};

const handleApiKeyChange = (value: string) => {
  settings.updateProvider(currentProvider.value.id, { apiKey: value });
};

const handleModelChange = (model: string) => {
  settings.updateProvider(currentProvider.value.id, { selectedModel: model });
};

const providerOptions = computed(() => {
  return settings.providers.map(p => ({
    label: p.name,
    value: p.id,
  }));
});

const modelOptions = computed(() => {
  return currentProvider.value.models.map(m => ({
    label: m,
    value: m,
  }));
});
</script>

<template>
  <n-layout class="settings-view">
    <n-layout-content :native-scrollbar="false" class="settings-content">
      <div class="settings-container">
        <h1 class="page-title">
          <n-icon :size="28" style="margin-right: 12px;"><ServerOutline /></n-icon>
          设置
        </h1>

        <!-- LLM Provider Settings -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><KeyOutline /></n-icon>
              <span>API 配置</span>
            </div>
          </template>

          <n-form label-placement="left" label-width="100px" class="settings-form">
            <n-form-item label="提供商">
              <n-select
                :value="settings.activeProvider"
                :options="providerOptions"
                @update:value="handleProviderChange"
                class="form-select"
              />
            </n-form-item>

            <n-form-item label="API Key">
              <n-input
                type="password"
                show-password-on="click"
                :placeholder="`请输入 ${currentProvider.name} 的 API Key`"
                :value="currentProvider.apiKey"
                @update:value="handleApiKeyChange"
                class="form-input"
              >
                <template #prefix>
                  <n-icon :size="16"><KeyOutline /></n-icon>
                </template>
              </n-input>
              <template #feedback>
                <n-text depth="3" style="font-size: 12px;">
                  <n-icon :size="12" style="margin-right: 4px;"><CheckmarkCircle /></n-icon>
                  API Key 仅存储在本地，使用系统密钥链加密
                </n-text>
              </template>
            </n-form-item>

            <n-form-item label="模型">
              <n-select
                :value="currentProvider.selectedModel"
                :options="modelOptions"
                @update:value="handleModelChange"
                class="form-select"
              />
            </n-form-item>

            <n-form-item label="接口地址">
              <n-input
                :value="currentProvider.baseUrl"
                disabled
                class="form-input"
              />
            </n-form-item>
          </n-form>
        </n-card>

        <!-- Appearance Settings -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><ColorPaletteOutline /></n-icon>
              <span>外观</span>
            </div>
          </template>

          <n-form label-placement="left" label-width="100px" class="settings-form">
            <n-form-item label="深色模式">
              <n-switch
                :value="settings.darkMode"
                @update:value="settings.toggleTheme"
                size="large"
              >
                <template #checked>开启</template>
                <template #unchecked>关闭</template>
              </n-switch>
            </n-form-item>
          </n-form>
        </n-card>

        <!-- About -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><InformationCircleOutline /></n-icon>
              <span>关于</span>
            </div>
          </template>

          <div class="about-content">
            <div class="about-item">
              <span class="about-label">版本</span>
              <n-tag type="success" size="small">v0.1.0</n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">许可证</span>
              <n-tag type="info" size="small">MPL-2.0</n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">GitHub</span>
              <n-text underline class="about-link">
                baiyuheniao/BaiyuAISpace2
              </n-text>
            </div>
          </div>
        </n-card>

        <div class="footer-text">
          <n-text depth="3" style="font-size: 12px;">
            Made with ❤️ by Baiyu
          </n-text>
        </div>
      </div>
    </n-layout-content>
  </n-layout>
</template>

<style scoped lang="scss">
.settings-view {
  height: 100%;
  background: var(--n-color);
}

.settings-content {
  height: 100%;
}

.settings-container {
  max-width: 700px;
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

.settings-card {
  margin-bottom: 20px;
  border-radius: 16px;
  background: var(--n-color-embed);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.04);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;
}

.settings-form {
  padding: 8px 0;
}

.form-select {
  max-width: 300px;
}

.form-input {
  max-width: 400px;
}

.about-content {
  padding: 8px 0;
}

.about-item {
  display: flex;
  align-items: center;
  padding: 12px 0;
  border-bottom: 1px solid var(--n-border-color);
}

.about-item:last-child {
  border-bottom: none;
}

.about-label {
  width: 100px;
  color: var(--n-text-color-3);
  font-size: 14px;
}

.about-link {
  color: #18a058;
  cursor: pointer;
  font-size: 14px;
}

.about-link:hover {
  color: #36ad6a;
}

.footer-text {
  text-align: center;
  margin-top: 40px;
  padding: 20px;
}
</style>
