<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatInput.vue - 聊天输入组件
  
  功能说明:
  - 消息文本输入 (支持多行)
  - 附件文件上传
  - 知识库选择 (RAG)
  - API 配置切换
  - MCP 工具开关
  - 消息发送/停止生成
-->

<script setup lang="ts">
// 导入 Vue 相关功能
import { ref, computed, onMounted } from "vue";

// 导入 Tauri API
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";

// 导入 NaiveUI 组件
import {
  NButton,
  NIcon,
  NSpace,
  NText,
  NTooltip,
  NSelect,
  NBadge,
  NTag,
  NCheckbox,
  NCheckboxGroup,
  NSwitch,
  NDivider,
  useNotification,
} from "naive-ui";

// 导入 Store
import { useChatStore } from "@/stores/chat";
import { useSettingsStore, PRESET_PROVIDERS } from "@/stores/settings";
import { useKnowledgeBaseStore } from "@/stores/knowledgeBase";
import { useMCPStore } from "@/stores/mcp";
import { useSkillsStore } from "@/stores/skills";

// 导入图标
import {
  Send,
  Library,
  Close,
  ServerOutline,
  ChevronDown,
  Cube,
  ExtensionPuzzleOutline,
  BulbOutline,
} from "@vicons/ionicons5";

// ============ Store 实例 ============

const chat = useChatStore();
const settings = useSettingsStore();
const kbStore = useKnowledgeBaseStore();
const mcp = useMCPStore();
const skillsStore = useSkillsStore();

// 通知组件
const notification = useNotification();

// ============ 响应式状态 ============

// 输入框文本内容
const inputValue = ref("");

// 输入框 DOM 引用
const inputRef = ref<HTMLTextAreaElement | null>(null);

// 文件输入 DOM 引用
const fileInputRef = ref<HTMLInputElement | null>(null);

// 已附加的文件列表
const attachedFiles = ref<File[]>([]);

// 已附加的文档列表（供直接上下文注入，无需知识库）
const attachedDocuments = ref<Array<{ name: string; path: string }>>([]);

// 是否显示知识库选择器
const showRagSelector = ref(false);

// 是否显示 API 配置选择器
const showApiSelector = ref(false);

// 是否显示 Skill 选择器
const showSkillSelector = ref(false);

// ============ 计算属性 ============

// 是否可以发送消息
const canSend = computed(() => {
  const hasContent = inputValue.value.trim().length > 0;
  const hasFiles = attachedFiles.value.length > 0;
  const hasDocs = attachedDocuments.value.length > 0;
  return (hasContent || hasFiles || hasDocs) && !chat.isLoading && settings.activeConfig;
});

