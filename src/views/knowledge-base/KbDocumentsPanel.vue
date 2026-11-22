<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  KbDocumentsPanel.vue - 文档标签页：文档列表、导入入口、删除
  （从 KnowledgeBaseView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { NButton, NList, NListItem, NThing, NTag, NText, NEmpty, NSpace, NPopconfirm, NIcon } from "naive-ui";
import { TrashOutline, CloudUploadOutline } from "@vicons/ionicons5";
import { useMessage } from "@/composables/useNotify";
import { useKnowledgeBaseStore, type Document } from "@/stores/knowledgeBase";

// importing 状态留在外壳（切换标签页会卸载本组件，导入进行中的加载态不能丢）
defineProps<{ importing: boolean }>();
const emit = defineEmits<{ (e: "import"): void }>();

const message = useMessage();
const kbStore = useKnowledgeBaseStore();

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
  <div class="kb-documents">
    <!-- 文档列表头部 -->
    <div class="kb-documents-header">
      <n-text depth="3">
        共 {{ kbStore.currentKbDocuments.length }} 个文档
      </n-text>
      <!-- 导入按钮 -->
      <n-button
        type="primary"
        :loading="importing"
        @click="emit('import')"
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
        <n-button @click="emit('import')">
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
</template>

<style scoped lang="scss">
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
</style>
