<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { ref, onMounted, h } from "vue";
import { useRouter } from "vue-router";
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
  NSelect,
  NSpace,
  NPopconfirm,
  NTooltip,
  NIcon,
  NUpload,
  NBreadcrumb,
  NBreadcrumbItem,
  NRadioGroup,
  NRadio,
  NSlider,
  useMessage,
  useDialog,
} from "naive-ui";
import {
  Add,
  TrashOutline,
  DocumentTextOutline,
  CloudUploadOutline,
  SettingsOutline,
  ArrowBack,
  Library,
  SearchOutline,
  ServerOutline,
} from "@vicons/ionicons5";
import { useKnowledgeBaseStore, type KnowledgeBase, type Document } from "@/stores/knowledgeBase";
import { useSettingsStore } from "@/stores/settings";

const router = useRouter();
const message = useMessage();
const dialog = useDialog();
const kbStore = useKnowledgeBaseStore();
const settingsStore = useSettingsStore();

// UI State
const showCreateModal = ref(false);
const creating = ref(false);
const importing = ref(false);
const activeTab = ref("documents"); // documents, settings

// Create form
const createForm = ref({
  name: "",
  description: "",
  embedding_provider: "openai",
  embedding_model: "text-embedding-3-small",
  chunk_size: 1000,
  chunk_overlap: 200,
});

const embeddingOptions = [
  { label: "OpenAI text-embedding-3-small", value: "text-embedding-3-small", provider: "openai", dim: 1536 },
  { label: "OpenAI text-embedding-3-large", value: "text-embedding-3-large", provider: "openai", dim: 3072 },
  { label: "智谱 embedding-2", value: "embedding-2", provider: "zhipu", dim: 1024 },
  { label: "硅基流动 bge-large-zh", value: "BAAI/bge-large-zh-v1.5", provider: "siliconflow", dim: 1024 },
];

// Retrieval mode options
const retrievalModeOptions = [
  { label: "混合检索（推荐）", value: "hybrid", desc: "向量相似度 + 关键词匹配" },
  { label: "向量检索", value: "vector", desc: "纯语义相似度" },
  { label: "关键词检索", value: "keyword", desc: "精确术语匹配" },
];

onMounted(() => {
  kbStore.loadKnowledgeBases();
});

// Create knowledge base
const handleCreate = async () => {
  if (!createForm.value.name.trim()) {
    message.error("请输入知识库名称");
    return;
  }

  creating.value = true;
  const selectedModel = embeddingOptions.find(m => m.value === createForm.value.embedding_model);
  
  const result = await kbStore.createKnowledgeBase({
    name: createForm.value.name,
    description: createForm.value.description,
    embedding_provider: selectedModel?.provider || "openai",
    embedding_model: createForm.value.embedding_model,
    chunk_size: createForm.value.chunk_size,
    chunk_overlap: createForm.value.chunk_overlap,
  });

  creating.value = false;
  
  if (result) {
    message.success("知识库创建成功");
    showCreateModal.value = false;
    // Reset form
    createForm.value = {
      name: "",
      description: "",
      embedding_provider: "openai",
      embedding_model: "text-embedding-3-small",
      chunk_size: 1000,
      chunk_overlap: 200,
    };
  } else {
    message.error("创建失败");
  }
};

// Delete knowledge base
const handleDeleteKb = async (kb: KnowledgeBase) => {
  const success = await kbStore.deleteKnowledgeBase(kb.id);
  if (success) {
    message.success("删除成功");
  } else {
    message.error("删除失败");
  }
};

// Select knowledge base
const handleSelectKb = async (kb: KnowledgeBase) => {
  await kbStore.setCurrentKb(kb);
  activeTab.value = "documents";
};

// Back to list
const handleBack = () => {
  kbStore.setCurrentKb(null);
};

// Import document
const handleImport = async () => {
  if (!kbStore.currentKb) return;
  
  // Get API config for embedding provider
  const config = settingsStore.apiConfigs.find(
    c => c.provider === kbStore.currentKb!.embedding_provider
  );
  if (!config?.apiKey) {
    message.error(`请先在设置中创建 ${kbStore.currentKb.embedding_provider} 的 API 配置并填写 API Key`);
    return;
  }

  importing.value = true;
  const success = await kbStore.selectAndImportDocument(kbStore.currentKb.id, config.apiKey);
  importing.value = false;
  
  if (success) {
    message.success("文档导入成功");
  } else {
    message.error("导入失败");
  }
};

// Delete document
const handleDeleteDoc = async (doc: Document) => {
  if (!kbStore.currentKb) return;
  
  const success = await kbStore.deleteDocument(doc.id, kbStore.currentKb.id);
  if (success) {
    message.success("删除成功");
  } else {
    message.error("删除失败");
  }
};

// Update retrieval settings
const handleUpdateSettings = () => {
  message.success("设置已保存");
};

// Format file size
const formatSize = (bytes: number) => {
  return kbStore.formatFileSize(bytes);
};

