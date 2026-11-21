<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  RerankerConfigCard.vue - Reranker 精排模型 API 配置卡片（列表 + 新建/编辑弹窗）
  （从 SettingsView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref } from "vue";
import { NCard, NForm, NFormItem, NInput, NButton, NSpace, NList, NListItem, NThing, NPopconfirm, NModal, NIcon, NText, NEmpty } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  useSettingsStore,
  type RerankerApiConfig,
} from "@/stores/settings";
import {
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline,
} from "@vicons/ionicons5";

const settings = useSettingsStore();
const message = useMessage();

/** Reranker API 配置 - 新建弹窗 */
const showRerankerCreateModal = ref(false);

/** Reranker API 配置 - 编辑弹窗 */
const showRerankerEditModal = ref(false);

/** Reranker API 配置 - 当前编辑的配置对象 */
const editingRerankerConfig = ref<RerankerApiConfig | null>(null);

/** Reranker API 配置表单数据 */
const rerankerFormData = ref({
  name: "",
  provider: "custom",
  baseUrl: "https://api.cohere.com",
  model: "rerank-multilingual-v3.0",
  apiKey: "",
});

const resetRerankerForm = () => {
  rerankerFormData.value = { name: "", provider: "custom", baseUrl: "https://api.cohere.com", model: "rerank-multilingual-v3.0", apiKey: "" };
};

// ============ Reranker CRUD ============

const openRerankerCreateModal = () => {
  resetRerankerForm();
  showRerankerCreateModal.value = true;
};

const openRerankerEditModal = (config: RerankerApiConfig) => {
  editingRerankerConfig.value = config;
  rerankerFormData.value = { name: config.name, provider: config.provider, baseUrl: config.baseUrl, model: config.model, apiKey: config.apiKey };
  showRerankerEditModal.value = true;
};

const handleRerankerCreate = () => {
  if (!rerankerFormData.value.name.trim()) { message.error("请输入配置名称"); return; }
  if (!rerankerFormData.value.model.trim()) { message.error("请输入模型名称"); return; }
  if (!rerankerFormData.value.apiKey.trim()) { message.error("请输入 API Key"); return; }
  settings.createRerankerApiConfig(rerankerFormData.value.name, rerankerFormData.value.provider, rerankerFormData.value.model, rerankerFormData.value.apiKey, rerankerFormData.value.baseUrl);
  message.success("Reranker 配置已创建");
  showRerankerCreateModal.value = false;
  resetRerankerForm();
};

const handleRerankerUpdate = () => {
  if (!editingRerankerConfig.value) return;
  if (!rerankerFormData.value.name.trim()) { message.error("请输入配置名称"); return; }
  if (!rerankerFormData.value.model.trim()) { message.error("请输入模型名称"); return; }
  settings.updateRerankerApiConfig(editingRerankerConfig.value.id, { name: rerankerFormData.value.name, provider: rerankerFormData.value.provider, baseUrl: rerankerFormData.value.baseUrl, model: rerankerFormData.value.model, apiKey: rerankerFormData.value.apiKey });
  message.success("Reranker 配置已更新");
  showRerankerEditModal.value = false;
  editingRerankerConfig.value = null;
};

const handleRerankerDelete = (configId: string) => {
  settings.deleteRerankerApiConfig(configId);
  message.success("Reranker 配置已删除");
};
</script>

