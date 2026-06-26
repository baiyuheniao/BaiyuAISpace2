<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SettingsView.vue - 设置视图组件
  
  功能说明:
  - 管理 LLM API 配置 (创建、编辑、删除、激活)
  - 管理 Embedding API 配置 (用于知识库向量化)
  - 外观设置 (深色/浅色主题切换)
  - 显示应用版本和关于信息

  主要组成部分:
  - LLM API 配置卡片
  - Embedding API 配置卡片
  - 外观设置卡片
  - 关于信息卡片
  - 新建/编辑弹窗表单
-->

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { 
  NLayout, 
  NLayoutContent, 
  NCard, 
  NForm, 
  NFormItem, 
  NSelect, 
  NInput, 
  NSwitch, 
  NButton,
  NSpace,
  NList,
  NListItem,
  NThing,
  NTag,
  NPopconfirm,
  NModal,
  NIcon,
  NText,
  NEmpty,
  NProgress,
  NAlert,
  NTooltip,
  useMessage
} from "naive-ui";
import { 
  useSettingsStore, 
  PRESET_PROVIDERS, 
  type ApiConfig,
  type EmbeddingApiConfig
} from "@/stores/settings";
import {
  useLocalModelStore,
  type ModelSource
} from "@/stores/localModel";
import { 
  ServerOutline, 
  KeyOutline, 
  ColorPaletteOutline, 
  InformationCircleOutline,
  DocumentTextOutline,
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline,
  CloudDownloadOutline,
  HardwareChipOutline,
  RefreshOutline,
  CheckmarkOutline,
  CloseOutline,
  PlayOutline,
  StopOutline,
  SearchOutline,
  RocketOutline,
  DownloadOutline,
} from "@vicons/ionicons5";

// ============ 状态管理 ============

// 设置 Store - 管理 API 配置和主题
const settings = useSettingsStore();

// 本地模型 Store
const localModel = useLocalModelStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

// ============ 日志导出 ============

const exportLogs = async () => {
  try {
    // 让用户选择保存位置
    const filePath = await save({
      defaultPath: `BaiyuAISpace2_logs_${new Date().toISOString().split('T')[0]}.log`,
      filters: [{ name: "Log Files", extensions: ["log"] }]
    });
    
    if (filePath) {
      // 调用后端复制日志文件
      const result = await invoke<string>("copy_log_file", { destPath: filePath });
      message.success("日志已导出到: " + result);
    }
  } catch (error) {
    message.error("导出日志失败: " + error);
  }
};

// ============ 弹窗状态 ============

/** LLM API 配置 - 新建弹窗显示状态 */
const showCreateModal = ref(false);

/** LLM API 配置 - 编辑弹窗显示状态 */
const showEditModal = ref(false);

/** LLM API 配置 - 当前编辑的配置对象 */
const editingConfig = ref<ApiConfig | null>(null);

/** Embedding API 配置 - 新建弹窗显示状态 */
const showEmbeddingCreateModal = ref(false);

/** Embedding API 配置 - 编辑弹窗显示状态 */
const showEmbeddingEditModal = ref(false);

/** Embedding API 配置 - 当前编辑的配置对象 */
const editingEmbeddingConfig = ref<EmbeddingApiConfig | null>(null);

// ============ 表单数据状态 ============

/**
 * LLM API 配置表单数据
 * 用于新建和编辑 LLM API 配置
 */
const formData = ref({
  name: "",                  // 配置名称
  provider: "openai",        // 默认使用 OpenAI
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,  // 默认 Base URL
  model: "",                 // 模型名称
  apiKey: "",                // API 密钥
});

/**
 * Embedding API 配置表单数据
 * 用于新建和编辑 Embedding API 配置
 */
const embeddingFormData = ref({
  name: "",                  // 配置名称
  provider: "openai",        // 默认使用 OpenAI
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,  // 默认 Base URL
  model: "text-embedding-3-small",  // 默认模型
  apiKey: "",                // API 密钥
});

// ============ 本地模型相关状态 ============

/** 下载模型表单数据 */
const pullFormData = ref({
  modelName: "",
  sourceId: "ollama",
});

/** Ollama 地址编辑状态 */
const ollamaUrlEditing = ref(false);

/** Ollama 地址编辑值 */
const ollamaUrlEditValue = ref(localModel.ollamaBaseUrl);

/** 下载源选项 */
const sourceOptions = computed(() =>
  localModel.modelSources.map((source: ModelSource) => ({
    label: source.name,
    value: source.id,
  }))
);

/** 镜像源选项 */
const mirrorOptions = computed(() =>
  localModel.downloadMirrors.map((m) => ({
    label: m.name,
    value: m.id,
  }))
);

