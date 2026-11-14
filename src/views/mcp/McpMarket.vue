<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  McpMarket.vue - MCP 市场卡片

  比"推荐能力"范围更大的可搜索目录，选中后走同一套"填表单 / 查状态"逻辑（由父组件打开弹窗）。
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NInput, NSpace, NTag, NList, NListItem, NThing, NText, NButton, NEmpty, NIcon } from "naive-ui";
import { Add, Search, Storefront } from "@vicons/ionicons5";
import { useMCPStore } from "@/stores/mcp";
import {
  MCP_MARKET,
  MCP_MARKET_CATEGORIES,
  isPresetConfigured as isConfiguredIn,
  type MCPPreset,
} from "./presets";

const emit = defineEmits<{
  (e: "use-preset", preset: MCPPreset): void;
}>();

const mcp = useMCPStore();

const isPresetConfigured = (preset: MCPPreset) => isConfiguredIn(preset, mcp.servers);

/** 市场搜索关键词，匹配名称或描述 */
const marketSearchQuery = ref("");

/** 市场当前选中的分类；null 表示"全部" */
const marketSelectedCategory = ref<string | null>(null);

/**
 * 切换市场分类筛选
 * 再次点击已选中的分类会取消筛选，回到"全部"
 *
 * @param category - 点击的分类
 */
const toggleMarketCategory = (category: string) => {
  marketSelectedCategory.value = marketSelectedCategory.value === category ? null : category;
};

/** 按关键词 + 分类筛选后的市场条目 */
const filteredMarketEntries = computed(() => {
  const query = marketSearchQuery.value.trim().toLowerCase();
  return MCP_MARKET.filter((entry) => {
    const matchesCategory =
      !marketSelectedCategory.value || entry.category === marketSelectedCategory.value;
    const matchesQuery =
      !query ||
      entry.name.toLowerCase().includes(query) ||
      entry.description.toLowerCase().includes(query);
    return matchesCategory && matchesQuery;
  });
});
</script>

<template>
  <!-- MCP 市场卡片：比"推荐能力"范围更大的可搜索目录，选中后走同一套
       填充表单/查状态逻辑，"推荐能力"里已经列出的条目在这里也能搜到 -->
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
          <Storefront />
        </n-icon>
        <span>MCP 市场</span>
      </div>
    </template>

    <div class="market-filters">
      <n-input
        v-model:value="marketSearchQuery"
        placeholder="搜索服务名称或功能描述"
        clearable
      >
        <template #prefix>
          <n-icon><Search /></n-icon>
        </template>
      </n-input>

      <n-space class="market-categories">
        <n-tag
          :checked="marketSelectedCategory === null"
          checkable
          @click="marketSelectedCategory = null"
        >
          全部
        </n-tag>
        <n-tag
          v-for="category in MCP_MARKET_CATEGORIES"
          :key="category"
          :checked="marketSelectedCategory === category"
          checkable
          @click="toggleMarketCategory(category)"
        >
          {{ category }}
        </n-tag>
      </n-space>
    </div>

    <n-list
      v-if="filteredMarketEntries.length > 0"
      hoverable
    >
      <n-list-item
        v-for="entry in filteredMarketEntries"
        :key="entry.id"
      >
        <n-thing>
          <template #header>
            <n-space align="center">
              <span>{{ entry.name }}</span>
              <n-tag
                size="tiny"
                :bordered="false"
              >
                {{ entry.category }}
              </n-tag>
            </n-space>
          </template>
          <template #description>
            <n-text depth="3">
              {{ entry.description }}
            </n-text>
          </template>
        </n-thing>
        <template #suffix>
          <!-- 已经配置过：操作入口挪到下方"已连接的服务" -->
          <n-tag
            v-if="isPresetConfigured(entry)"
            size="small"
          >
            已配置
          </n-tag>
          <n-button
            v-else
            size="small"
            @click="emit('use-preset', entry)"
          >
            <template #icon>
              <n-icon><Add /></n-icon>
            </template>
            使用此配置
          </n-button>
        </template>
      </n-list-item>
    </n-list>

    <!-- 搜索/筛选无结果 -->
    <n-empty
      v-else
      description="没有找到匹配的服务，换个关键词或分类试试"
    />
  </n-card>
</template>

<style scoped lang="scss">
/* 市场筛选区：搜索框 + 分类标签，与列表之间留出间距 */
.market-filters {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 16px;
}

.market-categories {
  flex-wrap: wrap;
}
</style>
