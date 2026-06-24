<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  LocalDeployView.vue - 本地部署视图组件

  功能说明:
  - 检测/安装 Ollama
  - 启动/停止 Ollama 服务
  - 搜索、下载、删除本地模型
  - 一键部署本地模型并创建聊天配置
-->

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
  NLayout,
  NLayoutContent,
  NCard,
  NSelect,
  NInput,
  NButton,
  NSpace,
  NList,
  NListItem,
  NThing,
  NTag,
  NPopconfirm,
  NIcon,
  NText,
  NEmpty,
  NProgress,
  NAlert,
  NTooltip,
  NTabs,
  NTabPane,
  useMessage,
} from "naive-ui";
import { open as openExternalUrl } from "@tauri-apps/plugin-shell";
import {
  useLocalModelStore,
  type ModelSource,
} from "@/stores/localModel";
import { useLMStudioStore } from "@/stores/lmStudio";
import {
  HardwareChipOutline,
  RefreshOutline,
  CheckmarkOutline,
  CloseOutline,
  PlayOutline,
  StopOutline,
  SearchOutline,
  RocketOutline,
  DownloadOutline,
  CloudDownloadOutline,
  TrashOutline,
  OpenOutline,
} from "@vicons/ionicons5";

// ============ 状态管理 ============

// 本地模型 Store (Ollama)
const localModel = useLocalModelStore();

// LM Studio Store
const lmStudio = useLMStudioStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

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

// ============ LM Studio 相关状态 ============

/** LM Studio 下载模型表单数据 (模型 ID，如 "qwen2.5-7b-instruct") */
const lmStudioPullModelId = ref("");

/** LM Studio 地址编辑状态 */
const lmStudioUrlEditing = ref(false);