/** 模型搜索关键词 */
const modelSearchQuery = ref("");

/** 模型搜索防抖定时器 */
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;

// ============ 表单方法 ============

/**
 * 重置 LLM API 表单数据
 * 恢复到初始状态
 */
const resetForm = () => {
  formData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
  };
};

/**
 * 重置 Embedding API 表单数据
 * 恢复到初始状态
 */
const resetEmbeddingForm = () => {
  embeddingFormData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
  };
};

// ============ 弹窗打开方法 ============

/**
 * 打开新建 LLM API 配置弹窗
 * 先重置表单，再显示弹窗
 */
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

/**
 * 打开编辑 LLM API 配置弹窗
 * 填充当前配置数据到表单
 * 
 * @param config - 要编辑的配置对象
 */
const openEditModal = (config: ApiConfig) => {
  editingConfig.value = config;
  formData.value = {
    name: config.name,
    provider: config.provider,
    baseUrl: config.baseUrl,
    model: config.model,
    apiKey: config.apiKey,
  };
  showEditModal.value = true;
};

/**
 * 打开新建 Embedding API 配置弹窗
 */
const openEmbeddingCreateModal = () => {
  resetEmbeddingForm();
  showEmbeddingCreateModal.value = true;
};

/**
 * 打开编辑 Embedding API 配置弹窗
 * 
 * @param config - 要编辑的配置对象
 */
const openEmbeddingEditModal = (config: EmbeddingApiConfig) => {
  editingEmbeddingConfig.value = config;
  embeddingFormData.value = {
    name: config.name,
    provider: config.provider,
    baseUrl: config.baseUrl,
    model: config.model,
    apiKey: config.apiKey,
  };
  showEmbeddingEditModal.value = true;
};

// ============ 提供商切换处理 ============

/**
 * 处理 LLM 提供商切换
 * 自动填入对应提供商的默认 Base URL
 * 
 * @param provider - 提供商标识符
 */
