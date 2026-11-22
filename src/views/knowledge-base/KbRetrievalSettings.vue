<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KbRetrievalSettings.vue - 检索设置标签页：全局检索参数 + 当前知识库信息
  （从 KnowledgeBaseView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { computed } from "vue";
import { NSpace, NText, NForm, NFormItem, NSelect, NInputNumber, NRadioGroup, NRadio, NSlider, NCard, NDescriptions, NDescriptionsItem, NAlert, NSwitch, NDivider } from "naive-ui";
import { useKnowledgeBaseStore, MIN_SIMILARITY_THRESHOLD, MAX_SIMILARITY_THRESHOLD } from "@/stores/knowledgeBase";
import { useSettingsStore } from "@/stores/settings";
import { getEmbeddingConfigName } from "./helpers";

const kbStore = useKnowledgeBaseStore();
const settingsStore = useSettingsStore();

const rerankerApiConfigOptions = computed(() => {
  return settingsStore.rerankerApiConfigs.map(config => ({
    label: `${config.name} (${config.model})`,
    value: config.id,
  }));
});

/**
 * 检索模式选项
 */
const retrievalModeOptions = [
  { label: "混合检索（推荐）", value: "hybrid", desc: "向量相似度 + 关键词匹配" },
  { label: "向量检索", value: "vector", desc: "纯语义相似度" },
  { label: "关键词检索", value: "keyword", desc: "精确术语匹配" },
];

/**
 * 格式化日期
 * 
 * @param timestamp - Unix 时间戳 (毫秒)
 * @returns 格式化后的日期字符串
 */
const formatDate = (timestamp: number) => {
  return kbStore.formatDate(timestamp);
};
</script>

<template>
  <div
    v-if="kbStore.currentKb"
    class="kb-settings"
  >
    <!-- 检索设置卡片 -->
    <n-card
      title="检索设置"
      class="settings-card"
    >
      <n-alert
        type="info"
        :show-icon="true"
        style="margin-bottom: 16px;"
      >
        以下检索参数为全局设置，会应用于你检索的每一个知识库，并非仅对当前知识库生效
      </n-alert>
      <n-form
        label-placement="left"
        label-width="120px"
      >
        <!-- 检索模式 -->
        <n-form-item label="检索模式">
          <n-radio-group v-model:value="kbStore.retrievalSettings.mode">
            <n-space vertical>
              <n-radio
                v-for="option in retrievalModeOptions"
                :key="option.value"
                :value="option.value"
              >
                <div class="radio-option">
                  <span class="radio-label">{{ option.label }}</span>
                  <n-text
                    depth="3"
                    class="radio-desc"
                  >
                    {{ option.desc }}
                  </n-text>
                </div>
              </n-radio>
            </n-space>
          </n-radio-group>
        </n-form-item>

        <!-- 返回数量 (Top-K) -->
        <n-form-item label="返回数量 (Top-K)">
          <div class="slider-row">
            <n-slider
              v-model:value="kbStore.retrievalSettings.topK"
              :min="1"
              :max="20"
              :step="1"
              show-tooltip
            />
            <n-text
              depth="3"
              class="slider-value"
            >
              {{ kbStore.retrievalSettings.topK }} 个结果
            </n-text>
          </div>
        </n-form-item>

        <!-- 相似度阈值 -->
        <n-form-item label="相似度阈值">
          <div class="slider-row">
            <n-slider
              v-model:value="kbStore.retrievalSettings.similarityThreshold"
              :min="MIN_SIMILARITY_THRESHOLD"
              :max="MAX_SIMILARITY_THRESHOLD"
              :step="0.05"
              show-tooltip
            />
            <n-text
              depth="3"
              class="slider-value"
            >
              {{ kbStore.retrievalSettings.similarityThreshold }}
            </n-text>
          </div>
        </n-form-item>

        <n-divider />

        <!-- Reranker 精排 -->
        <n-form-item label="启用 Reranker">
          <n-switch v-model:value="kbStore.retrievalSettings.enableReranker" />
        </n-form-item>

        <template v-if="kbStore.retrievalSettings.enableReranker">
          <n-form-item label="Reranker 配置">
            <n-select
              v-model:value="kbStore.retrievalSettings.rerankerConfigId"
              :options="rerankerApiConfigOptions"
              placeholder="选择 Reranker 配置"
              clearable
            />
          </n-form-item>
          <n-form-item label="精排保留 Top-N">
            <n-input-number
              v-model:value="kbStore.retrievalSettings.rerankTopN"
              :min="1"
              :max="50"
              :placeholder="`默认与 Top-K 相同 (${kbStore.retrievalSettings.topK})`"
              clearable
            />
          </n-form-item>
        </template>
      </n-form>
    </n-card>

    <!-- 知识库信息卡片 -->
    <n-card
      title="知识库信息"
      class="settings-card"
    >
      <n-descriptions
        bordered
        :column="2"
      >
        <n-descriptions-item label="名称">
          {{ kbStore.currentKb.name }}
        </n-descriptions-item>
        <n-descriptions-item label="Embedding API">
          {{ getEmbeddingConfigName(kbStore.currentKb.embedding_api_config_id) }}
        </n-descriptions-item>
        <n-descriptions-item label="文档数量">
          {{ kbStore.currentKb.document_count }}
        </n-descriptions-item>
        <n-descriptions-item label="分块大小">
          {{ kbStore.currentKb.chunk_size }}
        </n-descriptions-item>
        <n-descriptions-item label="重叠大小">
          {{ kbStore.currentKb.chunk_overlap }}
        </n-descriptions-item>
        <n-descriptions-item
          label="创建时间"
          :span="2"
        >
          {{ formatDate(kbStore.currentKb.created_at) }}
        </n-descriptions-item>
      </n-descriptions>
    </n-card>
  </div>
</template>

<style scoped lang="scss">
/* 检索设置标签页：两张卡片之间留出间距 */
.kb-settings .settings-card {
  margin-bottom: 20px;

  &:last-child {
    margin-bottom: 0;
  }
}

/* 检索模式单选项：标签和说明文字分两行展示 */
.radio-option {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.radio-label {
  font-size: 14px;
}

.radio-desc {
  font-size: 12px;
}

/* 检索设置标签页里 Top-K / 相似度阈值 两个滑块后面跟着一段数值文字。
   n-form-item-blank 默认就是 flex 布局，n-slider 和 n-text 各自占等分空间，
   而 n-slider 自身不会收缩，导致数值文字被挤到几乎零宽度的空间里，
   逐字换行（例如 "5 个结果" 拆成两行）。这里显式让滑块占满剩余空间，
   数值文字不参与收缩，两者之间留出固定间距 */
.slider-row {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
}

.slider-row .n-slider {
  flex: 1;
  min-width: 0;
}

.slider-value {
  flex-shrink: 0;
  white-space: nowrap;
}
</style>
