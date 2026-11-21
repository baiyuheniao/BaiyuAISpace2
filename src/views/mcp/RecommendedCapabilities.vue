<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  RecommendedCapabilities.vue - 推荐能力卡片

  内置能力（零配置）与社区预设（需配置）合并展示；点"使用此配置"由父组件打开添加服务弹窗。
-->

<script setup lang="ts">
import { computed } from "vue";
import { NCard, NList, NListItem, NThing, NText, NTag, NButton, NIcon } from "naive-ui";
import { Add, CheckmarkCircle, Sparkles } from "@vicons/ionicons5";
import { useMCPStore } from "@/stores/mcp";
import {
  BUILTIN_CAPABILITIES,
  MCP_PRESETS,
  isPresetConfigured as isConfiguredIn,
  type MCPPreset,
  type RecommendedItem,
} from "./presets";

const emit = defineEmits<{
  (e: "use-preset", preset: MCPPreset): void;
}>();

const mcp = useMCPStore();

/**
 * 合并内置能力与社区预设，供"推荐能力"卡片统一渲染
 * 保持在同一模块里是因为二者都是"软件默认推荐"的东西，用户不需要关心
 * 背后是原生实现还是社区包，只需要通过状态标签区分"能不能直接用"
 */
const recommendedItems = computed<RecommendedItem[]>(() => [
  ...BUILTIN_CAPABILITIES.map((capability) => ({ kind: "builtin" as const, capability })),
  ...MCP_PRESETS.map((preset) => ({ kind: "preset" as const, preset })),
]);

const isPresetConfigured = (preset: MCPPreset) => isConfiguredIn(preset, mcp.servers);
</script>

<template>
  <!-- 推荐能力卡片：内置能力（零配置）与社区预设（需配置）合并展示，
       用状态标签区分"能直接用"和"点了才能用"，而不是拆成两张视觉权重相同的卡片 -->
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon
          :size="20"
          depth="3"
        >
          <Sparkles />
        </n-icon>
        <span>推荐能力</span>
      </div>
    </template>

    <n-list hoverable>
      <n-list-item
        v-for="item in recommendedItems"
        :key="item.kind === 'builtin' ? item.capability.id : item.preset.id"
      >
        <n-thing>
          <template #header>
            {{ item.kind === "builtin" ? item.capability.name : item.preset.name }}
          </template>
          <template #description>
            <n-text depth="3">
              {{ item.kind === "builtin" ? item.capability.description : item.preset.description }}
            </n-text>
          </template>
        </n-thing>
        <template #suffix>
          <!-- 内置能力：常驻生效，不需要也不能操作 -->
          <n-tag
            v-if="item.kind === 'builtin'"
            type="success"
            size="small"
          >
            已启用
          </n-tag>
          <!-- 已经用过该预设保存出对应服务器：操作入口挪到下方"已连接的服务" -->
          <n-tag
            v-else-if="isPresetConfigured(item.preset)"
            size="small"
          >
            已配置
          </n-tag>
          <!-- 尚未配置的预设：一键填充表单 -->
          <n-button
            v-else
            size="small"
            @click="emit('use-preset', item.preset)"
          >
            <template #icon>
              <n-icon><Add /></n-icon>
            </template>
            使用此配置
          </n-button>
        </template>
      </n-list-item>
    </n-list>

    <template #footer>
      <n-text
        depth="3"
        style="font-size: 12px"
      >
        <n-icon
          :size="12"
          style="margin-right: 4px"
        >
          <CheckmarkCircle />
        </n-icon>
        内置能力随应用开箱即用；其余预设均为社区官方参考实现，首次运行时通过 npx/uvx
        自动下载，无需手动准备脚本文件
      </n-text>
    </template>
  </n-card>
</template>