const handleProviderChange = (provider: string) => {
  formData.value.provider = provider;
  formData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

/**
 * 处理 Embedding 提供商切换
 * 自动填入对应提供商的默认 Base URL
 * 
 * @param provider - 提供商标识符
 */
const handleEmbeddingProviderChange = (provider: string) => {
  embeddingFormData.value.provider = provider;
  embeddingFormData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

// ============ CRUD 操作处理 ============

/**
 * 创建新的 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleCreate = async () => {
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }
  if (!formData.value.apiKey.trim()) {
    message.error("请输入 API Key");
    return;
  }

  // 调用 Store 方法创建配置
  settings.createApiConfig(
    formData.value.name,
    formData.value.provider,
    formData.value.model,
    formData.value.apiKey,
    formData.value.baseUrl
  );

  // 提示成功并关闭弹窗
  message.success("API 配置已创建");
  showCreateModal.value = false;
  resetForm();
};

/**
 * 更新 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleUpdate = async () => {
  if (!editingConfig.value) return;
  
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }

  // 调用 Store 方法更新配置
  settings.updateApiConfig(editingConfig.value.id, {
    name: formData.value.name,
    provider: formData.value.provider,
    baseUrl: formData.value.baseUrl,
    model: formData.value.model,
    apiKey: formData.value.apiKey,
  });

  // 提示成功并关闭弹窗
  message.success("API 配置已更新");
  showEditModal.value = false;
  editingConfig.value = null;
};

/**
 * 删除 LLM API 配置
 * 
 * @param configId - 要删除的配置 ID
 */
const handleDelete = (configId: string) => {
  settings.deleteApiConfig(configId);
  message.success("API 配置已删除");
};

/**
 * 设置当前使用的 LLM API 配置
 * 
 * @param configId - 要激活的配置 ID
 */
const handleSetActive = (configId: string) => {
  settings.setActiveConfig(configId);
  message.success("已设为当前使用配置");
};

/**
 * 创建新的 Embedding API 配置
 */
const handleEmbeddingCreate = async () => {
  // 表单验证
  if (!embeddingFormData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!embeddingFormData.value.model.trim()) {
    message.error("请输入 Embedding 模型名称");
    return;
  }
  if (!embeddingFormData.value.apiKey.trim()) {
    message.error("请输入 API Key");
    return;
  }

  // 调用 Store 方法创建配置
  settings.createEmbeddingApiConfig(
    embeddingFormData.value.name,
    embeddingFormData.value.provider,
    embeddingFormData.value.model,
    embeddingFormData.value.apiKey,
    embeddingFormData.value.baseUrl
  );

  // 提示成功并关闭弹窗
  message.success("Embedding API 配置已创建");
  showEmbeddingCreateModal.value = false;
  resetEmbeddingForm();
};

/**
 * 更新 Embedding API 配置
 */
const handleEmbeddingUpdate = async () => {
  if (!editingEmbeddingConfig.value) return;
  
  // 表单验证
  if (!embeddingFormData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!embeddingFormData.value.model.trim()) {
    message.error("请输入 Embedding 模型名称");
    return;
  }

  // 调用 Store 方法更新配置
  settings.updateEmbeddingApiConfig(editingEmbeddingConfig.value.id, {
    name: embeddingFormData.value.name,
    provider: embeddingFormData.value.provider,
    baseUrl: embeddingFormData.value.baseUrl,
    model: embeddingFormData.value.model,
    apiKey: embeddingFormData.value.apiKey,
  });

  // 提示成功并关闭弹窗
  message.success("Embedding API 配置已更新");
  showEmbeddingEditModal.value = false;
  editingEmbeddingConfig.value = null;
};

/**
 * 删除 Embedding API 配置
 * 
 * @param configId - 要删除的配置 ID
 */
const handleEmbeddingDelete = (configId: string) => {
  settings.deleteEmbeddingApiConfig(configId);
  message.success("Embedding API 配置已删除");
};

/**
 * 设置当前使用的 Embedding API 配置
 * 
 * @param configId - 要激活的配置 ID
 */
const handleSetEmbeddingActive = (configId: string) => {
  settings.setActiveEmbeddingApiConfig(configId);
  message.success("已设为当前 Embedding 配置");
};

// ============ 计算属性 ============

/**
 * 提供商下拉选项
 * 从 Store 获取预设的提供商列表
 */
const providerOptions = computed(() => settings.presetProviderOptions);

// ============ 本地模型方法 ============

/** 下载模型 */
const handlePullModel = async () => {
  if (!pullFormData.value.modelName.trim()) {
    message.warning("请输入模型名称");
    return;
  }
  try {
    await localModel.pullModel(pullFormData.value.modelName, pullFormData.value.sourceId);
    pullFormData.value.modelName = "";
    message.success("开始下载模型");
  } catch (error) {
    message.error(`下载失败: ${error}`);
  }
};

/** 删除本地模型 */
const handleDeleteModel = async (name: string) => {
  try {
    await localModel.deleteModel(name);
    message.success(`已删除模型: ${name}`);
  } catch (error) {
    message.error(`删除失败: ${error}`);
  }
};

/** 刷新本地模型列表 */
const handleRefreshModels = async () => {
  try {
    await localModel.loadModels();
    message.success("已刷新模型列表");
  } catch (error) {
    message.error(`刷新失败: ${error}`);
  }
};

/** 保存 Ollama 地址 */
const handleSaveOllamaUrl = () => {
  localModel.setOllamaBaseUrl(ollamaUrlEditValue.value);
  ollamaUrlEditing.value = false;
  message.success("Ollama 地址已更新");
};

/** 格式化文件大小 */
const formatSize = (bytes: number): string => {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
};

// ============ Ollama 安装 & 服务管理方法 ============

/** 检测 Ollama 安装 */
const handleDetectInstallation = async () => {
  try {
    await localModel.detectInstallation();
    if (localModel.ollamaInstalled) {
      message.success(`检测到 Ollama${localModel.ollamaInstalledVersion ? ' v' + localModel.ollamaInstalledVersion : ''}`);
    }
  } catch (error) {
    message.error(`检测失败: ${error}`);
  }
};

/** 启动 Ollama 服务 */
const handleStartService = async () => {
  try {
    const status = await localModel.startService();
    if (status?.running) {
      message.success("Ollama 服务已启动");
    } else {
      message.error("Ollama 服务启动失败");
    }
  } catch (error) {
    message.error(`启动失败: ${error}`);
  }
};

/** 停止 Ollama 服务 */
const handleStopService = async () => {
  try {
    await localModel.stopService();
    message.success("Ollama 服务已停止");
  } catch (error) {
    message.error(`停止失败: ${error}`);
  }
};

/** 下载安装 Ollama */
const handleInstallOllama = async () => {
  try {
    await localModel.downloadAndInstallOllama();
    message.success("Ollama 安装完成并已启动服务");
  } catch (error) {
    message.error(`安装失败: ${error}`);
  }
};

/** 模型搜索（防抖） */
const handleModelSearch = (query: string) => {
  modelSearchQuery.value = query;
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer);
  }
  searchDebounceTimer = setTimeout(() => {
    localModel.searchModels(query);
  }, 500);
};

