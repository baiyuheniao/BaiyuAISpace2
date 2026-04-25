<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  MCPView.vue - MCP (Model Context Protocol) 服务管理视图组件
  
  功能说明:
  - MCP 服务器列表管理 (添加、删除、启用/禁用)
  - MCP 服务器配置 (stdio / SSE / HTTP 三种连接类型)
  - 服务器连接测试
  - 可用工具列表展示
  
  什么是 MCP:
  - MCP (Model Context Protocol) 是一个开放协议
  - 允许 AI 模型与外部工具和服务进行标准化交互
  - 通过 MCP，AI 可以调用各种外部工具扩展能力

  主要组成部分:
  - 服务器列表卡片
  - 可用工具卡片
  - 添加服务弹窗 (包含多步骤表单)
-->

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

// ============ 状态管理 ============

// MCP Store - 管理 MCP 服务器和工具
const mcp = useMCPStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

// ============ 弹窗状态 ============

/** 添加服务弹窗显示状态 */
const showCreateModal = ref(false);

/** 测试连接中状态 */
const testingConnection = ref(false);

/** 测试结果: true=成功, false=失败, null=未测试 */
const testResult = ref<boolean | null>(null);

// ============ 表单数据 ============

/**
 * 添加服务表单数据
 */
const formData = ref({
  name: "",                        // 服务器名称
  description: "",                 // 服务器描述
  server_type: "stdio" as "stdio" | "sse" | "http",  // 服务器类型
  command: "",                     // stdio 类型: 启动命令
  args: "",                        // stdio 类型: 命令参数 (每行一个)
  port: "",                        // HTTP/SSE 类型: 端口
  url: "",                         // HTTP/SSE 类型: 服务 URL
  api_key: "",                     // HTTP/SSE 类型: API Key
  enabled: true,                   // 是否启用
});

// ============ 辅助函数 ============

/**
 * 获取服务器类型对应的图标组件
 * 
 * @param type - 服务器类型 ("stdio" | "sse" | "http")
 * @returns 对应的图标组件
 */
