<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  EmbeddingConfigCard.vue - Embedding 向量模型 API 配置卡片（列表 + 新建/编辑弹窗）
  （从 SettingsView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NForm, NFormItem, NSelect, NInput, NButton, NSpace, NList, NListItem, NThing, NTag, NPopconfirm, NModal, NIcon, NText, NEmpty } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  useSettingsStore,
  PRESET_PROVIDERS,
  type EmbeddingApiConfig,
} from "@/stores/settings";
import {
  DocumentTextOutline,
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline,
} from "@vicons/ionicons5";

const settings = useSettingsStore();
const message = useMessage();

/** Embedding API 配置 - 新建弹窗显示状态 */
const showEmbeddingCreateModal = ref(false);

/** Embedding API 配置 - 编辑弹窗显示状态 */
const showEmbeddingEditModal = ref(false);

/** Embedding API 配置 - 当前编辑的配置对象 */
const editingEmbeddingConfig = ref<EmbeddingApiConfig | null>(null);

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

const providerOptions = computed(() => settings.presetProviderOptions);
</script>

<template>
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
</template>
