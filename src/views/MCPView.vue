<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  MCPView.vue - MCP (Model Context Protocol) 服务管理视图

  页面外壳：标题 + 各功能卡片组件。
  - MemoryPanel：分层记忆管理（暂留在此页，位置待 #137 一并调整）
  - RecommendedCapabilities / TerminalSettingsCard / McpMarket：推荐能力、内置终端设置、市场
  - McpServerList / McpToolList：已连接的服务、可用工具
  - McpServerModal：添加服务弹窗（含多步骤表单与连接测试）

  什么是 MCP:
  - MCP (Model Context Protocol) 是一个开放协议，允许 AI 模型与外部工具和服务进行标准化交互
-->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NLayout, NLayoutContent } from "naive-ui";
import { useMCPStore } from "@/stores/mcp";
import MemoryPanel from "@/components/MemoryPanel.vue";
import RecommendedCapabilities from "./mcp/RecommendedCapabilities.vue";
import TerminalSettingsCard from "./mcp/TerminalSettingsCard.vue";
import McpMarket from "./mcp/McpMarket.vue";
import McpServerList from "./mcp/McpServerList.vue";
import McpToolList from "./mcp/McpToolList.vue";
import McpServerModal from "./mcp/McpServerModal.vue";
import type { MCPPreset } from "./mcp/presets";

const mcp = useMCPStore();

const serverModal = ref<InstanceType<typeof McpServerModal> | null>(null);

const openCreate = () => serverModal.value?.open();
const usePreset = (preset: MCPPreset) => serverModal.value?.openWithPreset(preset);

onMounted(() => {
  mcp.loadServers();
});
</script>

<template>
  <!-- MCP 主布局 -->
  <n-layout class="mcp-view">
    <n-layout-content
      :native-scrollbar="false"
      class="mcp-content"
    >
      <div class="mcp-container">
        <!-- 页面标题 -->
        <header class="page-header enter-up">
          <span class="eyebrow">MCP</span>
          <h1 class="page-title">
            MCP 服务管理
          </h1>
        </header>
        <MemoryPanel />

        <RecommendedCapabilities @use-preset="usePreset" />
        <TerminalSettingsCard />
        <McpMarket @use-preset="usePreset" />
        <McpServerList @create="openCreate" />
        <McpToolList />
      </div>
    </n-layout-content>

    <McpServerModal ref="serverModal" />
  </n-layout>
</template>

<style scoped lang="scss">
/* 主容器 */
.mcp-view {
  height: 100%;
  background: $bg;
}

/* 内容区域 */
.mcp-content {
  height: 100%;
}

/* 内容容器
   宽屏/超宽屏下分级放宽，列表行能容纳更多信息，两侧也不至于大片空白 */
.mcp-container {
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

/* 卡片样式由各卡片子组件共用，用 :deep 透进去 */
.mcp-container {
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

    .n-button {
      margin-left: auto;
    }
  }
}
</style>