const getServerIcon = (type: string) => {
  switch (type) {
    case "stdio":
      return Terminal;     // 终端图标
    case "sse":
      return Code;        // 代码图标
    case "http":
      return Globe;        // 网络图标
    default:
      return Cube;        // 默认立方体图标
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
 * 服务器类型选项列表
 * 用于下拉选择
 */
const serverTypeOptions = [
  { label: "标准输入输出 (stdio)", value: "stdio" },
  { label: "SSE 流", value: "sse" },
  { label: "HTTP API", value: "http" },
];

// ============ 表单操作方法 ============

/**
 * 重置表单数据
 * 清空所有输入并恢复默认值
 */
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

/**
 * 打开添加服务弹窗
 * 先重置表单再显示弹窗
 */
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

/**
 * 处理服务器类型变更
 * 切换类型时重置测试结果
 * 
 * @param type - 新的服务器类型
 */
const handleServerTypeChange = (type: string) => {
  formData.value.server_type = type as "stdio" | "sse" | "http";
  testResult.value = null;
};

// ============ 业务方法 ============

/**
 * 测试服务器连接
 * 验证配置能否成功连接到 MCP 服务器
 */
const handleTestConnection = async () => {
  // stdio 类型需要命令
  if (formData.value.server_type === "stdio") {
    if (!formData.value.command.trim()) {
      message.error("请输入命令");
      return;
    }
  } else {
    // HTTP/SSE 类型需要 URL
    if (!formData.value.url?.trim()) {
      message.error("请输入服务器 URL");
      return;
    }
  }

  try {
    testingConnection.value = true;
    
    // 调用 Store 方法测试连接
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

/**
 * 创建 MCP 服务器
 * 验证表单后添加到系统
 */
const handleCreate = async () => {
  // 名称验证
  if (!formData.value.name.trim()) {
    message.error("请输入服务器名称");
    return;
  }

  // 根据类型验证不同字段
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
    // 解析命令参数 (每行一个参数)
    const argsArray = formData.value.args
      .split("\n")
      .filter((arg) => arg.trim())
      .map((arg) => arg.trim());

    // 调用 Store 方法创建服务器
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
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px"
          >
            <Cube />
          </n-icon>
          MCP 服务管理
        </h1>

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
                @click="openCreateModal"
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
            clickable
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
                      命令: <n-text code>
                        {{ server.command }}
                      </n-text>
                    </n-text>
                    <!-- HTTP/SSE 类型显示 URL -->
                    <n-text
                      v-if="server.url"
                      depth="3"
                    >
                      地址: <n-text code>
                        {{ server.url }}
                      </n-text>
                    </n-text>
                  </n-space>
                </template>

                <!-- 操作按钮区域 -->
                <template #header-extra>
                  <n-space>
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
                      来自: <n-text strong>
                        {{ tool.server_name }}
                      </n-text>
                    </n-text>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>
        </n-card>
      </div>
    </n-layout-content>

    <!-- 添加 MCP 服务弹窗 -->
    <n-modal
      v-model:show="showCreateModal"
      title="添加 MCP 服务"
      preset="card"
      style="width: 600px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <!-- 基本信息 section -->
        <div class="form-section">
          <div class="section-title">
            基本信息
          </div>
          
          <!-- 服务名称 -->
          <n-form-item
            label="服务名称"
            required
          >
            <n-input
              v-model:value="formData.name"
              placeholder="例如：Wikipedia 搜索工具"
            />
          </n-form-item>

          <!-- 描述 -->
          <n-form-item label="描述">
            <n-input
              v-model:value="formData.description"
              placeholder="简要说明此 MCP 服务的功能"
              type="textarea"
              :rows="3"
            />
          </n-form-item>
        </div>

        <!-- 服务类型 section -->
        <div class="form-section">
          <div class="section-title">
            服务类型
          </div>
          
          <!-- 类型选择 -->
          <n-form-item
            label="类型"
            required
          >
            <n-select
              :value="formData.server_type"
              :options="serverTypeOptions"
              placeholder="选择服务类型"
              @update:value="handleServerTypeChange"
            />
          </n-form-item>

          <!-- stdio 类型配置 (条件渲染) -->
          <template v-if="formData.server_type === 'stdio'">
            <!-- 启动命令 -->
            <n-form-item
              label="启动命令"
              required
            >
              <n-input
                v-model:value="formData.command"
                placeholder="例如：/path/to/mcp-server 或 python mcp_server.py"
              />
              <template #feedback>
                <n-text
                  depth="3"
                  style="font-size: 12px"
                >
                  完整的启动命令路径或脚本
                </n-text>
              </template>
            </n-form-item>

            <!-- 命令参数 -->
            <n-form-item label="命令参数">
              <n-input
                v-model:value="formData.args"
                placeholder="每行一个参数&#10;例如&#10;--config config.json&#10;--port 8000"
                type="textarea"
                :rows="3"
              />
              <template #feedback>
                <n-text
                  depth="3"
                  style="font-size: 12px"
                >
                  可选，每行一个参数
                </n-text>
              </template>
            </n-form-item>
          </template>

          <!-- HTTP/SSE 类型配置 (条件渲染) -->
          <template v-else>
            <!-- 服务 URL -->
            <n-form-item
              label="服务 URL"
              required
            >
              <n-input
                v-model:value="formData.url"
                placeholder="例如：http://localhost:8000 或 https://api.example.com"
              />
            </n-form-item>

            <!-- 端口 (可选) -->
            <n-form-item label="端口 (可选)">
              <n-input
                v-model:value="formData.port"
                type="text"
                placeholder="默认端口如不提供将使用 URL 中的端口"
              />
            </n-form-item>

            <!-- API Key (可选) -->
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

        <!-- 设置 section -->
        <div class="form-section">
          <div class="section-title">
            设置
          </div>

          <!-- 启用开关 -->
          <n-form-item label="启用服务">
            <n-switch
              v-model:value="formData.enabled"
              size="large"
            >
              <template #checked>
                已启用
              </template>
              <template #unchecked>
                已禁用
              </template>
            </n-switch>
          </n-form-item>

          <!-- 测试连接按钮 -->
          <n-form-item label="测试连接">
            <n-button
              :loading="testingConnection"
              :type="testResult === true ? 'success' : testResult === false ? 'error' : 'default'"
              @click="handleTestConnection"
            >
              {{ testResult === true ? "✓ 连接成功" : testResult === false ? "✗ 连接失败" : "测试连接" }}
            </n-button>
            <n-text
              depth="3"
              style="font-size: 12px; margin-left: 12px"
            >
              建议先测试连接确保配置正确
            </n-text>
          </n-form-item>
        </div>
      </n-form>

      <!-- 弹窗底部按钮 -->
      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleCreate"
          >
            添加服务
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </n-layout>
</template>

<style scoped lang="scss">
/* 主容器 */
.mcp-view {
  height: 100%;
  background: var(--n-color);
}

/* 内容区域 */
.mcp-content {
  height: 100%;
}

/* 内容容器 */
.mcp-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 40px 32px;
}

/* 页面标题 */
.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

/* 卡片样式 */
.settings-card {
  margin-bottom: 20px;
  border-radius: 16px;
  background: var(--n-color-embed);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.04);
}

/* 卡片头部 */
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

/* 表单分区 */
.form-section {
  margin-bottom: 24px;
  padding-bottom: 24px;
  border-bottom: 1px solid var(--n-border-color);

  &:last-child {
    border-bottom: none;
    margin-bottom: 0;
  }

  /* 分区标题 */
  .section-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--n-text-color-2);
    margin-bottom: 12px;
  }
}
</style>
