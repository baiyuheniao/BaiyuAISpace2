<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  McpToolList.vue - 可用工具卡片

  展示所有已启用 MCP 服务器提供的工具；没有工具时整张卡片不渲染。
-->

<script setup lang="ts">
import { NCard, NList, NListItem, NThing, NSpace, NText, NIcon } from "naive-ui";
import { Play } from "@vicons/ionicons5";
import { useMCPStore } from "@/stores/mcp";

const mcp = useMCPStore();
</script>

<template>
  <!-- 可用工具卡片 -->
  <n-card
    v-if="mcp.availableTools.length > 0"
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon
          :size="20"
          depth="3"
        >
          <Play />
        </n-icon>
        <span>可用工具</span>
      </div>
    </template>

    <!-- 工具列表 -->
    <n-list hoverable>
      <n-list-item
        v-for="tool in mcp.availableTools"
        :key="`${tool.server_id}-${tool.name}`"
      >
        <n-thing>
          <!-- 工具名称 -->
          <template #header>
            {{ tool.name }}
          </template>
          <!-- 工具描述和来源 -->
          <template #description>
            <n-space
              vertical
              size="small"
            >
              <n-text depth="3">
                {{ tool.description }}
              </n-text>
              <n-text depth="3">
                来自:
                <n-text strong>
                  {{ tool.server_name }}
                </n-text>
              </n-text>
            </n-space>
          </template>
        </n-thing>
      </n-list-item>
    </n-list>
  </n-card>
</template>
