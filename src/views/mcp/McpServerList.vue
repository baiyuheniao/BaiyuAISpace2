<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  McpServerList.vue - 已连接的服务卡片

  已保存的 MCP 服务器：测试连通性 / 启用禁用 / 删除；"添加服务"交给父组件打开弹窗。
-->

<script setup lang="ts">
import { ref } from "vue";
import { NCard, NList, NListItem, NThing, NSpace, NTag, NText, NButton, NPopconfirm, NEmpty, NIcon } from "naive-ui";
import { Add, CheckmarkCircle, Code, Cube, Globe, Play, Terminal, TrashOutline } from "@vicons/ionicons5";
import { useMessage } from "@/composables/useNotify";
import { useMCPStore, type MCPServer } from "@/stores/mcp";

const emit = defineEmits<{
  (e: "create"): void;
}>();

const mcp = useMCPStore();
const message = useMessage();

/**
 * 获取服务器类型对应的图标组件
 *
 * @param type - 服务器类型 ("stdio" | "sse" | "http")
 * @returns 对应的图标组件
 */
const getServerIcon = (type: string) => {
  switch (type) {
    case "stdio":
      return Terminal; // 终端图标
    case "sse":
      return Code; // 代码图标
    case "http":
      return Globe; // 网络图标
    default:
      return Cube; // 默认立方体图标
  }
};

/**
 * 获取服务器类型的中文标签
 *
 * @param type - 服务器类型
 * @returns 中文标签
 */
const getServerTypeLabel = (type: string) => {
  switch (type) {
    case "stdio":
      return "标准输入输出";
    case "sse":
      return "SSE 流";
    case "http":
      return "HTTP API";
    default:
      return "未知";
  }
};

/**
 * 删除 MCP 服务器
 *
 * @param serverId - 要删除的服务器 ID
 */
const handleDelete = async (serverId: string) => {
  try {
    await mcp.deleteServer(serverId);
    message.success("MCP 服务器已删除");
  } catch (error) {
    message.error("删除失败：" + String(error));
  }
};

/**
 * 切换服务器启用/禁用状态
 *
 * @param serverId - 服务器 ID
 */
const handleToggle = async (serverId: string) => {
  try {
    await mcp.toggleServerEnabled(serverId);
  } catch (error) {
    message.error("操作失败：" + String(error));
  }
};

/** 正在测试连接中的服务器 ID（用于按钮 loading 态） */
const testingServerId = ref<string | null>(null);

/**
 * 测试已保存服务器的连接
 * 之前只能在"添加服务"弹窗里测试，保存后的服务器没有任何重新验证连通性的入口
 *
 * @param server - 要测试的服务器
 */
const handleTestSavedServer = async (server: MCPServer) => {
  testingServerId.value = server.id;
  try {
    const result = await mcp.testConnection(
      server.server_type,
      server.command || undefined,
      server.args,
      server.url || undefined
    );
    console.debug("[MCP] test saved server", { id: server.id, name: server.name, result });
    if (result.success) {
      message.success(`「${server.name}」连接成功`);
    } else {
      message.error(`「${server.name}」${result.error || "连接失败"}`);
    }
  } finally {
    testingServerId.value = null;
  }
};
</script>

<template>
  <!-- MCP 服务器列表卡片 -->
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
          <Cube />
        </n-icon>
        <span>已连接的服务</span>
        <!-- 添加服务按钮 -->
        <n-button
          type="primary"
          size="small"
          @click="emit('create')"
        >
          <template #icon>
            <n-icon><Add /></n-icon>
          </template>
          添加服务
        </n-button>
      </div>
    </template>

    <!-- 服务器列表 -->
    <n-list
      v-if="mcp.servers.length > 0"
      hoverable
    >
      <n-list-item
        v-for="server in mcp.servers"
        :key="server.id"
      >
        <n-thing>
          <!-- 服务器名称和状态标签 -->
          <template #header>
            <n-space align="center">
              <span>{{ server.name }}</span>
              <n-tag
                :type="server.enabled ? 'success' : 'default'"
                size="small"
              >
                {{ server.enabled ? "已启用" : "已禁用" }}
              </n-tag>
            </n-space>
          </template>

          <!-- 服务器描述和配置信息 -->
          <template #description>
            <n-space
              vertical
              size="small"
            >
              <!-- 服务器类型 -->
              <n-text depth="3">
                <n-icon
                  :size="14"
                  style="margin-right: 4px"
                >
                  <component :is="getServerIcon(server.server_type)" />
                </n-icon>
                {{ getServerTypeLabel(server.server_type) }}
              </n-text>
              <!-- 描述 (如果有) -->
              <n-text
                v-if="server.description"
                depth="3"
              >
                {{ server.description }}
              </n-text>
              <!-- stdio 类型显示命令 -->
              <n-text
                v-if="server.server_type === 'stdio'"
                depth="3"
              >
                命令:
                <n-text code>
                  {{ server.command }}
                </n-text>
              </n-text>
              <!-- HTTP/SSE 类型显示 URL -->
              <n-text
                v-if="server.url"
                depth="3"
              >
                地址:
                <n-text code>
                  {{ server.url }}
                </n-text>
              </n-text>
            </n-space>
          </template>

          <!-- 操作按钮区域 -->
          <template #header-extra>
            <n-space>
              <!-- 测试连接按钮 -->
              <n-button
                quaternary
                circle
                size="small"
                :loading="testingServerId === server.id"
                @click.stop="handleTestSavedServer(server)"
              >
                <template #icon>
                  <n-icon><Play /></n-icon>
                </template>
              </n-button>
              <!-- 启用/禁用切换按钮 -->
              <n-button
                quaternary
                circle
                size="small"
                :type="server.enabled ? 'success' : 'default'"
                @click.stop="handleToggle(server.id)"
              >
                <template #icon>
                  <n-icon><CheckmarkCircle /></n-icon>
                </template>
              </n-button>
              <!-- 删除确认弹窗 -->
              <n-popconfirm
                positive-text="删除"
                negative-text="取消"
                @positive-click="handleDelete(server.id)"
              >
                <template #trigger>
                  <n-button
                    quaternary
                    circle
                    size="small"
                    type="error"
                    @click.stop
                  >
                    <template #icon>
                      <n-icon><TrashOutline /></n-icon>
                    </template>
                  </n-button>
                </template>
                确定删除服务 "{{ server.name }}"？
              </n-popconfirm>
            </n-space>
          </template>
        </n-thing>
      </n-list-item>
    </n-list>

    <!-- 空状态 -->
    <n-empty
      v-else
      description="暂无 MCP 服务"
    />

    <!-- 底部提示 -->
    <template
      v-if="mcp.servers.length > 0"
      #footer
    >
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
        已启用的服务允许在对话中调用其提供的工具
      </n-text>
    </template>
  </n-card>
</template>
