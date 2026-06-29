<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KnowledgeBaseView.vue - 知识库管理视图组件
  
  功能说明:
  - 知识库列表管理 (创建、删除、选择)
  - 知识库详情查看 (文档列表)
  - 文档导入和管理
  - 检索参数设置 (检索模式、Top-K、相似度阈值)
  - 知识库信息展示

  主要组成部分:
  - 左侧边栏 (知识库列表)
  - 主内容区 (文档列表 / 检索设置)
  - 创建知识库弹窗
  - 空状态提示

  布局说明:
  - 桌面端: 左侧边栏 + 右侧内容区
  - 移动端: 无边栏，点击后全屏显示内容
-->

<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import {
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NButton,
  NList,
  NListItem,
  NThing,
  NTag,
  NText,
  NEmpty,
  NSpin,
  NModal,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSelect,
  NSpace,
  NPopconfirm,
  NIcon,
  NBreadcrumb,
  NBreadcrumbItem,
  NRadioGroup,
  NRadio,
  NSlider,
  NCard,
  NDescriptions,
  NDescriptionsItem,
  NAlert,
  NSwitch,
  NDivider,
  useMessage,
} from "naive-ui";
import {
  Add,
  TrashOutline,
  DocumentTextOutline,
  CloudUploadOutline,
  SettingsOutline,
  ArrowBack,
  Library,
} from "@vicons/ionicons5";
import { useKnowledgeBaseStore, type KnowledgeBase, type Document } from "@/stores/knowledgeBase";
import { useSettingsStore } from "@/stores/settings";

// ============ 状态管理 ============

// 消息提示
const message = useMessage();

// 知识库 Store - 管理知识库和文档
const kbStore = useKnowledgeBaseStore();

// 设置 Store - 获取 Embedding API 配置
const settingsStore = useSettingsStore();

// ============ 本地状态 ============

/** 新建知识库弹窗显示状态 */
const showCreateModal = ref(false);

/** 创建中状态 - 禁用按钮并显示加载 */
const creating = ref(false);

/** 导入中状态 - 显示导入加载动画 */
const importing = ref(false);

/** 当前激活的标签页: "documents" | "settings" */
const activeTab = ref("documents");

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

// ============ 计算属性 ============

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

// ============ 方法函数 ============

/**
 * 组件挂载时加载知识库列表
 */
