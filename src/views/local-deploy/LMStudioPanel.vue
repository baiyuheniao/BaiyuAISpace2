<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  LMStudioPanel.vue - LM Studio 标签页

  LM Studio 连接状态检测 / 模型下载 / 加载卸载 / 一键部署。
  页面初始化时的连接探测仍在 LocalDeployView.vue 的 onMounted 里，这里只读 store。
-->

<script setup lang="ts">
import { ref } from "vue";
import { open as openExternalUrl } from "@tauri-apps/plugin-shell";
import {
  NCard,
  NInput,
  NButton,
  NSpace,
  NList,
  NListItem,
  NThing,
  NTag,
  NIcon,
  NText,
  NEmpty,
  NProgress,
  NAlert,
  NTooltip,
} from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import { useLMStudioStore } from "@/stores/lmStudio";
import {
  HardwareChipOutline,
  RefreshOutline,
  CheckmarkOutline,
  CloseOutline,
  PlayOutline,
  StopOutline,
  RocketOutline,
  CloudDownloadOutline,
  OpenOutline,
} from "@vicons/ionicons5";

// ============ 状态管理 ============

// LM Studio Store
const lmStudio = useLMStudioStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

// ============ LM Studio 相关状态 ============

/** LM Studio 下载模型表单数据 (模型 ID，如 "qwen2.5-7b-instruct") */
const lmStudioPullModelId = ref("");

/** LM Studio 地址编辑状态 */
const lmStudioUrlEditing = ref(false);

/** LM Studio 地址编辑值 */
const lmStudioUrlEditValue = ref(lmStudio.baseUrl);

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
</script>

<template>
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
            :color="lmStudio.isOnline ? 'var(--color-ink)' : 'var(--color-ink-faint)'"
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
</template>
