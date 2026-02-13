<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref } from "vue";
import {
  NLayout,
  NLayoutContent,
  NCard,
  NButton,
  NList,
  NListItem,
  NThing,
  NTag,
  NText,
  NEmpty,
  NModal,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NSwitch,
  NSpace,
  NPopconfirm,
  NIcon,
  useMessage,
} from "naive-ui";
import {
  Add,
  TrashOutline,
  CheckmarkCircle,
  Play,
  Cube,
  Code,
  Globe,
  Terminal,
} from "@vicons/ionicons5";
import { useMCPStore } from "@/stores/mcp";

const mcp = useMCPStore();
const message = useMessage();

// Modal state
const showCreateModal = ref(false);
const testingConnection = ref(false);
const testResult = ref<boolean | null>(null);

// Form state
const formData = ref({
  name: "",
  description: "",
  server_type: "stdio" as "stdio" | "sse" | "http",
  command: "",
  args: "",
  port: "",
  url: "",
  api_key: "",
  enabled: true,
});

// Get server type icon
const getServerIcon = (type: string) => {
  switch (type) {
    case "stdio":
      return Terminal;
    case "sse":
      return Code;
    case "http":
      return Globe;
    default:
      return Cube;
  }
};

// Get server type label
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

// Reset form
const resetForm = () => {
  formData.value = {
    name: "",
    description: "",
    server_type: "stdio",
    command: "",
    args: "",
    port: "",
    url: "",
    api_key: "",
    enabled: true,
  };
};

// Open create modal
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

// Test connection
const handleTestConnection = async () => {
  if (formData.value.server_type === "stdio") {
    if (!formData.value.command.trim()) {
      message.error("请输入命令");
      return;
    }
  } else {
    if (!formData.value.url?.trim()) {
      message.error("请输入服务器 URL");
      return;
    }
  }

  try {
    testingConnection.value = true;
    const result = await mcp.testConnection(
      formData.value.server_type,
      formData.value.command || undefined,
      formData.value.url || undefined
    );
    testResult.value = result;
    message.success(result ? "连接成功" : "连接失败");
  } finally {
    testingConnection.value = false;
  }
};

// Create server
const handleCreate = async () => {
  if (!formData.value.name.trim()) {
    message.error("请输入服务器名称");
    return;
  }

  if (formData.value.server_type === "stdio") {
    if (!formData.value.command.trim()) {
      message.error("请输入启动命令");
      return;
    }
  } else {
    if (!formData.value.url?.trim()) {
      message.error("请输入服务器 URL");
      return;
    }
  }

  try {
    const argsArray = formData.value.args
      .split("\n")
      .filter((arg) => arg.trim())
      .map((arg) => arg.trim());

    const server = await mcp.createServer({
      name: formData.value.name,
      description: formData.value.description,
      server_type: formData.value.server_type,
      command: formData.value.command,
      args: argsArray,
      env: {},
      port: formData.value.port ? parseInt(formData.value.port) : undefined,
      url: formData.value.url,
      api_key: formData.value.api_key || undefined,
      enabled: formData.value.enabled,
    });

    if (server) {
      message.success("MCP 服务器已添加");
      showCreateModal.value = false;
      resetForm();
      testResult.value = null;
    }
  } catch (error) {
    message.error("添加失败：" + String(error));
  }
};

// Delete server
const handleDelete = async (serverId: string) => {
  try {
    await mcp.deleteServer(serverId);
    message.success("MCP 服务器已删除");
  } catch (error) {
    message.error("删除失败：" + String(error));
  }
};

// Toggle server
const handleToggle = async (serverId: string) => {
  try {
    await mcp.toggleServerEnabled(serverId);
  } catch (error) {
    message.error("操作失败：" + String(error));
  }
};

// Server type options
const serverTypeOptions = [
  { label: "标准输入输出 (stdio)", value: "stdio" },
  { label: "SSE 流", value: "sse" },
  { label: "HTTP API", value: "http" },
];

// Update server type label when changed
const handleServerTypeChange = (type: string) => {
  formData.value.server_type = type as "stdio" | "sse" | "http";
  testResult.value = null;
};
</script>

