<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KbCreateModal.vue - 新建知识库弹窗（表单状态、校验、创建）
  （从 KnowledgeBaseView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import { NButton, NText, NModal, NForm, NFormItem, NInput, NInputNumber, NSelect, NSpace } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import { useKnowledgeBaseStore } from "@/stores/knowledgeBase";
import { useSettingsStore } from "@/stores/settings";

const message = useMessage();
const kbStore = useKnowledgeBaseStore();
const settingsStore = useSettingsStore();

/** 新建知识库弹窗显示状态 */
const showCreateModal = ref(false);

/** 创建中状态 - 禁用按钮并显示加载 */
const creating = ref(false);

/**
 * 创建知识库表单数据
 */
const createForm = ref({
  name: "",                    // 知识库名称
  description: "",             // 知识库描述
  embeddingApiConfigId: "",    // 选中的 Embedding API 配置 ID
  chunk_size: 1000,            // 分块大小 (字符数)
  chunk_overlap: 200,          // 分块重叠大小
});

/**
 * Embedding API 配置下拉选项
 * 从设置 Store 获取可用的 Embedding 配置列表
 */
const embeddingApiConfigOptions = computed(() => {
  // 不在此处插入占位选项 —— n-select 自带 placeholder 属性即可显示
  // 未选择状态；若把占位项塞进 options，由于其 value 为空字符串，会和
  // 初始空值匹配上，导致下拉列表里出现一个被打勾选中的假选项
  return settingsStore.embeddingApiConfigs.map(config => ({
    label: `${config.name} (${config.model})`,
    value: config.id,
  }));
});

/**
 * 创建新的知识库
 * 验证表单后调用 Store 方法创建
 */
const handleCreate = async () => {
  // 表单验证
  if (!createForm.value.name.trim()) {
    message.error("请输入知识库名称");
    return;
  }
  if (!createForm.value.embeddingApiConfigId) {
    message.error("请选择 Embedding API 配置");
    return;
  }
  if (createForm.value.chunk_overlap >= createForm.value.chunk_size) {
    message.error("重叠大小必须小于分块大小");
    return;
  }

  // 取出选中的 Embedding API 配置，把服务商/模型/Base URL 快照到知识库上，
  // 这样导入文档和检索时才会真正使用用户选择的服务商，而不是固定写死成 OpenAI
  const embeddingConfig = settingsStore.embeddingApiConfigs.find(
    c => c.id === createForm.value.embeddingApiConfigId
  );
  if (!embeddingConfig) {
    message.error("选中的 Embedding API 配置不存在，请重新选择");
    return;
  }

  creating.value = true;

  // 调用 Store 方法创建知识库
  const result = await kbStore.createKnowledgeBase({
    name: createForm.value.name,
    description: createForm.value.description,
    embedding_api_config_id: createForm.value.embeddingApiConfigId,
    embedding_provider: embeddingConfig.provider,
    embedding_model: embeddingConfig.model,
    embedding_base_url: embeddingConfig.baseUrl,
    chunk_size: createForm.value.chunk_size,
    chunk_overlap: createForm.value.chunk_overlap,
  });

  creating.value = false;
  
  if (result) {
    message.success("知识库创建成功");
    showCreateModal.value = false;
    // 重置表单
    createForm.value = {
      name: "",
      description: "",
      embeddingApiConfigId: "",
      chunk_size: 1000,
      chunk_overlap: 200,
    };
  } else {
    message.error("创建失败");
  }
};

defineExpose({ open: () => { showCreateModal.value = true; } });
</script>

<template>
  <n-modal
    v-model:show="showCreateModal"
    title="创建知识库"
    preset="card"
    style="width: 600px; max-height: 85vh"
    :content-style="{ overflowY: 'auto' }"
    :mask-closable="false"
  >
    <n-form
      label-placement="left"
      label-width="150px"
    >
      <!-- 名称 -->
      <n-form-item
        label="名称"
        required
      >
        <n-input
          v-model:value="createForm.name"
          placeholder="输入知识库名称"
        />
      </n-form-item>

      <!-- 描述 -->
      <n-form-item label="描述">
        <n-input
          v-model:value="createForm.description"
          type="textarea"
          placeholder="输入描述（可选）"
          :rows="2"
        />
      </n-form-item>

      <!-- Embedding API 配置 -->
      <n-form-item
        label="Embedding API"
        required
      >
        <n-select
          v-model:value="createForm.embeddingApiConfigId"
          :options="embeddingApiConfigOptions"
          placeholder="选择 Embedding API 配置"
        />
        <template #feedback>
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            在「设置」中添加 Embedding API 配置，支持任意 OpenAI 兼容的嵌入模型
          </n-text>
        </template>
      </n-form-item>

      <!-- 分块大小 -->
      <n-form-item label="分块大小（字符数）">
        <n-input-number
          v-model:value="createForm.chunk_size"
          :min="100"
          :max="4000"
          :step="100"
          style="width: 100%"
        />
      </n-form-item>

      <!-- 重叠大小 -->
      <n-form-item label="重叠大小">
        <n-input-number
          v-model:value="createForm.chunk_overlap"
          :min="0"
          :max="1000"
          :step="50"
          style="width: 100%"
        />
      </n-form-item>
    </n-form>

    <!-- 弹窗底部按钮 -->
    <template #footer>
      <n-space justify="end">
        <n-button @click="showCreateModal = false">
          取消
        </n-button>
        <n-button
          type="primary"
          :loading="creating"
          @click="handleCreate"
        >
          创建
        </n-button>
      </n-space>
    </template>
  </n-modal>
</template>