/** LM Studio 地址编辑值 */
const lmStudioUrlEditValue = ref(lmStudio.baseUrl);

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
    if (status.running) {
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

// ============ LM Studio 方法 ============

/** 打开 LM Studio 官网下载页 */
const handleOpenLMStudioWebsite = () => {
  openExternalUrl("https://lmstudio.ai/download");
};

/** 检测 / 刷新 LM Studio 连接状态 */
const handleCheckLMStudio = async () => {
  await lmStudio.checkStatus();
  if (lmStudio.isOnline) {
    await lmStudio.loadModels();
    message.success("已连接到 LM Studio");
  } else {
    message.warning("未检测到正在运行的 LM Studio 服务");
  }
};

/** 刷新 LM Studio 模型列表 */
const handleRefreshLMStudioModels = async () => {
  try {
    await lmStudio.loadModels();
    message.success("已刷新模型列表");
  } catch (error) {
    message.error(`刷新失败: ${error}`);
  }
};

/** 保存 LM Studio 地址 */
const handleSaveLMStudioUrl = () => {
  lmStudio.setBaseUrl(lmStudioUrlEditValue.value);
  lmStudioUrlEditing.value = false;
  message.success("LM Studio 地址已更新");
};

/** 下载 LM Studio 模型 */
const handlePullLMStudioModel = async () => {
  if (!lmStudioPullModelId.value.trim()) {
    message.warning("请输入模型 ID");
    return;
  }
  try {
    await lmStudio.pullModel(lmStudioPullModelId.value.trim());
    lmStudioPullModelId.value = "";
    message.success("开始下载模型");
  } catch (error) {
    message.error(`下载失败: ${error}`);
  }
};

/** 加载 / 卸载 LM Studio 模型 */
const handleToggleLMStudioModel = async (model: { id: string; state: string }) => {
  try {
    if (model.state === "loaded") {
      await lmStudio.unloadModel(model.id);
      message.success(`已卸载模型: ${model.id}`);
    } else {
      await lmStudio.loadModel(model.id);
      message.success(`已加载模型: ${model.id}`);
    }
  } catch (error) {
    message.error(`操作失败: ${error}`);
  }
};

/** 一键部署 LM Studio 模型 */
const handleLMStudioOneClickDeploy = async (modelId: string) => {
  try {
    await lmStudio.oneClickDeploy(modelId);
    message.success(`模型 ${modelId} 部署完成`);
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
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px;"
          >
            <HardwareChipOutline />
          </n-icon>
          本地部署
        </h1>

        <!-- Ollama / LM Studio 切换标签 -->
        <n-tabs
          type="line"
          animated
        >
        <n-tab-pane name="ollama" tab="Ollama">
        <!-- 本地部署卡片 -->
        <n-card
          class="settings-card"
          :bordered="false"
          :segmented="{ content: true, footer: true }"
        >
          <template #header>
            <div class="card-header">
              <n-icon
                :size="20"
                depth="3"
              >
                <HardwareChipOutline />
              </n-icon>
              <span>Ollama 本地模型</span>
            </div>
          </template>

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
                刷新模型列表
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
                重新检测安装状态
              </n-button>
            </n-space>
          </template>
        </n-card>
        </n-tab-pane>

        <n-tab-pane name="lmstudio" tab="LM Studio">
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
                  <HardwareChipOutline />
                </n-icon>
                <span>LM Studio 本地模型</span>
              </div>
            </template>

            <template #header-extra>
              <n-space align="center">
                <n-button
                  size="small"
                  @click="handleOpenLMStudioWebsite"
                >
                  <template #icon>
                    <n-icon><OpenOutline /></n-icon>
                  </template>
                  前往官网下载
                </n-button>
                <n-button
                  size="small"
                  @click="handleRefreshLMStudioModels"
                  :loading="lmStudio.isLoadingModels"
                  :disabled="!lmStudio.isOnline"
                >
                  <template #icon>
                    <n-icon><RefreshOutline /></n-icon>
                  </template>
                  刷新模型列表
                </n-button>
              </n-space>
            </template>

            <!-- Step 1: 连接状态 -->
            <div class="deploy-section">
              <div class="section-title">
                <n-text strong>1. LM Studio 服务</n-text>
              </div>

              <n-space
                align="center"
                justify="space-between"
                style="padding: 8px 0;"
              >
                <n-space align="center">
                  <n-icon
                    size="20"
                    :color="lmStudio.isOnline ? '#18a058' : '#d03050'"
                  >
                    <HardwareChipOutline />
                  </n-icon>
                  <n-text v-if="lmStudio.isOnline">
                    LM Studio 在线
                  </n-text>
                  <n-text
                    v-else
                    type="warning"
                  >
                    未连接到 LM Studio
                  </n-text>
                </n-space>
                <n-space align="center">
                  <n-button
                    size="small"
                    @click="handleCheckLMStudio"
                  >
                    <template #icon>
                      <n-icon><RefreshOutline /></n-icon>
                    </template>
                    检测连接
                  </n-button>

                  <!-- LM Studio 地址编辑 -->
                  <template v-if="lmStudioUrlEditing">
                    <n-input
                      v-model:value="lmStudioUrlEditValue"
                      size="small"
                      style="width: 220px;"
                      placeholder="http://localhost:1234"
                    />
                    <n-button
                      size="small"
                      type="primary"
                      @click="handleSaveLMStudioUrl"
                    >
                      <template #icon>
                        <n-icon><CheckmarkOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-button
                      size="small"
                      @click="lmStudioUrlEditing = false"
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
                      {{ lmStudio.baseUrl }}
                    </n-text>
                    <n-button
                      size="tiny"
                      @click="lmStudioUrlEditing = true; lmStudioUrlEditValue = lmStudio.baseUrl"
                    >
                      编辑
                    </n-button>
                  </template>
                </n-space>
              </n-space>

              <n-alert
                v-if="!lmStudio.isOnline"
                type="info"
                style="margin-top: 8px;"
              >
                LM Studio 没有公开的无人值守安装方式，需要你自行从官网下载安装，
                然后在 LM Studio 的 Developer 标签页里启动本地服务器（默认地址
                http://localhost:1234），这里再点击"检测连接"。
              </n-alert>
            </div>

            <!-- Step 2: 模型下载 -->
            <div
              v-if="lmStudio.isOnline"
              class="deploy-section"
              style="margin-top: 16px;"
            >
              <div class="section-title">
                <n-text strong>2. 下载模型</n-text>
              </div>

              <n-space
                align="center"
                style="margin-top: 8px;"
              >
                <n-input
                  v-model:value="lmStudioPullModelId"
                  size="small"
                  style="width: 280px;"
                  placeholder="模型 ID，如 qwen2.5-7b-instruct 或 HuggingFace 链接"
                />
                <n-button
                  size="small"
                  type="primary"
                  :loading="lmStudio.isPulling"
                  :disabled="!lmStudioPullModelId.trim()"
                  @click="handlePullLMStudioModel"
                >
                  <template #icon>
                    <n-icon><CloudDownloadOutline /></n-icon>
                  </template>
                  下载
                </n-button>
              </n-space>

              <!-- 下载进度 -->
              <n-alert
                v-if="lmStudio.isPulling"
                type="info"
                style="margin-top: 12px;"
              >
                <template #icon>
                  <n-icon><CloudDownloadOutline /></n-icon>
                </template>
                正在下载: {{ lmStudio.pullingModelId }}
                <n-progress
                  v-if="lmStudio.downloadPercent !== null"
                  type="line"
                  :percentage="lmStudio.downloadPercent"
                  :indicator-placement="'inside'"
                  processing
                  style="margin-top: 8px;"
                />
              </n-alert>
            </div>

            <!-- Step 3: 已有模型 & 一键部署 -->
            <div
              v-if="lmStudio.isOnline"
              class="deploy-section"
              style="margin-top: 16px;"
            >
              <div class="section-title">
                <n-text strong>3. 本地模型</n-text>
                <n-text
                  depth="3"
                  style="font-size: 12px; margin-left: 8px;"
                >
                  {{ lmStudio.models.length }} 个
                </n-text>
              </div>

              <n-empty
                v-if="!lmStudio.isLoadingModels && lmStudio.models.length === 0"
                description="暂无本地模型，请先下载"
                style="margin-top: 12px;"
              />

              <div
                v-if="lmStudio.isLoadingModels"
                style="text-align: center; padding: 20px;"
              >
                <n-text depth="3">加载模型列表中...</n-text>
              </div>

              <n-list
                v-if="lmStudio.models.length > 0"
                bordered
                size="small"
                style="margin-top: 8px;"
              >
                <n-list-item
                  v-for="model in lmStudio.models"
                  :key="model.id"
                >
                  <n-thing>
                    <template #header>
                      <n-space align="center" :size="6">
                        <n-icon><HardwareChipOutline /></n-icon>
                        {{ model.id }}
                        <n-tag
                          size="small"
                          :type="model.state === 'loaded' ? 'success' : 'default'"
                        >
                          {{ model.state === 'loaded' ? '已加载' : '未加载' }}
                        </n-tag>
                      </n-space>
                    </template>
                    <template #description>
                      <n-space
                        size="small"
                        style="margin-top: 4px;"
                      >
                        <n-tag
                          v-if="model.publisher"
                          size="small"
                          type="info"
                        >
                          {{ model.publisher }}
                        </n-tag>
                        <n-tag
                          v-if="model.quantization"
                          size="small"
                        >
                          {{ model.quantization }}
                        </n-tag>
                        <n-tag
                          v-if="model.maxContextLength"
                          size="small"
                        >
                          {{ model.maxContextLength.toLocaleString() }} ctx
                        </n-tag>
                        <n-tag
                          v-if="model.arch"
                          size="small"
                          type="success"
                        >
                          {{ model.arch }}
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
                            @click="handleLMStudioOneClickDeploy(model.id)"
                          >
                            <template #icon>
                              <n-icon><RocketOutline /></n-icon>
                            </template>
                            部署
                          </n-button>
                        </template>
                        一键部署并创建聊天配置
                      </n-tooltip>
                      <n-button
                        size="small"
                        @click="handleToggleLMStudioModel(model)"
                      >
                        <template #icon>
                          <n-icon>
                            <StopOutline v-if="model.state === 'loaded'" />
                            <PlayOutline v-else />
                          </n-icon>
                        </template>
                        {{ model.state === 'loaded' ? '卸载' : '加载' }}
                      </n-button>
                    </n-space>
                  </template>
                </n-list-item>
              </n-list>
            </div>
          </n-card>
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
  background: var(--n-color);
}

/* 内容区域 */
.local-deploy-content {
  height: 100%;
}

/* 内容容器 */
.local-deploy-container {
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
  border-radius: $radius-xl;
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