// Format date
const formatDate = (timestamp: number) => {
  return kbStore.formatDate(timestamp);
};

// Get status tag
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
  <n-layout class="kb-view" has-sider>
    <!-- Sidebar: Knowledge Base List -->
    <n-layout-sider
      v-if="!kbStore.currentKb"
      bordered
      :width="300"
      :native-scrollbar="false"
      class="kb-sidebar"
    >
      <div class="kb-sidebar-content">
        <div class="kb-header">
          <h2 class="kb-title">
            <n-icon :size="24"><Library /></n-icon>
            知识库
          </h2>
          <n-button type="primary" size="small" @click="showCreateModal = true">
            <template #icon>
              <n-icon><Add /></n-icon>
            </template>
            新建
          </n-button>
        </div>

        <div class="kb-list">
          <n-spin v-if="kbStore.loading" class="kb-loading" />
          
          <n-empty
            v-else-if="kbStore.knowledgeBases.length === 0"
            description="暂无知识库"
            class="kb-empty"
          >
            <template #extra>
              <n-button @click="showCreateModal = true">
                创建知识库
              </n-button>
            </template>
          </n-empty>

          <n-list v-else hoverable clickable>
            <n-list-item
              v-for="kb in kbStore.knowledgeBases"
              :key="kb.id"
              @click="handleSelectKb(kb)"
            >
              <n-thing>
                <template #header>
                  <span class="kb-item-name">{{ kb.name }}</span>
                </template>
                <template #description>
                  <n-space vertical size="small">
                    <n-text depth="3" class="kb-item-desc">
                      {{ kb.description || "无描述" }}
                    </n-text>
                    <n-space size="small">
                      <n-tag size="small" type="info">
                        {{ kb.document_count }} 个文档
                      </n-tag>
                      <n-tag size="small" type="default">
                        {{ kb.embedding_model }}
                      </n-tag>
                    </n-space>
                  </n-space>
                </template>
                <template #header-extra>
                  <n-popconfirm
                    @positive-click="handleDeleteKb(kb)"
                    positive-text="删除"
                    negative-text="取消"
                  >
                    <template #trigger>
                      <n-button quaternary circle size="small" type="error" @click.stop>
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

    <!-- Main Content: Knowledge Base Detail -->
    <n-layout-content
      v-if="kbStore.currentKb"
      :native-scrollbar="false"
      class="kb-detail"
    >
      <!-- Header -->
      <div class="kb-detail-header">
        <n-space align="center">
          <n-button quaternary circle @click="handleBack">
            <template #icon>
              <n-icon><ArrowBack /></n-icon>
            </template>
          </n-button>
          <n-breadcrumb>
            <n-breadcrumb-item @click="handleBack">知识库</n-breadcrumb-item>
            <n-breadcrumb-item>{{ kbStore.currentKb.name }}</n-breadcrumb-item>
          </n-breadcrumb>
        </n-space>

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

      <!-- Documents Tab -->
      <div v-if="activeTab === 'documents'" class="kb-documents">
        <div class="kb-documents-header">
          <n-text depth="3">
            共 {{ kbStore.currentKbDocuments.length }} 个文档
          </n-text>
          <n-button type="primary" :loading="importing" @click="handleImport">
            <template #icon>
              <n-icon><CloudUploadOutline /></n-icon>
            </template>
            导入文档
          </n-button>
        </div>

        <n-empty
          v-if="kbStore.currentKbDocuments.length === 0"
          description="暂无文档"
          class="kb-documents-empty"
        >
          <template #extra>
            <n-button @click="handleImport">导入文档</n-button>
          </template>
        </n-empty>

        <n-list v-else hoverable>
          <n-list-item
            v-for="doc in kbStore.currentKbDocuments"
            :key="doc.id"
          >
            <n-thing>
              <template #header>
                <span class="doc-name">{{ doc.filename }}</span>
              </template>
              <template #description>
                <n-space vertical size="small">
                  <n-text depth="3" class="doc-preview">
                    {{ doc.content_preview || "无预览" }}
                  </n-text>
                  <n-space size="small" align="center">
                    <n-tag :type="getStatusTag(doc.status).type as any" size="small">
                      {{ getStatusTag(doc.status).text }}
                    </n-tag>
                    <n-tag size="small" type="default">{{ formatSize(doc.file_size) }}</n-tag>
                    <n-tag size="small" type="default">{{ doc.chunk_count }} 块</n-tag>
                    <n-text depth="3" class="doc-date">{{ formatDate(doc.created_at) }}</n-text>
                  </n-space>
                  <n-text v-if="doc.error_message" type="error" class="doc-error">
                    {{ doc.error_message }}
                  </n-text>
                </n-space>
              </template>
              <template #header-extra>
                <n-popconfirm
                  @positive-click="handleDeleteDoc(doc)"
                  positive-text="删除"
                  negative-text="取消"
                >
                  <template #trigger>
                    <n-button quaternary circle size="small" type="error">
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

      <!-- Settings Tab -->
      <div v-else-if="activeTab === 'settings'" class="kb-settings">
        <n-card title="检索设置" class="settings-card">
          <n-form label-placement="left" label-width="120px">
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
                      <n-text depth="3" class="radio-desc">{{ option.desc }}</n-text>
                    </div>
                  </n-radio>
                </n-space>
              </n-radio-group>
            </n-form-item>

            <n-form-item label="返回数量 (Top-K)">
              <n-slider
                v-model:value="kbStore.retrievalSettings.topK"
                :min="1"
                :max="20"
                :step="1"
                show-tooltip
              />
              <n-text depth="3">{{ kbStore.retrievalSettings.topK }} 个结果</n-text>
            </n-form-item>

            <n-form-item label="相似度阈值">
              <n-slider
                v-model:value="kbStore.retrievalSettings.similarityThreshold"
                :min="0"
                :max="1"
                :step="0.05"
                show-tooltip
              />
              <n-text depth="3">{{ kbStore.retrievalSettings.similarityThreshold }}</n-text>
            </n-form-item>

            <n-form-item>
              <n-button type="primary" @click="handleUpdateSettings">
                保存设置
              </n-button>
            </n-form-item>
          </n-form>
        </n-card>

        <n-card title="知识库信息" class="settings-card">
          <n-descriptions bordered :column="2">
            <n-descriptions-item label="名称">
              {{ kbStore.currentKb.name }}
            </n-descriptions-item>
            <n-descriptions-item label="Embedding 模型">
              {{ kbStore.currentKb.embedding_model }}
            </n-descriptions-item>
            <n-descriptions-item label="向量维度">
              {{ kbStore.currentKb.embedding_dim }}
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
            <n-descriptions-item label="创建时间" :span="2">
              {{ formatDate(kbStore.currentKb.created_at) }}
            </n-descriptions-item>
          </n-descriptions>
        </n-card>
      </div>
    </n-layout-content>

    <!-- Empty State: When no KB selected on mobile -->
    <n-layout-content
      v-else
      :native-scrollbar="false"
      class="kb-empty-content"
    >
      <n-empty description="请从左侧选择知识库或创建新知识库">
        <template #icon>
          <n-icon :size="64" depth="3"><Library /></n-icon>
        </template>
      </n-empty>
    </n-layout-content>
  </n-layout>

  <!-- Create Knowledge Base Modal -->
  <n-modal
    v-model:show="showCreateModal"
    title="创建知识库"
    preset="card"
    style="width: 500px"
    :mask-closable="false"
  >
    <n-form label-placement="left" label-width="100px">
      <n-form-item label="名称" required>
        <n-input v-model:value="createForm.name" placeholder="输入知识库名称" />
      </n-form-item>

      <n-form-item label="描述">
        <n-input
          v-model:value="createForm.description"
          type="textarea"
          placeholder="输入描述（可选）"
          :rows="2"
        />
      </n-form-item>

      <n-form-item label="Embedding" required>
        <n-select
          v-model:value="createForm.embedding_model"
          :options="embeddingOptions.map(o => ({ label: o.label, value: o.value }))"
          placeholder="选择 Embedding 模型"
        />
      </n-form-item>

      <n-form-item label="分块大小">
        <n-input-number
          v-model:value="createForm.chunk_size"
          :min="100"
          :max="4000"
          :step="100"
          style="width: 100%"
        />
      </n-form-item>

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

    <template #footer>
      <n-space justify="end">
        <n-button @click="showCreateModal = false">取消</n-button>
        <n-button type="primary" :loading="creating" @click="handleCreate">
          创建
        </n-button>
      </n-space>
    </template>
  </n-modal>
</template>

<style scoped lang="scss">
.kb-view {
  height: 100%;
  background: var(--n-color);
}

.kb-sidebar {
  background: var(--n-color-embed);
}

.kb-sidebar-content {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px;
}

.kb-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.kb-title {
  font-size: 18px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
}

.kb-list {
  flex: 1;
  overflow-y: auto;
}

.kb-loading {
  padding: 40px;
}

.kb-empty {
  padding: 60px 20px;
}

.kb-item-name {
  font-weight: 600;
  font-size: 15px;
}

.kb-item-desc {
  font-size: 13px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.kb-detail {
  padding: 24px;
}

.kb-detail-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--n-border-color);
}

.kb-documents-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.kb-documents-empty {
  padding: 80px;
}

.doc-name {
  font-weight: 600;
}

.doc-preview {
  font-size: 13px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  color: var(--n-text-color-3);
}

.doc-date {
  font-size: 12px;
}

.doc-error {
  font-size: 12px;
}

.kb-settings {
  max-width: 800px;
}

.settings-card {
  margin-bottom: 20px;
}

.radio-option {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.radio-label {
  font-weight: 500;
}

.radio-desc {
  font-size: 12px;
}

.kb-empty-content {
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>
