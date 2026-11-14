<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  McpServerModal.vue - 添加 MCP 服务弹窗

  父组件通过 ref 调用 open()（空白表单）或 openWithPreset(preset)（用预设填充）。
  表单状态、连接测试、保存都在这里。
-->

<script setup lang="ts">
import { ref } from "vue";
import { NModal, NForm, NFormItem, NInput, NSelect, NSwitch, NSpace, NButton, NText } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import { useMCPStore } from "@/stores/mcp";
import type { MCPPreset } from "./presets";

const mcp = useMCPStore();
const message = useMessage();

/** 添加服务弹窗显示状态 */
const showCreateModal = ref(false);

/** 测试连接中状态 */
const testingConnection = ref(false);

/** 测试结果: true=成功, false=失败, null=未测试 */
const testResult = ref<boolean | null>(null);

/** 测试失败时的具体原因（比如缺少 uv/Node.js），用于替代笼统的"连接失败" */
const testErrorMessage = ref<string | null>(null);

/**
 * 添加服务表单数据
 */
const formData = ref({
  name: "", // 服务器名称
  description: "", // 服务器描述
  server_type: "stdio" as "stdio" | "sse" | "http", // 服务器类型
  command: "", // stdio 类型: 启动命令
  args: "", // stdio 类型: 命令参数 (每行一个)
  port: "", // HTTP/SSE 类型: 端口
  url: "", // HTTP/SSE 类型: 服务 URL
  api_key: "", // HTTP/SSE 类型: API Key
  enabled: true, // 是否启用
});

/**
 * 服务器类型选项列表
 * 用于下拉选择
 */
const serverTypeOptions = [
  { label: "标准输入输出 (stdio)", value: "stdio" },
  { label: "SSE 流", value: "sse" },
  { label: "HTTP API", value: "http" },
];

/**
 * 重置表单数据
 * 清空所有输入并恢复默认值，同时清掉上一次的测试结果——
 * 否则重新打开弹窗时会显示一个对应着空表单的"连接成功/失败"徽章，
 * 让人误以为当前（空）配置已经测试过
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
  testResult.value = null;
  testErrorMessage.value = null;
};

/**
 * 打开添加服务弹窗
 * 先重置表单再显示弹窗
 */
const open = () => {
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
  testErrorMessage.value = null;
};

/**
 * 应用推荐服务预设
 * 用预设数据填充表单并打开添加服务弹窗，用户仍可在弹窗中检查/修改后再保存
 *
 * @param preset - 选中的预设
 */
const openWithPreset = (preset: MCPPreset) => {
  formData.value = {
    name: preset.name,
    description: preset.description,
    server_type: preset.serverType,
    command: preset.command,
    args: preset.args.join("\n"),
    port: "",
    url: "",
    api_key: "",
    enabled: true,
  };
  testResult.value = null;
  testErrorMessage.value = null;
  showCreateModal.value = true;
  if (preset.needsConfig) {
    message.info(preset.needsConfig);
  }
};

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

    // 命令参数是单独一个文本框（每行一个），必须和 command 一起传给后端，
    // 否则测试探测的是一个不带参数的裸命令（比如单独的 "npx"），和真正保存
    // 后启动的命令完全不是一回事，会产生误报
    const argsArray = formData.value.args
      .split("\n")
      .filter((arg) => arg.trim())
      .map((arg) => arg.trim());

    // 调用 Store 方法测试连接
    const result = await mcp.testConnection(
      formData.value.server_type,
      formData.value.command || undefined,
      argsArray,
      formData.value.url || undefined
    );

    testResult.value = result.success;
    testErrorMessage.value = result.error ?? null;
    console.debug("[MCP] test connection", {
      command: formData.value.command,
      args: argsArray,
      url: formData.value.url,
      result,
    });
    if (result.success) {
      message.success("连接成功");
    } else {
      message.error(result.error || "连接失败");
    }
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
      testErrorMessage.value = null;
    }
  } catch (error) {
    message.error("添加失败：" + String(error));
  }
};

defineExpose({ open, openWithPreset });
</script>

<template>
  <!-- 添加 MCP 服务弹窗 -->
  <n-modal
    v-model:show="showCreateModal"
    title="添加 MCP 服务"
    preset="card"
    style="width: 600px; max-height: 85vh"
    :content-style="{ overflowY: 'auto' }"
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
              placeholder="例如：npx 或 /path/to/mcp-server"
            />
            <template #feedback>
              <n-text
                depth="3"
                style="font-size: 12px"
              >
                只填可执行文件本身（如 npx、python、/path/to/binary），不要包含参数或脚本名 --
                脚本名和其余参数请填到下面的"命令参数"里
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
            {{
              testResult === true
                ? "✓ 连接成功"
                : testResult === false
                  ? "✗ 连接失败"
                  : "测试连接"
            }}
          </n-button>
          <n-text
            v-if="testResult === false && testErrorMessage"
            depth="1"
            style="font-size: 12px; margin-left: 12px"
          >
            {{ testErrorMessage }}
          </n-text>
          <n-text
            v-else
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
</template>

<style scoped lang="scss">
/* 表单分区 */
.form-section {
  margin-bottom: 24px;
  padding-bottom: 24px;
  border-bottom: $border-faint;

  &:last-child {
    border-bottom: none;
    margin-bottom: 0;
  }

  /* 分区标题: 小字号大写 label */
  .section-title {
    font-size: $label-size;
    font-weight: 500;
    letter-spacing: $label-tracking;
    text-transform: uppercase;
    color: $ink-faint;
    margin-bottom: 12px;
  }
}
</style>
