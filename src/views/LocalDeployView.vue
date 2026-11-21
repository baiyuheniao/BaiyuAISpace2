<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  LocalDeployView.vue - 本地部署视图

  页面外壳：标题 + Ollama / LM Studio 两个标签页，各自一个面板组件（见 ./local-deploy/）。
  进入页面时对两个服务的探测集中在这里，面板只读 store。

  标签页用 display-directive="show:lazy"：首次打开后保持挂载，这样切换标签页时输入框里
  没提交的文字、地址编辑状态不会丢（拆分前这些状态放在本文件里，天然不丢）。
-->

<script setup lang="ts">
import { onMounted } from "vue";
import { NLayout, NLayoutContent, NTabs, NTabPane } from "naive-ui";
import { useLocalModelStore } from "@/stores/localModel";
import { useLMStudioStore } from "@/stores/lmStudio";
import OllamaPanel from "./local-deploy/OllamaPanel.vue";
import LMStudioPanel from "./local-deploy/LMStudioPanel.vue";

// 本地模型 Store (Ollama)
const localModel = useLocalModelStore();

// LM Studio Store
const lmStudio = useLMStudioStore();

// ============ 初始化 ============

onMounted(async () => {
  // Initialize local model store data
  await Promise.all([
    localModel.loadModelSources(),
    localModel.loadDownloadMirrors(),
    localModel.detectInstallation(),
  ]);

  // If Ollama is installed, check service status
  if (localModel.ollamaInstalled) {
    await localModel.checkStatus();
    if (localModel.isOnline) {
      await localModel.loadModels();
    }
  }

  // LM Studio has no install detection -- just probe whether its server
  // (started by the user via its own GUI) happens to be reachable
  await lmStudio.checkStatus();
  if (lmStudio.isOnline) {
    await lmStudio.loadModels();
  }
});
</script>

<template>
  <!-- 本地部署主布局 -->
  <n-layout class="local-deploy-view">
    <n-layout-content
      :native-scrollbar="false"
      class="local-deploy-content"
    >
      <div class="local-deploy-container">
        <!-- 页面标题 -->
        <header class="page-header enter-up">
          <span class="eyebrow">Local</span>
          <h1 class="page-title">
            本地部署
          </h1>
        </header>

        <!-- Ollama / LM Studio 切换标签 -->
        <n-tabs
          type="line"
          animated
        >
          <n-tab-pane
            name="ollama"
            tab="Ollama"
            display-directive="show:lazy"
          >
            <OllamaPanel />
          </n-tab-pane>

          <n-tab-pane
            name="lmstudio"
            tab="LM Studio"
            display-directive="show:lazy"
          >
            <LMStudioPanel />
          </n-tab-pane>
        </n-tabs>
      </div>
    </n-layout-content>
  </n-layout>
</template>

<style scoped lang="scss">
/* 主容器 */
.local-deploy-view {
  height: 100%;
  background: $bg;
}

/* 内容区域 */
.local-deploy-content {
  height: 100%;
}

/* 内容容器
   宽屏/超宽屏下分级放宽，列表行能容纳更多信息，两侧也不至于大片空白 */
.local-deploy-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 5rem 2rem 8rem;

  @media (min-width: $bp-wide) {
    max-width: 1150px;
  }

  @media (min-width: $bp-ultrawide) {
    max-width: 1350px;
  }
}

/* 页面标题 */
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

/* 卡片 / 分区样式由两个面板共用，用 :deep 透进去 */
.local-deploy-container {
  :deep(.settings-card) {
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

  :deep(.card-header) {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 16px;
    font-weight: 600;
  }

  /* 本地部署分区 */
  :deep(.deploy-section) {
    .section-title {
      margin-bottom: 8px;
      padding-bottom: 4px;
      border-bottom: 1px dashed var(--color-line-faint);
    }
  }
}
</style>