onMounted(() => {
  kbStore.loadKnowledgeBases();
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

/**
 * 删除知识库
 * 
 * @param kb - 要删除的知识库对象
 */
const handleDeleteKb = async (kb: KnowledgeBase) => {
  const success = await kbStore.deleteKnowledgeBase(kb.id);
  if (success) {
    message.success("删除成功");
  } else {
    message.error("删除失败");
  }
};

/**
 * 选择知识库
 * 设置为当前知识库并切换到文档标签页
 * 
 * @param kb - 要选择的知识库对象
 */
const handleSelectKb = async (kb: KnowledgeBase) => {
  await kbStore.setCurrentKb(kb);
  activeTab.value = "documents";
};

/**
 * 返回知识库列表
 * 清除当前选中的知识库
 */
const handleBack = () => {
  kbStore.setCurrentKb(null);
};

/**
 * 导入文档
 * 打开文件选择器，选择文档后进行向量化处理
 */
const handleImport = async () => {
  // 检查是否有选中的知识库
  if (!kbStore.currentKb) return;

  // 获取对应的 Embedding API 配置
  const embeddingConfig = settingsStore.embeddingApiConfigs.find(
    c => c.id === kbStore.currentKb!.embedding_api_config_id
  );

  // 验证 API 配置存在
  // API Key is retrieved from secure storage by backend (#32)
  if (!embeddingConfig) {
    message.error("关联的 Embedding API 配置已被删除，请重新配置知识库");
    return;
  }

  importing.value = true;

  // 调用 Store 方法选择并导入文档（不再传递 API Key，后端自行读取）
  const success = await kbStore.selectAndImportDocument(
    kbStore.currentKb.id,
  );
  
  importing.value = false;
  
  if (success) {
    message.success("文档导入成功");
  } else {
    message.error("导入失败");
  }
};

/**
 * 删除文档
 * 
 * @param doc - 要删除的文档对象
 */
const handleDeleteDoc = async (doc: Document) => {
  if (!kbStore.currentKb) return;
  
  const success = await kbStore.deleteDocument(doc.id, kbStore.currentKb.id);
  if (success) {
    message.success("删除成功");
  } else {
    message.error("删除失败");
  }
};

/**
 * 格式化文件大小
 * 
 * @param bytes - 文件大小 (字节)
 * @returns 格式化后的大小字符串 (如 1.5 MB)
 */
const formatSize = (bytes: number) => {
  return kbStore.formatFileSize(bytes);
};

/**
 * 格式化日期
 * 
 * @param timestamp - Unix 时间戳 (毫秒)
 * @returns 格式化后的日期字符串
 */
const formatDate = (timestamp: number) => {
  return kbStore.formatDate(timestamp);
};

/**
 * 根据配置 ID 获取配置名称
 * 
 * @param configId - 配置 ID
 * @returns 配置名称 (格式: "名称 (模型名)" 或 "未知配置")
 */
const getEmbeddingConfigName = (configId: string): string => {
  const config = settingsStore.embeddingApiConfigs.find(c => c.id === configId);
  return config ? `${config.name} (${config.model})` : "未知配置";
};

/**
 * 获取文档状态标签
 * 
 * @param status - 文档处理状态
 * @returns 包含类型和文字的对象
 */
const getStatusTag = (status: Document["status"]) => {
  switch (status) {
    case "completed":
      return { type: "success", text: "已完成" };
    case "processing":
      return { type: "warning", text: "处理中" };
    case "error":
      return { type: "error", text: "失败" };
    default:
      return { type: "default", text: status };
  }
};
</script>

<template>
  <!-- 知识库主布局 (包含侧边栏) -->
  <n-layout
    class="kb-view"
    has-sider
  >
    <!-- 侧边栏: 知识库列表 -->
    <n-layout-sider
      v-if="!kbStore.currentKb"
      bordered
      :width="300"
      :native-scrollbar="false"
      class="kb-sidebar"
    >
      <div class="kb-sidebar-content">
        <!-- 侧边栏头部 -->
        <div class="kb-header">
          <h2 class="kb-title">
            <n-icon :size="24">
              <Library />
            </n-icon>
            知识库
          </h2>
          <!-- 新建按钮 -->
          <n-button
            type="primary"
            size="small"
            @click="showCreateModal = true"
          >
            <template #icon>
              <n-icon><Add /></n-icon>
            </template>
            新建
          </n-button>
        </div>

        <!-- 知识库列表 -->
        <div class="kb-list">
          <!-- 加载状态 -->
          <n-spin
            v-if="kbStore.loading"
            class="kb-loading"
          />
          
          <!-- 空状态 -->
          <n-empty
            v-else-if="kbStore.knowledgeBases.length === 0"
            description="暂无知识库"
            class="kb-empty"
          />

          <!-- 知识库列表 -->
          <n-list
            v-else
            hoverable
            clickable
          >
            <n-list-item
              v-for="kb in kbStore.knowledgeBases"
              :key="kb.id"
              @click="handleSelectKb(kb)"
            >
              <n-thing>
                <!-- 知识库名称 -->
                <template #header>
                  <span class="kb-item-name">{{ kb.name }}</span>
                </template>
                
                <!-- 知识库描述 -->
                <template #description>
                  <n-space
                    vertical
                    size="small"
                  >
                    <n-text
                      depth="3"
                      class="kb-item-desc"
                    >
                      {{ kb.description || "无描述" }}
                    </n-text>
                    <!-- 元信息标签 -->
                    <n-space size="small">
                      <n-tag
                        size="small"
                        type="info"
                      >
                        {{ kb.document_count }} 个文档
                      </n-tag>
                      <n-tag
                        size="small"
                        type="default"
                      >
                        {{ getEmbeddingConfigName(kb.embedding_api_config_id) }}
                      </n-tag>
                    </n-space>
                  </n-space>
                </template>
                
                <!-- 删除按钮 -->
                <template #header-extra>
                  <n-popconfirm
                    positive-text="删除"
                    negative-text="取消"
                    @positive-click="handleDeleteKb(kb)"
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
                    确定删除知识库 "{{ kb.name }}"？
                  </n-popconfirm>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>
        </div>
      </div>
    </n-layout-sider>

    <!-- 主内容区: 知识库详情 -->
    <n-layout-content
      v-if="kbStore.currentKb"
      :native-scrollbar="false"
      class="kb-detail"
    >
      <!-- 详情页头部 -->
      <div class="kb-detail-header">
        <!-- 面包屑导航 -->
        <n-space align="center">
          <!-- 返回按钮 -->
          <n-button
            quaternary
            circle
            @click="handleBack"
          >
            <template #icon>
              <n-icon><ArrowBack /></n-icon>
            </template>
          </n-button>
          <n-breadcrumb>
            <n-breadcrumb-item @click="handleBack">
              知识库
            </n-breadcrumb-item>
            <n-breadcrumb-item>{{ kbStore.currentKb.name }}</n-breadcrumb-item>
          </n-breadcrumb>
        </n-space>

        <!-- 标签页切换按钮 -->
        <n-space>
          <n-button
            :type="activeTab === 'documents' ? 'primary' : 'default'"
            @click="activeTab = 'documents'"
          >
            <template #icon>
              <n-icon><DocumentTextOutline /></n-icon>
            </template>
            文档
          </n-button>
          <n-button
            :type="activeTab === 'settings' ? 'primary' : 'default'"
            @click="activeTab = 'settings'"
          >
            <template #icon>
              <n-icon><SettingsOutline /></n-icon>
            </template>
            检索设置
          </n-button>
        </n-space>
      </div>

      <!-- 文档标签页 -->
      <div
        v-if="activeTab === 'documents'"
        class="kb-documents"
      >
        <!-- 文档列表头部 -->
        <div class="kb-documents-header">
          <n-text depth="3">
            共 {{ kbStore.currentKbDocuments.length }} 个文档
          </n-text>
          <!-- 导入按钮 -->
          <n-button
            type="primary"
            :loading="importing"
            @click="handleImport"
          >
            <template #icon>
              <n-icon><CloudUploadOutline /></n-icon>
            </template>
            导入文档
          </n-button>
        </div>

        <!-- 空状态 -->
        <n-empty
          v-if="kbStore.currentKbDocuments.length === 0"
          description="暂无文档"
          class="kb-documents-empty"
        >
          <template #extra>
            <n-button @click="handleImport">
              导入文档
            </n-button>
          </template>
        </n-empty>

        <!-- 文档列表 -->
        <n-list
          v-else
          hoverable
        >
          <n-list-item
            v-for="doc in kbStore.currentKbDocuments"
            :key="doc.id"
          >
            <n-thing>
              <!-- 文件名 -->
              <template #header>
                <span class="doc-name">{{ doc.filename }}</span>
              </template>
              
              <!-- 文件详情 -->
              <template #description>
                <n-space
                  vertical
                  size="small"
                >
                  <!-- 内容预览 -->
                  <n-text
                    depth="3"
                    class="doc-preview"
                  >
                    {{ doc.content_preview || "无预览" }}
                  </n-text>
                  <!-- 元信息标签 -->
                  <n-space
                    size="small"
                    align="center"
                  >
                    <!-- 状态标签 -->
                    <n-tag
                      :type="getStatusTag(doc.status).type as any"
                      size="small"
                    >
                      {{ getStatusTag(doc.status).text }}
                    </n-tag>
                    <!-- 文件大小 -->
                    <n-tag
                      size="small"
                      type="default"
                    >
                      {{ formatSize(doc.file_size) }}
                    </n-tag>
                    <!-- 块数量 -->
                    <n-tag
                      size="small"
                      type="default"
                    >
                      {{ doc.chunk_count }} 块
                    </n-tag>
                    <!-- 创建日期 -->
                    <n-text
                      depth="3"
                      class="doc-date"
                    >
                      {{ formatDate(doc.created_at) }}
                    </n-text>
                  </n-space>
                  <!-- 错误信息 -->
                  <n-text
                    v-if="doc.error_message"
                    type="error"
                    class="doc-error"
                  >
                    {{ doc.error_message }}
                  </n-text>
                </n-space>
              </template>
              
              <!-- 删除按钮 -->
              <template #header-extra>
                <n-popconfirm
                  positive-text="删除"
                  negative-text="取消"
                  @positive-click="handleDeleteDoc(doc)"
                >
                  <template #trigger>
                    <n-button
                      quaternary
                      circle
                      size="small"
                      type="error"
                    >
                      <template #icon>
                        <n-icon><TrashOutline /></n-icon>
                      </template>
                    </n-button>
                  </template>
                  确定删除文档 "{{ doc.filename }}"？
                </n-popconfirm>
              </template>
            </n-thing>
          </n-list-item>
        </n-list>
      </div>

      <!-- 检索设置标签页 -->
      <div
        v-else-if="activeTab === 'settings'"
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
                  :min="0"
                  :max="1"
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
    </n-layout-content>

    <!-- 空状态: 移动端未选中知识库 -->
    <n-layout-content
      v-else
      :native-scrollbar="false"
      class="kb-empty-content"
    >
      <div class="kb-empty-state">
        <n-empty description="请从左侧选择一个知识库">
          <template #icon>
            <n-icon :size="48">
              <Library />
            </n-icon>
          </template>
        </n-empty>
      </div>
    </n-layout-content>
  </n-layout>

  <!-- 新建知识库弹窗 -->
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

<style scoped lang="scss">
/* 主容器 */
.kb-view {
  height: 100%;
  background: var(--n-color);
}

/* 侧边栏背景 */
.kb-sidebar {
  background: var(--n-color-embed);
}

/* 侧边栏内容 */
.kb-sidebar-content {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
}

/* 侧边栏头部 */
.kb-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

/* 标题样式 */
.kb-title {
  font-size: 18px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
}

/* 列表区域 */
.kb-list {
  flex: 1;
  overflow-y: auto;
}

/* 加载状态 */
.kb-loading {
  padding: 40px;
}

/* 空状态 */
.kb-empty {
  padding: 60px 20px;
}

/* 知识库项名称 */
.kb-item-name {
  font-weight: 600;
  font-size: 15px;
}

/* 知识库项描述 */
.kb-item-desc {
  font-size: 13px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  line-clamp: 2;
}

/* 知识库项元信息标签 —— Embedding 配置名可能很长（用户自定义名称 + 模型名），
   弹性容器默认不会收缩到比内容更窄，必须显式 min-width: 0 才能让省略号截断
   生效，否则标签会把侧边栏卡片撑出一条水平滚动条 */
:deep(.n-list-item .n-thing-main),
:deep(.n-list-item .n-space),
:deep(.n-list-item .n-space-item) {
  min-width: 0;
}

:deep(.n-list-item .n-tag) {
  max-width: 100%;
  min-width: 0;
}

:deep(.n-list-item .n-tag .n-tag__content) {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 220px;
}

/* 详情页主内容区 —— 之前没有任何 padding，面包屑/返回按钮直接贴着布局
   容器的边缘，标签页按钮和下面的内容区之间也没有间距，显得很挤 */
.kb-detail {
  height: 100%;
}

:deep(.kb-detail .n-scrollbar-content) {
  padding: 24px 32px;
}

/* 详情页头部：面包屑导航 + 标签页切换按钮，两行之间留出间距 */
.kb-detail-header {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-bottom: 24px;
}

/* 文档标签页 */
.kb-documents-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.kb-documents-empty {
  padding: 60px 20px;
}

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

/* 未选中知识库时的空状态占位区域
   n-layout-content 在 native-scrollbar=false 时会把 slot 内容包进自己的
   滚动容器 div 里，直接在 n-layout-content 根节点上 flex 居中不会影响到
   被包了一层的子内容，所以改成给 slot 内部自己的 wrapper 设置高度 + 居中 */
.kb-empty-content :deep(.n-scrollbar-content) {
  height: 100%;
}

.kb-empty-state {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
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