<template>
  <n-layout class="mcp-view">
    <n-layout-content :native-scrollbar="false" class="mcp-content">
      <div class="mcp-container">
        <h1 class="page-title">
          <n-icon :size="28" style="margin-right: 12px"><Cube /></n-icon>
          MCP 服务管理
        </h1>

        <!-- MCP Servers List -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><Cube /></n-icon>
              <span>已连接的服务</span>
              <n-button type="primary" size="small" @click="openCreateModal">
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                添加服务
              </n-button>
            </div>
          </template>

          <!-- Servers List -->
          <n-list v-if="mcp.servers.length > 0" hoverable clickable>
            <n-list-item
              v-for="server in mcp.servers"
              :key="server.id"
            >
              <n-thing>
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

                <template #description>
                  <n-space vertical size="small">
                    <n-text depth="3">
                      <n-icon :size="14" style="margin-right: 4px">
                        <component :is="getServerIcon(server.server_type)" />
                      </n-icon>
                      {{ getServerTypeLabel(server.server_type) }}
                    </n-text>
                    <n-text depth="3" v-if="server.description">
                      {{ server.description }}
                    </n-text>
                    <n-text depth="3" v-if="server.server_type === 'stdio'">
                      命令: <n-text code>{{ server.command }}</n-text>
                    </n-text>
                    <n-text depth="3" v-if="server.url">
                      地址: <n-text code>{{ server.url }}</n-text>
                    </n-text>
                  </n-space>
                </template>

                <template #header-extra>
                  <n-space>
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
                    <n-popconfirm
                      @positive-click="handleDelete(server.id)"
                      positive-text="删除"
                      negative-text="取消"
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

          <n-empty v-else description="暂无 MCP 服务" />

          <template #footer v-if="mcp.servers.length > 0">
            <n-text depth="3" style="font-size: 12px">
              <n-icon :size="12" style="margin-right: 4px"><CheckmarkCircle /></n-icon>
              已启用的服务允许在对话中调用其提供的工具
            </n-text>
          </template>
        </n-card>

        <!-- Available Tools -->
        <n-card v-if="mcp.availableTools.length > 0" class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><Play /></n-icon>
              <span>可用工具</span>
            </div>
          </template>

          <n-list hoverable>
            <n-list-item
              v-for="tool in mcp.availableTools"
              :key="`${tool.server_id}-${tool.name}`"
            >
              <n-thing>
                <template #header>
                  {{ tool.name }}
                </template>
                <template #description>
                  <n-space vertical size="small">
                    <n-text depth="3">{{ tool.description }}</n-text>
                    <n-text depth="3">
                      来自: <n-text strong>{{ tool.server_name }}</n-text>
                    </n-text>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>
        </n-card>
      </div>
    </n-layout-content>

    <!-- Create Modal -->
    <n-modal
      v-model:show="showCreateModal"
      title="添加 MCP 服务"
      preset="card"
      style="width: 600px"
      :mask-closable="false"
    >
      <n-form label-placement="left" label-width="100px">
        <!-- Basic Info Section -->
        <div class="form-section">
          <div class="section-title">基本信息</div>
          
          <n-form-item label="服务名称" required>
            <n-input
              v-model:value="formData.name"
              placeholder="例如：Wikipedia 搜索工具"
            />
          </n-form-item>

          <n-form-item label="描述">
            <n-input
              v-model:value="formData.description"
              placeholder="简要说明此 MCP 服务的功能"
              type="textarea"
              :rows="3"
            />
          </n-form-item>
        </div>

        <!-- Server Type Section -->
        <div class="form-section">
          <div class="section-title">服务类型</div>
          
          <n-form-item label="类型" required>
            <n-select
              :value="formData.server_type"
              :options="serverTypeOptions"
              @update:value="handleServerTypeChange"
              placeholder="选择服务类型"
            />
          </n-form-item>

          <!-- Stdio Configuration -->
          <template v-if="formData.server_type === 'stdio'">
            <n-form-item label="启动命令" required>
              <n-input
                v-model:value="formData.command"
                placeholder="例如：/path/to/mcp-server 或 python mcp_server.py"
              />
              <template #feedback>
                <n-text depth="3" style="font-size: 12px">
                  完整的启动命令路径或脚本
                </n-text>
              </template>
            </n-form-item>

            <n-form-item label="命令参数">
              <n-input
                v-model:value="formData.args"
                placeholder="每行一个参数&#10;例如&#10;--config config.json&#10;--port 8000"
                type="textarea"
                :rows="3"
              />
              <template #feedback>
                <n-text depth="3" style="font-size: 12px">
                  可选，每行一个参数
                </n-text>
              </template>
            </n-form-item>
          </template>

          <!-- HTTP/SSE Configuration -->
          <template v-else>
            <n-form-item label="服务 URL" required>
              <n-input
                v-model:value="formData.url"
                placeholder="例如：http://localhost:8000 或 https://api.example.com"
              />
            </n-form-item>

            <n-form-item label="端口 (可选)">
              <n-input
                v-model:value="formData.port"
                type="text"
                placeholder="默认端口如不提供将使用 URL 中的端口"
              />
            </n-form-item>

            <n-form-item label="API Key (可选)">
              <n-input
                v-model:value="formData.api_key"
                type="password"
                show-password-on="click"
                placeholder="如服务需要认证"
              />
            </n-form-item>
          </template>
        </div>

        <!-- Settings Section -->
        <div class="form-section">
          <div class="section-title">设置</div>

          <n-form-item label="启用服务">
            <n-switch
              v-model:value="formData.enabled"
              size="large"
            >
              <template #checked>已启用</template>
              <template #unchecked>已禁用</template>
            </n-switch>
          </n-form-item>

          <n-form-item label="测试连接">
            <n-button
              :loading="testingConnection"
              :type="testResult === true ? 'success' : testResult === false ? 'error' : 'default'"
              @click="handleTestConnection"
            >
              {{ testResult === true ? "✓ 连接成功" : testResult === false ? "✗ 连接失败" : "测试连接" }}
            </n-button>
            <n-text depth="3" style="font-size: 12px; margin-left: 12px">
              建议先测试连接确保配置正确
            </n-text>
          </n-form-item>
        </div>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateModal = false">取消</n-button>
          <n-button type="primary" @click="handleCreate">添加服务</n-button>
        </n-space>
      </template>
    </n-modal>
  </n-layout>
</template>

<style scoped lang="scss">
.mcp-view {
  height: 100%;
  background: var(--n-color);
}

.mcp-content {
  height: 100%;
}

.mcp-container {
  max-width: 900px;
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

  .n-button {
    margin-left: auto;
  }
}

.form-section {
  margin-bottom: 24px;
  padding-bottom: 24px;
  border-bottom: 1px solid var(--n-border-color);

  &:last-child {
    border-bottom: none;
    margin-bottom: 0;
  }

  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--n-text-color-2);
    margin-bottom: 12px;
  }
}
</style>
