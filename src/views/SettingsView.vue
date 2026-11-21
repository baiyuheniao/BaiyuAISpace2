<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SettingsView.vue - 设置视图

  页面外壳：标题 + 各设置卡片组件（见 ./settings/）。
  - LlmConfigCard / EmbeddingConfigCard / RerankerConfigCard：三类模型 API 配置，各自带新建/编辑弹窗
  - GeneralSettingsCard：外观、聊天显示、对话与工具、系统行为、安全与数据
  - AboutCard：版本信息、检测最新版本、导出日志
-->

<script setup lang="ts">
import { NLayout, NLayoutContent, NText } from "naive-ui";
import LlmConfigCard from "./settings/LlmConfigCard.vue";
import EmbeddingConfigCard from "./settings/EmbeddingConfigCard.vue";
import RerankerConfigCard from "./settings/RerankerConfigCard.vue";
import GeneralSettingsCard from "./settings/GeneralSettingsCard.vue";
import AboutCard from "./settings/AboutCard.vue";
</script>

<template>
  <!-- 设置主布局容器 -->
  <n-layout class="settings-view">
    <!-- 设置内容区域 -->
    <n-layout-content
      :native-scrollbar="false"
      class="settings-content"
    >
      <div class="settings-container">
        <!-- 页面标题 -->
        <header class="page-header enter-up">
          <span class="eyebrow">Settings</span>
          <h1 class="page-title">
            设置
          </h1>
        </header>

        <LlmConfigCard />
        <EmbeddingConfigCard />
        <RerankerConfigCard />
        <GeneralSettingsCard />
        <AboutCard />

        <!-- 页脚 -->
        <div class="footer-text">
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            Made with ❤️ by Baiyu
          </n-text>
        </div>
      </div>
    </n-layout-content>

  </n-layout>
</template>

<style scoped lang="scss">
/* 设置主容器 */
.settings-view {
  height: 100%;
  background: $bg;
}

/* 设置内容区域 */
.settings-content {
  height: 100%;
}

/* 内容容器 - 限制最大宽度并居中
   宽屏/超宽屏下分级放宽，避免两侧大片留白（但设置项是一行行的表单/列表，
   放太宽反而拉散标签和操作按钮的视觉关联，所以放宽幅度比列表页克制） */
.settings-container {
  max-width: 700px;
  margin: 0 auto;
  padding: 5rem 2rem 8rem;

  @media (min-width: $bp-wide) {
    max-width: 900px;
  }

  @media (min-width: $bp-ultrawide) {
    max-width: 1000px;
  }
}

/* 页面标题区域 */
.page-header {
  margin-bottom: 4rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.page-title {
  font-family: $font-serif;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

/* 设置卡片样式（卡片根元素在子组件里，用 :deep 透进去） */
.settings-container :deep(.settings-card) {
  margin-bottom: 20px;
  background: $bg;
  border: $border-soft;
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;

  &:hover {
    transform: translateY(-4px);
    box-shadow: $shadow-hover;
  }
}

/* 卡片标题样式（各卡片子组件共用，用 :deep 透进去） */
.settings-container :deep(.card-header) {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;

  .n-button {
    margin-left: auto;
  }
}
</style>
