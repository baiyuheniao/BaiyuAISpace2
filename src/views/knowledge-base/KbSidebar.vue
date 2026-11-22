<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KbSidebar.vue - 侧边栏：知识库列表（选择/删除/新建入口）
  （从 KnowledgeBaseView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { NLayoutSider, NButton, NList, NListItem, NThing, NTag, NText, NEmpty, NSpin, NSpace, NPopconfirm, NIcon } from "naive-ui";
import { Add, TrashOutline } from "@vicons/ionicons5";
import { useMessage } from "@/composables/useNotify";
import { useKnowledgeBaseStore, type KnowledgeBase } from "@/stores/knowledgeBase";
import { getEmbeddingConfigName } from "./helpers";

const emit = defineEmits<{
  (e: "create"): void;
  (e: "select", kb: KnowledgeBase): void;
}>();

const message = useMessage();
const kbStore = useKnowledgeBaseStore();

const handleSelectKb = (kb: KnowledgeBase) => emit("select", kb);

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
</script>

<template>
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
        <div class="kb-title-block">
          <span class="eyebrow">RAG</span>
          <h2 class="kb-title">
            知识库
          </h2>
        </div>
        <!-- 新建按钮 -->
        <n-button
          type="primary"
          size="small"
          @click="emit('create')"
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
</template>

<style scoped lang="scss">
/* 侧边栏背景 */
.kb-sidebar {
  background: $bg;
  border-right: $border;
}

/* 侧边栏内容 */
.kb-sidebar-content {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 2rem 1.25rem;
}

/* 侧边栏头部 */
.kb-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  margin-bottom: 2rem;
  padding-bottom: 1rem;
  border-bottom: $border;
}

/* 标题区块 */
.kb-title-block {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

/* 标题样式 */
.kb-title {
  font-family: $font-serif;
  font-size: 1.4rem;
  font-weight: 700;
  line-height: $leading-display;
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

/* 知识库项元信息标签等长文本截断的 :deep 规则留在 KnowledgeBaseView（文档列表共用） */
</style>
