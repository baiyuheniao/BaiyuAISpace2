<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KnowledgeBaseView.vue - 知识库管理视图

  页面外壳：左侧知识库列表 + 右侧详情（文档 / 检索设置）+ 新建弹窗。具体部分见 ./knowledge-base/：
  - KbSidebar：知识库列表（选择、删除、新建入口）
  - KbDocumentsPanel：文档标签页
  - KbRetrievalSettings：检索设置标签页
  - KbCreateModal：新建知识库弹窗

  布局说明:
  - 桌面端: 左侧边栏 + 右侧内容区
  - 移动端: 无边栏，点击后全屏显示内容
-->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NLayout, NLayoutContent, NButton, NEmpty, NSpace, NIcon, NBreadcrumb, NBreadcrumbItem } from "naive-ui";
import { DocumentTextOutline, SettingsOutline, ArrowBack, Library } from "@vicons/ionicons5";
import { useMessage } from "@/composables/useNotify";
import { useKnowledgeBaseStore, type KnowledgeBase } from "@/stores/knowledgeBase";
import { useSettingsStore } from "@/stores/settings";
import KbSidebar from "./knowledge-base/KbSidebar.vue";
import KbDocumentsPanel from "./knowledge-base/KbDocumentsPanel.vue";
import KbRetrievalSettings from "./knowledge-base/KbRetrievalSettings.vue";
import KbCreateModal from "./knowledge-base/KbCreateModal.vue";

const message = useMessage();
const kbStore = useKnowledgeBaseStore();
const settingsStore = useSettingsStore();

const createModal = ref<InstanceType<typeof KbCreateModal> | null>(null);

/** 导入中状态 - 显示导入加载动画 */
const importing = ref(false);

/** 当前激活的标签页: "documents" | "settings" */
const activeTab = ref("documents");

onMounted(() => {
  kbStore.loadKnowledgeBases();
});

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
</script>

<template>
  <!-- 知识库主布局 (包含侧边栏) -->
  <n-layout
    class="kb-view"
    has-sider
  >
    <!-- 侧边栏: 知识库列表 -->
    <KbSidebar
      v-if="!kbStore.currentKb"
      @create="createModal?.open()"
      @select="handleSelectKb"
    />

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
      <KbDocumentsPanel
        v-if="activeTab === 'documents'"
        :importing="importing"
        @import="handleImport"
      />

      <!-- 检索设置标签页 -->
      <KbRetrievalSettings v-else-if="activeTab === 'settings'" />
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
  <KbCreateModal ref="createModal" />
</template>

<style scoped lang="scss">
/* 主容器 */
.kb-view {
  height: 100%;
  background: $bg;
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

/* 详情页头部：面包屑导航 + 标签页切换按钮，两行之间留出间距
   宽屏下文档列表/表单不设上限会被拉成一整行贴到屏幕两端，
   所以头部与内容区统一限宽居中，避免宽屏下比例失衡 */
.kb-detail-header {
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-bottom: 24px;
  max-width: 1000px;
  margin-left: auto;
  margin-right: auto;

  @media (min-width: $bp-wide) {
    max-width: 1200px;
  }

  @media (min-width: $bp-ultrawide) {
    max-width: 1400px;
  }
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
</style>