<template>
  <!-- Reranker API 配置卡片 -->
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon :size="20" depth="3">
          <CubeOutline />
        </n-icon>
        <span>Reranker 精排模型 API 配置</span>
        <n-button type="primary" size="small" @click="openRerankerCreateModal">
          <template #icon><n-icon><Add /></n-icon></template>
          新建配置
        </n-button>
      </div>
    </template>

    <n-list v-if="settings.rerankerApiConfigs.length > 0" hoverable clickable>
      <n-list-item
        v-for="config in settings.rerankerApiConfigs"
        :key="config.id"
      >
        <n-thing>
          <template #header>
            <span>{{ config.name }}</span>
          </template>
          <template #description>
            <n-space vertical size="small">
              <n-text depth="3">
                <n-icon :size="14" style="margin-right: 4px;"><CubeOutline /></n-icon>
                模型: {{ config.model }}
              </n-text>
              <n-text depth="3">
                <n-icon :size="14" style="margin-right: 4px;"><LinkOutline /></n-icon>
                {{ config.baseUrl }}
              </n-text>
            </n-space>
          </template>
          <template #header-extra>
            <n-space>
              <n-button quaternary circle size="small" @click.stop="openRerankerEditModal(config)">
                <template #icon><n-icon><CreateOutline /></n-icon></template>
              </n-button>
              <n-popconfirm positive-text="删除" negative-text="取消" @positive-click="handleRerankerDelete(config.id)">
                <template #trigger>
                  <n-button quaternary circle size="small" type="error" @click.stop>
                    <template #icon><n-icon><TrashOutline /></n-icon></template>
                  </n-button>
                </template>
                确定删除 Reranker 配置 "{{ config.name }}"？
              </n-popconfirm>
            </n-space>
          </template>
        </n-thing>
      </n-list-item>
    </n-list>

    <n-empty v-else description="暂无 Reranker 配置" />

    <template v-if="settings.rerankerApiConfigs.length > 0" #footer>
      <n-text depth="3" style="font-size: 12px;">
        <n-icon :size="12" style="margin-right: 4px;"><CheckmarkCircle /></n-icon>
        Reranker 用于对 RAG 检索结果进行二次精排，兼容 Cohere / Jina / Voyage 等 API 格式
      </n-text>
    </template>
  </n-card>

  <!-- 新建 Reranker API 配置弹窗 -->
  <n-modal v-model:show="showRerankerCreateModal" title="新建 Reranker API 配置" preset="card" style="width: 500px" :mask-closable="false">
    <n-form label-placement="left" label-width="120px">
      <n-form-item label="配置名称" required>
        <n-input v-model:value="rerankerFormData.name" placeholder="例如：Cohere Reranker" />
      </n-form-item>
      <n-form-item label="Base URL" required>
        <n-input v-model:value="rerankerFormData.baseUrl" placeholder="https://api.cohere.com" />
        <template #feedback>
          <n-text depth="3" style="font-size: 12px;">Cohere-compatible API 地址，需支持 POST /v1/rerank</n-text>
        </template>
      </n-form-item>
      <n-form-item label="模型名称" required>
        <n-input v-model:value="rerankerFormData.model" placeholder="例如：rerank-multilingual-v3.0" />
      </n-form-item>
      <n-form-item label="API Key" required>
        <n-input v-model:value="rerankerFormData.apiKey" type="password" show-password-on="click" placeholder="输入 API Key" />
      </n-form-item>
    </n-form>
    <template #footer>
      <n-space justify="end">
        <n-button @click="showRerankerCreateModal = false">取消</n-button>
        <n-button type="primary" @click="handleRerankerCreate">创建</n-button>
      </n-space>
    </template>
  </n-modal>

  <!-- 编辑 Reranker API 配置弹窗 -->
  <n-modal v-model:show="showRerankerEditModal" title="编辑 Reranker API 配置" preset="card" style="width: 500px" :mask-closable="false">
    <n-form label-placement="left" label-width="120px">
      <n-form-item label="配置名称" required>
        <n-input v-model:value="rerankerFormData.name" placeholder="例如：Cohere Reranker" />
      </n-form-item>
      <n-form-item label="Base URL" required>
        <n-input v-model:value="rerankerFormData.baseUrl" placeholder="https://api.cohere.com" />
      </n-form-item>
      <n-form-item label="模型名称" required>
        <n-input v-model:value="rerankerFormData.model" placeholder="例如：rerank-multilingual-v3.0" />
      </n-form-item>
      <n-form-item label="API Key">
        <n-input v-model:value="rerankerFormData.apiKey" type="password" show-password-on="click" placeholder="留空表示不修改" />
        <template #feedback>
          <n-text depth="3" style="font-size: 12px;">留空表示保持原 API Key 不变</n-text>
        </template>
      </n-form-item>
    </n-form>
    <template #footer>
      <n-space justify="end">
        <n-button @click="showRerankerEditModal = false">取消</n-button>
        <n-button type="primary" @click="handleRerankerUpdate">保存</n-button>
      </n-space>
    </template>
  </n-modal>
</template>