/** 选择搜索结果中的模型并下载 */
const handleSelectSearchResult = async (modelName: string) => {
  try {
    // Search results are from Ollama library, force use "ollama" source
    await localModel.pullModel(modelName, "ollama");
    message.success(`模型 ${modelName} 下载完成`);
  } catch (error) {
    message.error(`下载失败: ${error}`);
  }
  modelSearchQuery.value = "";
  localModel.modelSearchResults = [];
};

/** 一键部署 */
const handleOneClickDeploy = async (modelName: string) => {
  try {
    await localModel.oneClickDeploy(modelName);
    message.success(`模型 ${modelName} 部署完成`);
  } catch (error) {
    message.error(`部署失败: ${error}`);
  }
};

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
});
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
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px;"
          >
            <ServerOutline />
          </n-icon>
          设置
        </h1>

        <!-- LLM API 配置卡片 -->
        <n-card
          class="settings-card"
          :bordered="false"
        >
          <!-- 卡片标题 -->
          <template #header>
            <div class="card-header">
              <n-icon
                :size="20"
                depth="3"
              >
                <KeyOutline />
              </n-icon>
              <span>对话模型 API 配置</span>
              <!-- 新建配置按钮 -->
              <n-button
                type="primary"
                size="small"
                @click="openCreateModal"
              >
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- 配置列表 -->
          <n-list
            v-if="settings.apiConfigs.length > 0"
            hoverable
            clickable
          >
            <!-- 遍历显示每个配置 -->
            <n-list-item 
              v-for="config in settings.apiConfigs" 
              :key="config.id"
              @click="handleSetActive(config.id)"
            >
              <n-thing>
                <!-- 配置名称 -->
                <template #header>
                  <n-space align="center">
                    <span>{{ config.name }}</span>
                    <!-- 当前使用标签 -->
                    <n-tag 
                      v-if="config.id === settings.activeConfigId" 
                      type="success" 
                      size="small"
                    >
                      当前使用
                    </n-tag>
                  </n-space>
                </template>
                
                <!-- 配置描述 -->
                <template #description>
                  <n-space
                    vertical
                    size="small"
                  >
                    <n-text depth="3">
                      <n-icon
                        :size="14"
                        style="margin-right: 4px;"
                      >
                        <CubeOutline />
                      </n-icon>
                      模型: {{ config.model }}
                    </n-text>
                    <n-text depth="3">
                      <n-icon
                        :size="14"
                        style="margin-right: 4px;"
                      >
                        <LinkOutline />
                      </n-icon>
                      {{ PRESET_PROVIDERS[config.provider]?.name || config.provider }}
                    </n-text>
                  </n-space>
                </template>
                
                <!-- 操作按钮 -->
                <template #header-extra>
                  <n-space>
                    <!-- 编辑按钮 -->
                    <n-button
                      quaternary
                      circle
                      size="small"
                      @click.stop="openEditModal(config)"
                    >
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <!-- 删除按钮 -->
                    <n-popconfirm 
                      positive-text="删除"
                      negative-text="取消"
                      @positive-click="handleDelete(config.id)"
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
                      确定删除配置 "{{ config.name }}"？
                    </n-popconfirm>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>

          <!-- 空状态 -->
          <n-empty
            v-else
            description="暂无 API 配置"
          />

          <!-- 卡片底部提示 -->
          <template
            v-if="settings.apiConfigs.length > 0"
            #footer
          >
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              <n-icon
                :size="12"
                style="margin-right: 4px;"
              >
                <CheckmarkCircle />
              </n-icon>
              API Key 使用系统密钥链加密存储（Windows Credential / macOS Keychain / Linux Secret Service）
            </n-text>
          </template>
        </n-card>

        <!-- Embedding API 配置卡片 -->
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
                <DocumentTextOutline />
              </n-icon>
              <span>Embedding 向量模型 API 配置</span>
              <n-button
                type="primary"
                size="small"
                @click="openEmbeddingCreateModal"
              >
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- Embedding 配置列表 -->
          <n-list
            v-if="settings.embeddingApiConfigs.length > 0"
            hoverable
            clickable
          >
            <n-list-item 
              v-for="config in settings.embeddingApiConfigs" 
              :key="config.id"
              @click="handleSetEmbeddingActive(config.id)"
            >
              <n-thing>
                <template #header>
                  <n-space align="center">
                    <span>{{ config.name }}</span>
                    <n-tag 
                      v-if="config.id === settings.activeEmbeddingApiConfigId" 
                      type="success" 
                      size="small"
                    >
                      当前使用
                    </n-tag>
                  </n-space>
                </template>
                <template #description>
                  <n-space
                    vertical
                    size="small"
                  >
                    <n-text depth="3">
                      <n-icon
                        :size="14"
                        style="margin-right: 4px;"
                      >
                        <CubeOutline />
                      </n-icon>
                      模型: {{ config.model }}
                    </n-text>
                    <n-text depth="3">
                      <n-icon
                        :size="14"
                        style="margin-right: 4px;"
                      >
                        <LinkOutline />
                      </n-icon>
                      {{ PRESET_PROVIDERS[config.provider]?.name || config.provider }}
                    </n-text>
                  </n-space>
                </template>
                <template #header-extra>
                  <n-space>
                    <n-button
                      quaternary
                      circle
                      size="small"
                      @click.stop="openEmbeddingEditModal(config)"
                    >
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-popconfirm 
                      positive-text="删除"
                      negative-text="取消"
                      @positive-click="handleEmbeddingDelete(config.id)"
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
                      确定删除 Embedding API 配置 "{{ config.name }}"？
                    </n-popconfirm>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>

          <n-empty
            v-else
            description="暂无 Embedding API 配置"
          />

          <template
            v-if="settings.embeddingApiConfigs.length > 0"
            #footer
          >
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              <n-icon
                :size="12"
                style="margin-right: 4px;"
              >
                <CheckmarkCircle />
              </n-icon>
              Embedding API 用于知识库的文档向量化和检索查询
            </n-text>
          </template>
        </n-card>

        <!-- 本地部署卡片 -->
        <n-card
          class="settings-card"
          title="本地部署"
          :bordered="false"
          :segmented="{ content: true, footer: true }"
        >
          <template #header-extra>
            <n-space align="center">
              <n-button
                size="small"
                @click="handleRefreshModels"
                :loading="localModel.isLoadingModels"
                :disabled="!localModel.isOnline"
              >
                <template #icon>
                  <n-icon><RefreshOutline /></n-icon>
                </template>
                刷新
              </n-button>
            </n-space>
          </template>

          <!-- Step 1: 安装检测 -->
          <div class="deploy-section">
            <div class="section-title">
              <n-text strong>1. Ollama 环境</n-text>
            </div>

            <!-- 未安装状态 -->
            <template v-if="!localModel.ollamaInstalled">
              <n-space
                vertical
                align="center"
                style="padding: 16px 0;"
              >
                <n-text depth="3">未检测到 Ollama，需要先安装才能使用本地部署</n-text>
                <n-space align="center">
                  <n-select
                    v-model:value="localModel.selectedMirrorId"
                    :options="mirrorOptions"
                    size="small"
                    style="width: 200px;"
                    placeholder="选择下载源"
                  />
                  <n-button
                    type="primary"
                    :loading="localModel.isInstallingOllama"
                    @click="handleInstallOllama"
                  >
                    <template #icon>
                      <n-icon><DownloadOutline /></n-icon>
                    </template>
                    一键安装 Ollama
                  </n-button>
                </n-space>

                <!-- 安装进度 -->
                <template v-if="localModel.isInstallingOllama && localModel.ollamaInstallProgress">
                  <div style="width: 100%; max-width: 400px;">
                    <n-text depth="3" style="font-size: 12px;">
                      {{ localModel.ollamaInstallProgress.message }}
                    </n-text>
                    <n-progress
                      v-if="localModel.ollamaInstallProgress.progressPercent > 0"
                      type="line"
                      :percentage="localModel.ollamaInstallProgress.progressPercent"
                      :indicator-placement="'inside'"
                      processing
                      style="margin-top: 4px;"
                    />
                  </div>
                </template>
              </n-space>
            </template>

            <!-- 已安装状态 -->
            <template v-else>
              <n-space
                align="center"
                justify="space-between"
                style="padding: 8px 0;"
              >
                <n-space align="center">
                  <n-icon
                    size="20"
                    :color="localModel.isOnline ? '#18a058' : '#d03050'"
                  >
                    <HardwareChipOutline />
                  </n-icon>
                  <n-text v-if="localModel.isOnline">
                    Ollama 在线
                    <n-tag
                      v-if="localModel.ollamaVersion"
                      size="small"
                      type="success"
                      style="margin-left: 8px;"
                    >
                      v{{ localModel.ollamaVersion }}
                    </n-tag>
                    <n-tag
                      v-if="localModel.serviceManagedByApp"
                      size="small"
                      type="info"
                      style="margin-left: 4px;"
                    >
                      应用管理
                    </n-tag>
                  </n-text>
                  <n-text
                    v-else
                    type="warning"
                  >
                    Ollama 已安装但未运行
                  </n-text>
                </n-space>
                <n-space align="center">
                  <!-- 启动/停止按钮 -->
                  <n-button
                    v-if="!localModel.isOnline"
                    type="primary"
                    size="small"
                    @click="handleStartService"
                  >
                    <template #icon>
                      <n-icon><PlayOutline /></n-icon>
                    </template>
                    启动服务
                  </n-button>
                  <n-button
                    v-else-if="localModel.serviceManagedByApp"
                    size="small"
                    @click="handleStopService"
                  >
                    <template #icon>
                      <n-icon><StopOutline /></n-icon>
                    </template>
                    停止服务
                  </n-button>

                  <!-- Ollama 地址编辑 -->
                  <template v-if="ollamaUrlEditing">
                    <n-input
                      v-model:value="ollamaUrlEditValue"
                      size="small"
                      style="width: 250px;"
                      placeholder="http://localhost:11434"
                    />
                    <n-button
                      size="small"
                      type="primary"
                      @click="handleSaveOllamaUrl"
                    >
                      <template #icon>
                        <n-icon><CheckmarkOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-button
                      size="small"
                      @click="ollamaUrlEditing = false"
                    >
                      <template #icon>
                        <n-icon><CloseOutline /></n-icon>
                      </template>
                    </n-button>
                  </template>
                  <template v-else>
                    <n-text
                      depth="3"
                      style="font-size: 12px;"
                    >
                      {{ localModel.ollamaBaseUrl }}
                    </n-text>
                    <n-button
                      size="tiny"
                      @click="ollamaUrlEditing = true; ollamaUrlEditValue = localModel.ollamaBaseUrl"
                    >
                      编辑
                    </n-button>
                  </template>
                </n-space>
              </n-space>
            </template>
          </div>

          <!-- Step 2: 模型下载（仅在 Ollama 在线时显示） -->
          <div
            v-if="localModel.isOnline"
            class="deploy-section"
            style="margin-top: 16px;"
          >
            <div class="section-title">
              <n-text strong>2. 下载模型</n-text>
            </div>

            <!-- 搜索输入 -->
            <n-space
              vertical
              style="margin-top: 8px;"
            >
              <n-input
                :value="modelSearchQuery"
                placeholder="输入模型名称搜索，如 llama3, qwen2, mistral..."
                @update:value="handleModelSearch"
              >
                <template #prefix>
                  <n-icon><SearchOutline /></n-icon>
                </template>
              </n-input>

              <!-- 搜索结果 -->
              <div
                v-if="localModel.isSearchingModels"
                style="text-align: center; padding: 12px;"
              >
                <n-text depth="3">搜索中...</n-text>
              </div>
              <n-list
                v-else-if="localModel.modelSearchResults.length > 0"
                bordered
                size="small"
              >
                <n-list-item
                  v-for="result in localModel.modelSearchResults"
                  :key="result.name"
                >
                  <n-thing>
                    <template #header>{{ result.name }}</template>
                    <template #description>
                      <n-text depth="3" style="font-size: 12px;">
                        {{ result.description }}
                      </n-text>
                    </template>
                  </n-thing>
                  <template #suffix>
                    <n-button
                      size="small"
                      type="primary"
                      :loading="localModel.isPulling"
                      @click="handleSelectSearchResult(result.name)"
                    >
                      <template #icon>
                        <n-icon><CloudDownloadOutline /></n-icon>
                      </template>
                      下载
                    </n-button>
                  </template>
                </n-list-item>
              </n-list>

              <!-- 手动输入模型名下载 -->
              <n-space align="center" style="margin-top: 4px;">
                <n-input
                  v-model:value="pullFormData.modelName"
                  size="small"
                  style="width: 250px;"
                  placeholder="或直接输入模型名，如 llama3:8b"
                />
                <n-select
                  v-model:value="pullFormData.sourceId"
                  :options="sourceOptions"
                  size="small"
                  style="width: 140px;"
                />
                <n-button
                  size="small"
                  type="primary"
                  :loading="localModel.isPulling"
                  :disabled="!pullFormData.modelName.trim()"
                  @click="handlePullModel"
                >
                  <template #icon>
                    <n-icon><CloudDownloadOutline /></n-icon>
                  </template>
                  下载
                </n-button>
              </n-space>
            </n-space>

            <!-- 下载进度 -->
            <n-alert
              v-if="localModel.isPulling"
              type="info"
              style="margin-top: 12px;"
            >
              <template #icon>
                <n-icon><CloudDownloadOutline /></n-icon>
              </template>
              正在下载: {{ localModel.pullingModelName }}
              <n-progress
                v-if="localModel.downloadPercent !== null"
                type="line"
                :percentage="localModel.downloadPercent"
                :indicator-placement="'inside'"
                processing
                style="margin-top: 8px;"
              />
            </n-alert>
          </div>

          <!-- Step 3: 已有模型 & 一键部署 -->
          <div
            v-if="localModel.isOnline"
            class="deploy-section"
            style="margin-top: 16px;"
          >
            <div class="section-title">
              <n-text strong>3. 本地模型</n-text>
              <n-text
                depth="3"
                style="font-size: 12px; margin-left: 8px;"
              >
                {{ localModel.localModels.length }} 个
              </n-text>
            </div>

            <n-empty
              v-if="!localModel.isLoadingModels && localModel.localModels.length === 0"
              description="暂无本地模型，请先下载"
              style="margin-top: 12px;"
            />

            <div
              v-if="localModel.isLoadingModels"
              style="text-align: center; padding: 20px;"
            >
              <n-text depth="3">加载模型列表中...</n-text>
            </div>

            <n-list
              v-if="localModel.localModels.length > 0"
              bordered
              size="small"
              style="margin-top: 8px;"
            >
              <n-list-item
                v-for="model in localModel.localModels"
                :key="model.name"
              >
                <n-thing>
                  <template #header>
                    <n-space align="center" :size="6">
                      <n-icon><HardwareChipOutline /></n-icon>
                      {{ model.name }}
                    </n-space>
                  </template>
                  <template #description>
                    <n-space
                      size="small"
                      style="margin-top: 4px;"
                    >
                      <n-tag
                        v-if="model.details?.parameterSize"
                        size="small"
                        type="info"
                      >
                        {{ model.details.parameterSize }}
                      </n-tag>
                      <n-tag
                        v-if="model.details?.quantizationLevel"
                        size="small"
                      >
                        {{ model.details.quantizationLevel }}
                      </n-tag>
                      <n-tag
                        v-if="model.size"
                        size="small"
                      >
                        {{ formatSize(model.size) }}
                      </n-tag>
                      <n-tag
                        v-if="model.details?.family"
                        size="small"
                        type="success"
                      >
                        {{ model.details.family }}
                      </n-tag>
                    </n-space>
                  </template>
                </n-thing>
                <template #suffix>
                  <n-space :size="4">
                    <n-tooltip trigger="hover">
                      <template #trigger>
                        <n-button
                          size="small"
                          type="primary"
                          @click="handleOneClickDeploy(model.name)"
                        >
                          <template #icon>
                            <n-icon><RocketOutline /></n-icon>
                          </template>
                          部署
                        </n-button>
                      </template>
                      一键部署并创建聊天配置
                    </n-tooltip>
                    <n-popconfirm @positive-click="handleDeleteModel(model.name)">
                      <template #trigger>
                        <n-button
                          size="small"
                          type="error"
                          quaternary
                        >
                          <template #icon>
                            <n-icon><TrashOutline /></n-icon>
                          </template>
                        </n-button>
                      </template>
                      确定删除模型 {{ model.name }}？
                    </n-popconfirm>
                  </n-space>
                </template>
              </n-list-item>
            </n-list>
          </div>

          <!-- 底部：下载源设置 -->
          <template #footer>
            <n-space
              align="center"
              justify="space-between"
            >
              <n-space align="center">
                <n-text
                  depth="3"
                  style="font-size: 12px;"
                >
                  模型下载源:
                </n-text>
                <n-select
                  :value="localModel.selectedSourceId"
                  :options="sourceOptions"
                  size="small"
                  style="width: 180px;"
                  @update:value="localModel.setSelectedSourceId"
                />
              </n-space>
              <n-button
                size="tiny"
                @click="handleDetectInstallation"
              >
                重新检测 Ollama
              </n-button>
            </n-space>
          </template>
        </n-card>

        <!-- 外观设置卡片 -->
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
                <ColorPaletteOutline />
              </n-icon>
              <span>外观</span>
            </div>
          </template>

          <!-- 表单设置 -->
          <n-form
            label-placement="left"
            label-width="100px"
            class="settings-form"
          >
            <n-form-item label="深色模式">
              <!-- 主题切换开关 -->
              <n-switch
                :value="settings.darkMode"
                size="large"
                @update:value="settings.toggleTheme"
              >
                <template #checked>
                  开启
                </template>
                <template #unchecked>
                  关闭
                </template>
              </n-switch>
            </n-form-item>
          </n-form>
        </n-card>

        <!-- 关于卡片 -->
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
                <InformationCircleOutline />
              </n-icon>
              <span>关于</span>
            </div>
          </template>

          <!-- 关于内容 -->
          <div class="about-content">
            <div class="about-item">
              <span class="about-label">版本</span>
              <n-tag
                type="success"
                size="small"
              >
                v0.1.0
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">许可证</span>
              <n-tag
                type="info"
                size="small"
              >
                MPL-2.0
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">GitHub</span>
              <n-text
                underline
                class="about-link"
              >
                baiyuheniao/BaiyuAISpace2
              </n-text>
            </div>
            
            <div
              class="about-item"
              style="margin-top: 16px;"
            >
              <n-button 
                type="primary" 
                size="small"
                @click="exportLogs"
              >
                导出日志
              </n-button>
            </div>
          </div>
        </n-card>

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

    <!-- 新建 LLM API 配置弹窗 -->
    <n-modal
      v-model:show="showCreateModal"
      title="新建 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              已自动填入 {{ PRESET_PROVIDERS[formData.provider]?.name }} 默认地址，可手动修改
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="模型"
          required
        >
          <n-input 
            v-model:value="formData.model" 
            placeholder="例如：gpt-4o, claude-3-5-sonnet, qwen-max..."
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              输入模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="API Key"
          required
        >
          <n-input 
            v-model:value="formData.apiKey" 
            type="password"
            show-password-on="click"
            placeholder="输入 API Key"
          />
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleCreate"
          >
            创建
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 编辑 LLM API 配置弹窗 -->
    <n-modal
      v-model:show="showEditModal"
      title="编辑 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
        </n-form-item>

        <n-form-item
          label="模型"
          required
        >
          <n-input 
            v-model:value="formData.model" 
            placeholder="例如：gpt-4o, claude-3-5-sonnet..."
          />
        </n-form-item>

        <n-form-item label="API Key">
          <n-input 
            v-model:value="formData.apiKey" 
            type="password"
            show-password-on="click"
            :placeholder="formData.provider === 'baidu' ? '请输入 access_token' : '留空表示不修改'"
          />
          <template #feedback>
            <n-text
              v-if="formData.provider === 'baidu'"
              depth="2"
              style="font-size: 12px; color: #f0a020;"
            >
              百度千帆需要 access_token，而非 API Key。请在
              <n-a
                href="https://console.bce.baidu.com/qianfan/"
                target="_blank"
              >
                百度千帆控制台
              </n-a>
              获取 API Key 和 Secret Key，然后
              <n-a
                href="https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Ck3edn42t"
                target="_blank"
              >
                换取 access_token
              </n-a>
            </n-text>
            <n-text
              v-else
              depth="3"
              style="font-size: 12px;"
            >
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEditModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleUpdate"
          >
            保存
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 新建 Embedding API 配置弹窗 -->
    <n-modal
      v-model:show="showEmbeddingCreateModal"
      title="新建 Embedding API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="140px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleEmbeddingProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              已自动填入 {{ PRESET_PROVIDERS[embeddingFormData.provider]?.name }} 默认地址
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="Embedding 模型"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.model" 
            placeholder="例如：text-embedding-3-small, embedding-2, bge-large-zh..."
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              输入 Embedding 模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="API Key"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.apiKey" 
            type="password"
            show-password-on="click"
            placeholder="输入 API Key"
          />
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEmbeddingCreateModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleEmbeddingCreate"
          >
            创建
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 编辑 Embedding API 配置弹窗 -->
    <n-modal
      v-model:show="showEmbeddingEditModal"
      title="编辑 Embedding API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="140px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleEmbeddingProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
        </n-form-item>

        <n-form-item
          label="Embedding 模型"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.model" 
            placeholder="例如：text-embedding-3-small, embedding-2..."
          />
        </n-form-item>

        <n-form-item label="API Key">
          <n-input 
            v-model:value="embeddingFormData.apiKey" 
            type="password"
            show-password-on="click"
            placeholder="留空表示不修改"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEmbeddingEditModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleEmbeddingUpdate"
          >
            保存
          </n-button>
        </n-space>
      </template>
    </n-modal>

  </n-layout>
</template>

<style scoped lang="scss">
/* 设置主容器 */
.settings-view {
  height: 100%;
  background: var(--n-color);
}

/* 设置内容区域 */
.settings-content {
  height: 100%;
}

/* 内容容器 - 限制最大宽度并居中 */
.settings-container {
  max-width: 700px;
  margin: 0 auto;
  padding: 40px 32px;
}

/* 页面标题样式 */
.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

/* 设置卡片样式 */
.settings-card {
  margin-bottom: 20px;
  border-radius: $radius-xl;
  background: var(--n-color-embed);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.04);
}

/* 卡片标题样式 */
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

/* Ollama 状态区域 */
.ollama-status-section {
  padding: 8px 0;
  border-bottom: 1px solid var(--n-border-color);
  margin-bottom: 4px;
}

/* 本地部署分区 */
.deploy-section {
  .section-title {
    margin-bottom: 8px;
    padding-bottom: 4px;
    border-bottom: 1px dashed var(--n-border-color);
  }
}
</style>