// API 配置下拉选项
const apiConfigOptions = computed(() => {
  return settings.apiConfigs.map(config => ({
    label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider})`,
    value: config.id,
  }));
});

// 当前使用的 API 配置
const currentApiConfig = computed(() => {
  return settings.activeConfig;
});

// 知识库下拉选项
const kbOptions = computed(() => {
  return [
    { label: "不使用知识库", value: "" },
    ...kbStore.knowledgeBases.map(kb => ({
      label: `${kb.name} (${kb.document_count} 文档)`,
      value: kb.id,
    }))
  ];
});

// 已选中的知识库名称
const selectedKbName = computed(() => {
  if (!chat.selectedKnowledgeBaseId) return null;
  const kb = kbStore.knowledgeBases.find(k => k.id === chat.selectedKnowledgeBaseId);
  return kb?.name;
});

// 已启用的 MCP 服务器数量
const enabledMcpServersCount = computed(() => {
  return mcp.servers.filter(s => s.enabled).length;
});

// 可用的 MCP 工具数量
const availableMcpToolsCount = computed(() => {
  return mcp.availableTools.length;
});

// 已启用的 Skill 选项 (用于多选列表)
const skillCheckboxOptions = computed(() =>
  skillsStore.enabledSkills.map((s) => ({ label: s.name, value: s.id, description: s.description }))
);

// 当前手动激活的 Skill 名称列表
const activeSkillNames = computed(() =>
  chat.activeSkillIds
    .map((id) => skillsStore.skills.find((s) => s.id === id)?.name)
    .filter((name): name is string => Boolean(name))
);

// 可用的知识库数量
const availableKbCount = computed(() => {
  return kbStore.knowledgeBases.length;
});

// ============ 生命周期钩子 ============

// 组件挂载时加载数据
onMounted(() => {
  kbStore.loadKnowledgeBases();
  mcp.loadServers();
  skillsStore.loadSkills();
});

// ============ 方法函数 ============

// 将 File 对象转换为 base64 ImageAttachment
const readFileAsBase64 = (file: File): Promise<{ data: string; mediaType: string }> => {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = (e) => {
      const dataUrl = e.target?.result as string;
      const commaIdx = dataUrl.indexOf(',');
      if (commaIdx === -1) { reject(new Error("Invalid data URL")); return; }
      const header = dataUrl.slice(0, commaIdx);
      const data = dataUrl.slice(commaIdx + 1);
      const mediaType = header.split(':')[1]?.split(';')[0] ?? file.type;
      resolve({ data, mediaType });
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
};

// 发送消息
const handleSend = async () => {
  const content = inputValue.value.trim();
  if ((!content && attachedFiles.value.length === 0) || chat.isLoading) return;

  if (!settings.activeConfig) {
    notification.error({
      title: "未配置 API",
      description: "请先前往设置创建 API 配置",
      duration: 3000,
    });
    return;
  }

  if (!chat.currentSession) {
    await chat.createSession(settings.activeConfig.id);
  }

  // 分类附件文件
  const imageFiles = attachedFiles.value.filter(f => f.type.startsWith('image/'));
  const videoFiles = attachedFiles.value.filter(f => f.type.startsWith('video/'));
  const otherFiles = attachedFiles.value.filter(f => !f.type.startsWith('image/') && !f.type.startsWith('video/'));

  // 读取图片/视频文件为 base64
  const images = imageFiles.length > 0
    ? await Promise.all(imageFiles.map(readFileAsBase64))
    : undefined;

  const videos = videoFiles.length > 0
    ? await Promise.all(videoFiles.map(readFileAsBase64))
    : undefined;

  // 文件元数据 (用于 UI 显示)
  const fileInfo = attachedFiles.value.map(f => ({ name: f.name, size: f.size }));

  // 其余文件以文本提及方式加入内容
  let messageContent = content;
  const mentions = otherFiles.map(f => `[文件: ${f.name} (${(f.size / 1024 / 1024).toFixed(2)}MB)]`);
  if (mentions.length > 0) {
    messageContent = messageContent ? `${messageContent}\n${mentions.join(' ')}` : mentions.join(' ');
  }

  // 加载附加文档内容（并行读取）
  const docsToLoad = [...attachedDocuments.value];
  const documentContents: Array<{ name: string; content: string }> = [];
  for (const doc of docsToLoad) {
    try {
      const text = await invoke<string>('read_document_for_context', { filePath: doc.path });
      documentContents.push({ name: doc.name, content: text });
    } catch (err) {
      console.error(`Failed to read document ${doc.name}:`, err);
    }
  }

  inputValue.value = "";
  attachedFiles.value = [];
  attachedDocuments.value = [];
  if (inputRef.value) {
    inputRef.value.style.height = "60px";
  }

  try {
    await chat.sendMessage(
      messageContent,
      fileInfo.length > 0 ? fileInfo : undefined,
      images,
      videos,
      documentContents.length > 0 ? documentContents : undefined,
    );
  } catch (error) {
    const errorInfo = chat.classifyError(error);
    notification.error({
      title: "发送失败",
      description: errorInfo.message,
      duration: 4000,
    });
  }
};

// 停止生成
const handleStop = () => {
  chat.stopStream();
};

// 键盘事件处理 (Enter 发送, Shift+Enter 换行)
const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSend();
  }
};

// 输入框内容变化处理 (自动调整高度)
const handleInput = () => {
  if (inputRef.value) {
    inputRef.value.style.height = "auto";
    inputRef.value.style.height = Math.min(inputRef.value.scrollHeight, 200) + "px";
  }
};

// 打开/关闭知识库选择器
// 由于路由用了 <keep-alive>，ChatView 只会在第一次访问时触发 onMounted，
// 之后 kbStore.knowledgeBases 就不会再自动刷新——如果用户是在知识库页面
// 导入完文档后切回 Chat，这里看到的文档数会是切页面之前的旧值。每次打开
// 选择器时主动刷新一次，确保看到的知识库列表/文档数是最新的
const handleToggleRagSelector = () => {
  showRagSelector.value = !showRagSelector.value;
  if (showRagSelector.value) {
    kbStore.loadKnowledgeBases();
  }
};

// 知识库选择变化处理
const handleKbChange = (value: string) => {
  if (value === "") {
    chat.selectKnowledgeBaseForRag(null);
    chat.toggleRag(false);
  } else {
    chat.selectKnowledgeBaseForRag(value);
    chat.toggleRag(true);
  }
};

// 禁用 RAG
const handleDisableRag = () => {
  chat.toggleRag(false);
  chat.selectKnowledgeBaseForRag(null);
};

// API 配置切换
const handleApiChange = (configId: string) => {
  settings.setActiveConfig(configId);
  showApiSelector.value = false;
};

// MCP 开关切换
const handleMcpToggle = () => {
  chat.mcpEnabled = !chat.mcpEnabled;
};

// 禁用 MCP
const handleDisableMcp = () => {
  chat.mcpEnabled = false;
};

// 选择文件按钮点击
const handleFileSelect = () => {
  fileInputRef.value?.click();
};

const handleFilesSelected = (event: Event) => {
  const target = event.target as HTMLInputElement;
  const files = target.files;
  
  if (!files) return;

  const supportedFormats = ['image/jpeg', 'image/png', 'image/gif', 'image/webp', 'video/mp4', 'video/webm', 'video/mpeg'];
  
  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    if (supportedFormats.includes(file.type)) {
      // Check if file already attached
      if (!attachedFiles.value.find(f => f.name === file.name && f.size === file.size)) {
        attachedFiles.value.push(file);
      }
    }
  }
  
  // Reset input
  target.value = '';
};

const removeAttachedFile = (index: number) => {
  attachedFiles.value.splice(index, 1);
};

const getFileDisplayName = (file: File): string => {
  const maxLength = 20;
  return file.name.length > maxLength
    ? file.name.substring(0, maxLength) + '...'
    : file.name;
};

// 打开文件对话框选择文档（直接上下文注入）
const handleDocumentAttach = async () => {
  const selected = await open({
    multiple: true,
    filters: [{
      name: 'Documents',
      extensions: ['pdf', 'docx', 'doc', 'xlsx', 'xls', 'csv', 'pptx', 'md', 'markdown', 'html', 'htm', 'txt', 'rs', 'js', 'ts', 'py', 'java', 'c', 'cpp', 'h', 'go'],
    }],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  for (const path of paths) {
    const name = path.split(/[\\/]/).pop() ?? path;
    if (!attachedDocuments.value.find(d => d.path === path)) {
      attachedDocuments.value.push({ name, path });
    }
  }
};

// 移除已附加文档
const removeAttachedDocument = (index: number) => {
  attachedDocuments.value.splice(index, 1);
};

// 获取文档显示名称（截断长名称）
const getDocDisplayName = (name: string): string => {
  const maxLength = 22;
  return name.length > maxLength ? name.substring(0, maxLength) + '...' : name;
};
</script>

<template>
  <div class="chat-input-wrapper">
    <!-- API Config Indicator -->
    <div
      v-if="currentApiConfig"
      class="api-indicator"
    >
      <n-tag 
        type="info" 
        size="small" 
        :bordered="false"
        class="api-tag"
        @click="showApiSelector = !showApiSelector"
      >
        <template #icon>
          <n-icon><ServerOutline /></n-icon>
        </template>
        {{ currentApiConfig.name }}
        <n-icon
          :size="12"
          class="chevron-icon"
        >
          <ChevronDown />
        </n-icon>
      </n-tag>
      <n-text
        depth="3"
        class="model-text"
      >
        {{ currentApiConfig.model }}
      </n-text>
    </div>
    <div
      v-else
      class="api-indicator"
    >
      <n-tag
        type="warning"
        size="small"
        :bordered="false"
      >
        <template #icon>
          <n-icon><ServerOutline /></n-icon>
        </template>
        未选择 API 配置
      </n-tag>
    </div>

    <!-- RAG Indicator -->
    <div
      v-if="chat.ragEnabled && selectedKbName"
      class="rag-indicator"
    >
      <n-tag
        type="success"
        size="small"
        closable
        @close="handleDisableRag"
      >
        <template #icon>
          <n-icon><Library /></n-icon>
        </template>
        知识库: {{ selectedKbName }}
      </n-tag>
      <n-text
        v-if="chat.lastRetrievalResult"
        depth="3"
        class="rag-result-info"
      >
        检索到 {{ chat.lastRetrievalResult.chunks.length }} 个片段
      </n-text>
    </div>

    <!-- MCP Indicator -->
    <div
      v-if="chat.mcpEnabled && enabledMcpServersCount > 0"
      class="mcp-indicator"
    >
      <n-tag
        type="warning"
        size="small"
        closable
        @close="handleDisableMcp"
      >
        <template #icon>
          <n-icon><Cube /></n-icon>
        </template>
        MCP: {{ enabledMcpServersCount }} 服务 / {{ availableMcpToolsCount }} 工具
      </n-tag>
    </div>

    <!-- Skill Indicator -->
    <div
      v-if="activeSkillNames.length > 0 || chat.skillAutonomyEnabled"
      class="skill-indicator"
    >
      <n-tag
        v-if="activeSkillNames.length > 0"
        type="info"
        size="small"
        closable
        @close="chat.activeSkillIds = []"
      >
        <template #icon>
          <n-icon><ExtensionPuzzleOutline /></n-icon>
        </template>
        Skill: {{ activeSkillNames.join('、') }}
      </n-tag>
      <n-tag
        v-if="chat.skillAutonomyEnabled"
        type="default"
        size="small"
      >
        模型可自主调用 Skill
      </n-tag>
    </div>

    <div class="input-container">
      <div class="input-box">
        <textarea
          ref="inputRef"
          v-model="inputValue"
          class="chat-input"
          :placeholder="!currentApiConfig 
            ? '请先前往设置创建 API 配置...'
            : chat.ragEnabled 
              ? '输入问题，将基于知识库回答...' 
              : '输入消息，按 Enter 发送...'"
          rows="1"
          :disabled="chat.isLoading || !currentApiConfig"
          @keydown="handleKeydown"
          @input="handleInput"
        />
      </div>

      <div class="input-actions">
        <!-- File Upload -->
        <input
          ref="fileInputRef"
          type="file"
          multiple
          accept="image/*,video/mp4,video/webm,video/mpeg"
          style="display: none"
          @change="handleFilesSelected"
        >
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              tertiary
              circle
              size="large"
              class="file-btn"
              @click="handleFileSelect"
            >
              <template #icon>
                <n-icon>
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                  >
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline points="17 8 12 3 7 8" />
                    <line
                      x1="12"
                      y1="3"
                      x2="12"
                      y2="15"
                    />
                  </svg>
                </n-icon>
              </template>
            </n-button>
          </template>
          添加图片/视频 ({{ attachedFiles.length }})
        </n-tooltip>

        <!-- Document Attach -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              tertiary
              circle
              size="large"
              :type="attachedDocuments.length > 0 ? 'info' : 'default'"
              class="doc-btn"
              @click="handleDocumentAttach"
            >
              <template #icon>
                <n-icon>
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                  >
                    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                    <polyline points="14 2 14 8 20 8" />
                    <line x1="12" y1="18" x2="12" y2="12" />
                    <line x1="9" y1="15" x2="15" y2="15" />
                  </svg>
                </n-icon>
              </template>
            </n-button>
          </template>
          附加文档（注入上下文）{{ attachedDocuments.length > 0 ? ` (${attachedDocuments.length})` : '' }}
        </n-tooltip>

        <!-- MCP Toggle -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="large"
              :type="chat.mcpEnabled && enabledMcpServersCount > 0 ? 'warning' : 'default'"
              class="mcp-btn"
              :disabled="enabledMcpServersCount === 0"
              @click="handleMcpToggle"
            >
              <template #icon>
                <n-badge
                  v-if="chat.mcpEnabled && enabledMcpServersCount > 0"
                  :value="availableMcpToolsCount"
                  color="#000000"
                >
                  <n-icon><Cube /></n-icon>
                </n-badge>
                <n-icon v-else>
                  <Cube />
                </n-icon>
              </template>
            </n-button>
          </template>
          {{ chat.mcpEnabled ? '禁用 MCP' : '启用 MCP' }}{{ enabledMcpServersCount > 0 ? ` (${enabledMcpServersCount} 服务)` : '(无可用服务)' }}
        </n-tooltip>

        <!-- RAG Selector -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="large"
              :type="chat.ragEnabled ? 'success' : 'default'"
              :disabled="availableKbCount === 0"
              class="rag-btn"
              @click="handleToggleRagSelector"
            >
              <template #icon>
                <n-badge
                  v-if="chat.ragEnabled"
                  dot
                  type="success"
                >
                  <n-icon><Library /></n-icon>
                </n-badge>
                <n-icon v-else>
                  <Library />
                </n-icon>
              </template>
            </n-button>
          </template>
          {{ availableKbCount === 0 ? '无可用知识库' : chat.ragEnabled ? '更改知识库' : '启用知识库' }}
        </n-tooltip>

        <!-- Skill Selector -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="large"
              :type="activeSkillNames.length > 0 || chat.skillAutonomyEnabled ? 'info' : 'default'"
              :disabled="skillsStore.enabledSkills.length === 0"
              class="skill-btn"
              @click="showSkillSelector = !showSkillSelector"
            >
              <template #icon>
                <n-badge
                  v-if="activeSkillNames.length > 0"
                  :value="activeSkillNames.length"
                  color="#000000"
                >
                  <n-icon><ExtensionPuzzleOutline /></n-icon>
                </n-badge>
                <n-icon v-else>
                  <ExtensionPuzzleOutline />
                </n-icon>
              </template>
            </n-button>
          </template>
          {{ skillsStore.enabledSkills.length === 0 ? '无可用 Skill' : 'Skill' }}
        </n-tooltip>

        <!-- Thinking Mode Toggle -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="large"
              :type="chat.thinkingEnabled ? 'warning' : 'default'"
              class="thinking-btn"
              @click="chat.thinkingEnabled = !chat.thinkingEnabled"
            >
              <template #icon>
                <n-icon><BulbOutline /></n-icon>
              </template>
            </n-button>
          </template>
          {{ chat.thinkingEnabled ? '关闭思考模式' : '开启思考模式' }}
        </n-tooltip>

        <!-- Send/Stop Button -->
        <n-tooltip placement="top">
          <template #trigger>
            <n-button
              type="primary"
              circle
              size="large"
              :disabled="!canSend && !chat.isLoading"
              class="send-btn"
              @click="chat.isLoading ? handleStop() : handleSend()"
            >
              <template #icon>
                <n-icon>
                  <Send v-if="!chat.isLoading" />
                  <svg
                    v-else
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <rect
                      x="6"
                      y="6"
                      width="12"
                      height="12"
                      rx="2"
                    />
                  </svg>
                </n-icon>
              </template>
            </n-button>
          </template>
          {{ chat.isLoading ? '停止生成' : '发送消息' }}
        </n-tooltip>
      </div>
    </div>

    <!-- Attached Documents Display -->
    <div
      v-if="attachedDocuments.length > 0"
      class="attached-files"
    >
      <div class="files-label">
        已附加文档（直接注入上下文）：
      </div>
      <div class="files-list">
        <div
          v-for="(doc, index) in attachedDocuments"
          :key="index"
          class="file-item"
        >
          <n-tag
            closable
            type="info"
            class="file-tag"
            @close="removeAttachedDocument(index)"
          >
            <template #icon>
              <n-icon :size="14">
                <svg viewBox="0 0 24 24" fill="currentColor">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6zm-1 1.5L18.5 9H13V3.5zM12 18v-5h1v4h1v-3h1v3h1v-5h-5v5h1zm-3-5h1v5H9v-5zm-3 0h1v2H6v1h1v2H6v-5z"/>
                </svg>
              </n-icon>
            </template>
            {{ getDocDisplayName(doc.name) }}
          </n-tag>
        </div>
      </div>
    </div>

    <!-- Attached Files Display -->
    <div
      v-if="attachedFiles.length > 0"
      class="attached-files"
    >
      <div class="files-label">
        已附加的文件：
      </div>
      <div class="files-list">
        <div
          v-for="(file, index) in attachedFiles"
          :key="index"
          class="file-item"
        >
          <n-tag 
            closable 
            class="file-tag"
            @close="removeAttachedFile(index)"
          >
            <template #icon>
              <n-icon :size="14">
                <svg
                  v-if="file.type.startsWith('image/')"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                >
                  <path d="M21 19V5c0-1.1-.9-2-2-2H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2zM8.5 13.5l2.5 3.01L14.5 12l4.5 6H5l3.5-4.5z" />
                </svg>
                <svg
                  v-else-if="file.type.startsWith('video/')"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                >
                  <path d="M18 3H6c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-5 10l-4-3v6l4-3z" />
                </svg>
              </n-icon>
            </template>
            {{ getFileDisplayName(file) }}
          </n-tag>
        </div>
      </div>
    </div>

    <!-- API Selector Popover -->
    <div
      v-if="showApiSelector"
      class="selector-popover api-selector"
    >
      <div class="selector-header">
        <n-text strong>
          选择 API 配置
        </n-text>
        <n-button
          quaternary
          circle
          size="small"
          @click="showApiSelector = false"
        >
          <template #icon>
            <n-icon><Close /></n-icon>
          </template>
        </n-button>
      </div>
      <n-select
        :value="settings.activeConfigId || ''"
        :options="apiConfigOptions"
        placeholder="选择要使用的 API 配置"
        @update:value="handleApiChange"
      />
      <n-text
        v-if="apiConfigOptions.length === 0"
        depth="3"
        class="selector-hint"
      >
        暂无 API 配置，请前往设置创建
      </n-text>
    </div>

    <!-- RAG Selector Popover -->
    <div
      v-if="showRagSelector"
      class="selector-popover rag-selector"
    >
      <div class="selector-header">
        <n-text strong>
          选择知识库
        </n-text>
        <n-button
          quaternary
          circle
          size="small"
          @click="showRagSelector = false"
        >
          <template #icon>
            <n-icon><Close /></n-icon>
          </template>
        </n-button>
      </div>
      <n-select
        :value="chat.selectedKnowledgeBaseId || ''"
        :options="kbOptions"
        placeholder="选择要使用的知识库"
        @update:value="handleKbChange"
      />
      <n-text
        depth="3"
        class="selector-hint"
      >
        选择知识库后，AI 将基于文档内容回答问题
      </n-text>
    </div>

    <!-- Skill Selector Popover -->
    <div
      v-if="showSkillSelector"
      class="selector-popover skill-selector"
    >
      <div class="selector-header">
        <n-text strong>
          选择 Skill
        </n-text>
        <n-button
          quaternary
          circle
          size="small"
          @click="showSkillSelector = false"
        >
          <template #icon>
            <n-icon><Close /></n-icon>
          </template>
        </n-button>
      </div>

      <n-checkbox-group v-model:value="chat.activeSkillIds">
        <n-space vertical :size="8">
          <n-checkbox
            v-for="option in skillCheckboxOptions"
            :key="option.value"
            :value="option.value"
            :label="option.label"
          />
        </n-space>
      </n-checkbox-group>
      <n-text
        v-if="skillCheckboxOptions.length === 0"
        depth="3"
        class="selector-hint"
      >
        暂无已启用的 Skill，请前往 Skill 页面创建
      </n-text>

      <n-divider style="margin: 12px 0;" />

      <n-space align="center" justify="space-between">
        <n-text depth="3" style="font-size: 13px;">
          允许模型自主判断调用其它已启用的 Skill
        </n-text>
        <n-switch v-model:value="chat.skillAutonomyEnabled" size="small" />
      </n-space>
    </div>

    <div class="input-footer">
      <n-space
        align="center"
        :size="16"
      >
        <n-text
          depth="3"
          class="hint-text"
        >
          <span style="margin-right: 4px;">⌨️</span>
          Enter 发送 · Shift+Enter 换行
        </n-text>
        <template v-if="chat.ragEnabled">
          <n-text
            depth="3"
            class="divider"
          >
            |
          </n-text>
          <n-space
            align="center"
            :size="4"
          >
            <n-icon
              :size="14"
              color="#000000"
            >
              <Library />
            </n-icon>
            <n-text
              depth="3"
              class="rag-text"
            >
              RAG 已启用
            </n-text>
          </n-space>
        </template>
      </n-space>
    </div>
  </div>
</template>

<style scoped lang="scss">
.chat-input-wrapper {
  padding: 16px 32px 24px;
  max-width: 900px;
  margin: 0 auto;
  position: relative;
}

.api-indicator {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
  padding: 0 4px;
}

.api-tag {
  cursor: pointer;
  transition: all 0.2s;

  &:hover {
    opacity: 0.8;
  }
}

.chevron-icon {
  margin-left: 4px;
  transition: transform 0.2s;
}

.model-text {
  font-size: 12px;
}

.rag-indicator {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
  padding: 0 4px;
}

.rag-result-info {
  font-size: 12px;
}

.mcp-indicator {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.mcp-btn {
  transition: all 0.2s;
}

.mcp-btn:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.05);
}

.skill-indicator {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.skill-btn {
  transition: all 0.2s;
}

.skill-btn:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.05);
}

.input-container {
  display: flex;
  gap: 12px;
  align-items: flex-end;
  background: $bg;
  padding: 12px 16px;
  border: $border;
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;
}

.input-container:focus-within {
  transform: translateY(-4px);
  box-shadow: $shadow-hover;
}

.input-box {
  flex: 1;
  min-height: 44px;
  max-height: 200px;
}

.chat-input {
  width: 100%;
  min-height: 44px;
  max-height: 200px;
  padding: 10px 12px;
  border: none;
  background: transparent;
  color: $ink;
  font-size: 15px;
  line-height: $leading-body;
  resize: none;
  font-family: inherit;
  outline: none;
}

.chat-input::placeholder {
  color: $ink-faint;
}

.chat-input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.input-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 2px;
}

.rag-btn {
  transition: all 0.2s;
}

.rag-btn:hover {
  background: rgba(0, 0, 0, 0.05);
}

.send-btn {
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;
}

.send-btn:not(:disabled):hover {
  transform: translateY(-4px);
  box-shadow: $shadow-hover;
}

.send-btn:disabled {
  opacity: 0.4;
}

.selector-popover {
  position: absolute;
  bottom: 100%;
  left: 32px;
  right: 32px;
  margin-bottom: 8px;
  background: $bg;
  border: $border;
  padding: 16px;
  box-shadow: $shadow-hover;
  z-index: 100;
  animation: message-enter $duration $ease both;
}

@keyframes message-enter {
  from {
    opacity: 0;
    transform: translateY(40px) scale(0.95);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.api-selector {
  z-index: 101;
}

.rag-selector {
  z-index: 100;
}

.skill-selector {
  z-index: 100;
}

.selector-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.selector-hint {
  display: block;
  margin-top: 8px;
  font-size: 12px;
}

.input-footer {
  display: flex;
  justify-content: center;
  margin-top: 12px;
}

.hint-text {
  font-size: 12px;
  display: flex;
  align-items: center;
}

.divider {
  font-size: 12px;
  opacity: 0.5;
}

.rag-text {
  font-size: 12px;
  color: $ink;
}

.attached-files {
  margin-top: 12px;
  padding: 8px 4px;
}

.files-label {
  font-size: 12px;
  color: $ink-faint;
  margin-bottom: 6px;
}

.files-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.file-item {
  display: inline-block;
}

.file-tag {
  font-size: 12px;
  max-width: 200px;
}

.file-btn {
  transition: all 0.2s;
}

.file-btn:hover {
  background: rgba(0, 0, 0, 0.05);
}

</style>
