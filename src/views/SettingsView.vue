<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SettingsView.vue - 设置视图组件
  
  功能说明:
  - 管理 LLM API 配置 (创建、编辑、删除、激活)
  - 管理 Embedding API 配置 (用于知识库向量化)
  - 外观设置 (深色/浅色主题切换)
  - 显示应用版本和关于信息

  主要组成部分:
  - LLM API 配置卡片
  - Embedding API 配置卡片
  - 外观设置卡片
  - 关于信息卡片
  - 新建/编辑弹窗表单
-->

<script setup lang="ts">
import { ref, computed, onBeforeUnmount, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { save } from "@tauri-apps/plugin-dialog";
import { open as openExternalUrl } from "@tauri-apps/plugin-shell";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { 
  NLayout, 
  NLayoutContent, 
  NCard, 
  NForm, 
  NFormItem, 
  NSelect,
  NInput,
  NInputNumber,
  NButton,
  NSpace,
  NSwitch,
  NList,
  NListItem,
  NThing,
  NTag,
  NPopconfirm,
  NModal,
  NIcon,
  NText,
  NEmpty,
  useMessage
} from "naive-ui";
import {
  useSettingsStore,
  PRESET_PROVIDERS,
  type ApiConfig,
  type EmbeddingApiConfig,
  type RerankerApiConfig
} from "@/stores/settings";
import {
  KeyOutline,
  InformationCircleOutline,
  DocumentTextOutline,
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline,
  SettingsOutline,
} from "@vicons/ionicons5";
import { isModifierOnly, acceleratorFromEvent } from "@/utils/hotkey";

// ============ 状态管理 ============

// 设置 Store - 管理 API 配置和主题
const settings = useSettingsStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

// ============ 日志导出 ============

const exportLogs = async () => {
  try {
    // 让用户选择保存位置
    const filePath = await save({
      defaultPath: `BaiyuAISpace2_logs_${new Date().toISOString().split('T')[0]}.log`,
      filters: [{ name: "Log Files", extensions: ["log"] }]
    });
    
    if (filePath) {
      // 调用后端复制日志文件
      const result = await invoke<string>("copy_log_file", { destPath: filePath });
      message.success("日志已导出到: " + result);
    }
  } catch (error) {
    message.error("导出日志失败: " + error);
  }
};

// ============ 检测最新版本 ============

interface ReleaseInfo {
  version: string;
  name: string;
  body: string;
  htmlUrl: string;
  publishedAt: string | null;
}

interface LatestReleasesResult {
  currentVersion: string;
  stable: ReleaseInfo | null;
  beta: ReleaseInfo | null;
}

/** 当前运行的应用版本号，启动时读取一次用于展示和比对 */
const currentAppVersion = ref("");

onMounted(async () => {
  try {
    currentAppVersion.value = await getVersion();
  } catch {
    // 读取失败不影响其他功能，留空即可
  }
});

const checkingUpdate = ref(false);
const showUpdateModal = ref(false);
const latestReleases = ref<LatestReleasesResult | null>(null);

/** 更新内容以对应 GitHub Release 页面的原文为准，用 marked 渲染成 HTML 再净化 */
const renderReleaseNotes = (body: string): string => {
  const html = marked.parse(body.trim() || "暂无更新说明。", { async: false, breaks: true }) as string;
  return DOMPurify.sanitize(html);
};

/**
 * 简化版 semver 比较：核心版本号（major.minor.patch）逐段比较；核心相同时，
 * 不带 -beta.N 后缀的正式版视为比任何 beta 都新，两边都有后缀则逐段比较
 * （数字段按数值比，否则按字典序）。返回值同 Array.sort 的比较器语义。
 * 用于判断 GitHub 上查到的版本是否比本地正在运行的版本更新——不能简单用
 * 字符串不等于判断，否则本地跑着尚未发布的开发版（比如 beta.5）时，
 * 会把已发布的旧版本（beta.4）误判成"有新版本"。
 */
const compareVersions = (a: string, b: string): number => {
  const parse = (v: string) => {
    const dashIndex = v.indexOf("-");
    const core = dashIndex === -1 ? v : v.slice(0, dashIndex);
    const pre = dashIndex === -1 ? null : v.slice(dashIndex + 1).split(".");
    return { core: core.split(".").map((n) => Number.parseInt(n, 10) || 0), pre };
  };
  const va = parse(a);
  const vb = parse(b);

  for (let i = 0; i < Math.max(va.core.length, vb.core.length); i++) {
    const diff = (va.core[i] ?? 0) - (vb.core[i] ?? 0);
    if (diff !== 0) return diff;
  }

  if (va.pre === null && vb.pre === null) return 0;
  if (va.pre === null) return 1;
  if (vb.pre === null) return -1;

  for (let i = 0; i < Math.max(va.pre.length, vb.pre.length); i++) {
    const sa = va.pre[i];
    const sb = vb.pre[i];
    if (sa === undefined) return -1;
    if (sb === undefined) return 1;
    const na = Number(sa);
    const nb = Number(sb);
    if (!Number.isNaN(na) && !Number.isNaN(nb)) {
      if (na !== nb) return na - nb;
    } else if (sa !== sb) {
      return sa < sb ? -1 : 1;
    }
  }
  return 0;
};

const isNewerVersion = (remoteVersion: string): boolean =>
  compareVersions(remoteVersion, currentAppVersion.value) > 0;

const checkLatestVersion = async () => {
  checkingUpdate.value = true;
  try {
    latestReleases.value = await invoke<LatestReleasesResult>("check_latest_releases");
    if (!latestReleases.value.stable && !latestReleases.value.beta) {
      message.warning("未能获取到任何已发布的版本信息");
      return;
    }
    showUpdateModal.value = true;
  } catch (error) {
    message.error("检测最新版本失败: " + error);
  } finally {
    checkingUpdate.value = false;
  }
};

const openReleasePage = (url: string) => {
  void openExternalUrl(url);
};

// ============ 弹窗状态 ============

/** LLM API 配置 - 新建弹窗显示状态 */
const showCreateModal = ref(false);

/** LLM API 配置 - 编辑弹窗显示状态 */
const showEditModal = ref(false);

/** LLM API 配置 - 当前编辑的配置对象 */
const editingConfig = ref<ApiConfig | null>(null);

/** Embedding API 配置 - 新建弹窗显示状态 */
const showEmbeddingCreateModal = ref(false);

/** Embedding API 配置 - 编辑弹窗显示状态 */
const showEmbeddingEditModal = ref(false);

/** Embedding API 配置 - 当前编辑的配置对象 */
const editingEmbeddingConfig = ref<EmbeddingApiConfig | null>(null);

/** Reranker API 配置 - 新建弹窗 */
const showRerankerCreateModal = ref(false);

/** Reranker API 配置 - 编辑弹窗 */
const showRerankerEditModal = ref(false);

/** Reranker API 配置 - 当前编辑的配置对象 */
const editingRerankerConfig = ref<RerankerApiConfig | null>(null);

// ============ 表单数据状态 ============

/**
 * LLM API 配置表单数据
 * 用于新建和编辑 LLM API 配置
 */
const formData = ref({
  name: "",                  // 配置名称
  provider: "openai",        // 默认使用 OpenAI
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,  // 默认 Base URL
  model: "",                 // 模型名称
  apiKey: "",                // API 密钥
  maxTokens: null as number | null,  // 最大输出 token 数（null = 后端默认值）
});

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

/** Reranker API 配置表单数据 */
const rerankerFormData = ref({
  name: "",
  provider: "custom",
  baseUrl: "https://api.cohere.com",
  model: "rerank-multilingual-v3.0",
  apiKey: "",
});

// ============ 表单方法 ============

/**
 * 重置 LLM API 表单数据
 * 恢复到初始状态
 */
const resetForm = () => {
  formData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
    maxTokens: null,
  };
};

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

const resetRerankerForm = () => {
  rerankerFormData.value = { name: "", provider: "custom", baseUrl: "https://api.cohere.com", model: "rerank-multilingual-v3.0", apiKey: "" };
};

// ============ 弹窗打开方法 ============

/**
 * 打开新建 LLM API 配置弹窗
 * 先重置表单，再显示弹窗
 */
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

/**
 * 打开编辑 LLM API 配置弹窗
 * 填充当前配置数据到表单
 * 
 * @param config - 要编辑的配置对象
 */
const openEditModal = (config: ApiConfig) => {
  editingConfig.value = config;
  formData.value = {
    name: config.name,
    provider: config.provider,
    baseUrl: config.baseUrl,
    model: config.model,
    apiKey: config.apiKey,
    maxTokens: config.maxTokens ?? null,
  };
  showEditModal.value = true;
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

// ============ 提供商切换处理 ============

/**
 * 处理 LLM 提供商切换
 * 自动填入对应提供商的默认 Base URL
 * 
 * @param provider - 提供商标识符
 */
const handleProviderChange = (provider: string) => {
  formData.value.provider = provider;
  formData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
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

// ============ CRUD 操作处理 ============

/**
 * 创建新的 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleCreate = async () => {
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }
  if (!formData.value.apiKey.trim()) {
    message.error("请输入 API Key");
    return;
  }

  // 调用 Store 方法创建配置
  settings.createApiConfig(
    formData.value.name,
    formData.value.provider,
    formData.value.model,
    formData.value.apiKey,
    formData.value.baseUrl,
    formData.value.maxTokens ?? undefined
  );

  // 提示成功并关闭弹窗
  message.success("API 配置已创建");
  showCreateModal.value = false;
  resetForm();
};

/**
 * 更新 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleUpdate = async () => {
  if (!editingConfig.value) return;
  
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }

  // 调用 Store 方法更新配置
  settings.updateApiConfig(editingConfig.value.id, {
    name: formData.value.name,
    provider: formData.value.provider,
    baseUrl: formData.value.baseUrl,
    model: formData.value.model,
    apiKey: formData.value.apiKey,
    maxTokens: formData.value.maxTokens ?? undefined,
  });

  // 提示成功并关闭弹窗
  message.success("API 配置已更新");
  showEditModal.value = false;
  editingConfig.value = null;
};

/**
 * 删除 LLM API 配置
 * 
 * @param configId - 要删除的配置 ID
 */
const handleDelete = (configId: string) => {
  settings.deleteApiConfig(configId);
  message.success("API 配置已删除");
};

/**
 * 设置当前使用的 LLM API 配置
 * 
 * @param configId - 要激活的配置 ID
 */
const handleSetActive = (configId: string) => {
  settings.setActiveConfig(configId);
  message.success("已设为当前使用配置");
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

// ============ 通用设置 ============

/**
 * 切换“关闭窗口时最小化到系统托盘”开关
 * 同时把设置同步给 Rust 后端（窗口关闭事件在后端拦截）
 */
const handleCloseToTrayChange = async (enabled: boolean) => {
  await settings.setCloseToTray(enabled);
  message.success(enabled ? "已开启：关闭窗口将最小化到托盘" : "已关闭：关闭窗口将直接退出程序");
};

// ============ 快捷键录制（托盘唤起 / 新建会话） ============

/** 当前正在录制哪个快捷键；null 表示未在录制 */
const recordingHotkeyTarget = ref<"show" | "newSession" | null>(null);

let hotkeyRecordListener: ((e: KeyboardEvent) => void) | null = null;

const stopRecordingHotkey = () => {
  if (hotkeyRecordListener) {
    window.removeEventListener("keydown", hotkeyRecordListener, true);
    hotkeyRecordListener = null;
  }
  recordingHotkeyTarget.value = null;
};

const startRecordingHotkey = (target: "show" | "newSession") => {
  if (recordingHotkeyTarget.value) return;
  recordingHotkeyTarget.value = target;

  hotkeyRecordListener = async (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      stopRecordingHotkey();
      return;
    }

    // 纯修饰键还没构成完整组合，继续等待主键
    if (isModifierOnly(e)) return;

    const accelerator = acceleratorFromEvent(e);
    if (!accelerator) {
      message.warning("请至少搭配一个修饰键（Ctrl / Alt / Shift），避免和普通按键冲突");
      return;
    }

    stopRecordingHotkey();

    if (target === "show") {
      try {
        await settings.setShowHotkey(accelerator);
        message.success(`唤起快捷键已设置为 ${accelerator}`);
      } catch (error) {
        message.error(`设置快捷键失败（可能已被其他程序占用）：${error}`);
      }
    } else {
      settings.setNewSessionHotkey(accelerator);
      message.success(`新建会话快捷键已设置为 ${accelerator}`);
    }
  };

  window.addEventListener("keydown", hotkeyRecordListener, true);
};

onBeforeUnmount(() => {
  stopRecordingHotkey();
});

// ============ 计算属性 ============

/**
 * 提供商下拉选项
 * 从 Store 获取预设的提供商列表
 */
const providerOptions = computed(() => settings.presetProviderOptions);

</script>

<template>
  <!-- 设置主布局容器 -->
  <n-layout class="settings-view">
    <!-- 设置内容区域 -->
    <n-layout-content
      :native-scrollbar="false"
      class="settings-content"
    >
      <div class="settings-container">
        <!-- 页面标题 -->
        <header class="page-header enter-up">
          <span class="eyebrow">Settings</span>
          <h1 class="page-title">
            设置
          </h1>
        </header>

        <!-- LLM API 配置卡片 -->
        <n-card
          class="settings-card"
          :bordered="false"
        >
          <!-- 卡片标题 -->
          <template #header>
            <div class="card-header">
              <n-icon
                :size="20"
                depth="3"
              >
                <KeyOutline />
              </n-icon>
              <span>对话模型 API 配置</span>
              <!-- 新建配置按钮 -->
              <n-button
                type="primary"
                size="small"
                @click="openCreateModal"
              >
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- 配置列表 -->
          <n-list
            v-if="settings.apiConfigs.length > 0"
            hoverable
            clickable
          >
            <!-- 遍历显示每个配置 -->
            <n-list-item 
              v-for="config in settings.apiConfigs" 
              :key="config.id"
              @click="handleSetActive(config.id)"
            >
              <n-thing>
                <!-- 配置名称 -->
                <template #header>
                  <n-space align="center">
                    <span>{{ config.name }}</span>
                    <!-- 当前使用标签 -->
                    <n-tag 
                      v-if="config.id === settings.activeConfigId" 
                      type="success" 
                      size="small"
                    >
                      当前使用
                    </n-tag>
                  </n-space>
                </template>
                
                <!-- 配置描述 -->
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
                
                <!-- 操作按钮 -->
                <template #header-extra>
                  <n-space>
                    <!-- 编辑按钮 -->
                    <n-button
                      quaternary
                      circle
                      size="small"
                      @click.stop="openEditModal(config)"
                    >
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <!-- 删除按钮 -->
                    <n-popconfirm 
                      positive-text="删除"
                      negative-text="取消"
                      @positive-click="handleDelete(config.id)"
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
                      确定删除配置 "{{ config.name }}"？
                    </n-popconfirm>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>

          <!-- 空状态 -->
          <n-empty
            v-else
            description="暂无 API 配置"
          />

          <!-- 卡片底部提示 -->
          <template
            v-if="settings.apiConfigs.length > 0"
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
              API Key 使用系统密钥链加密存储（Windows Credential / macOS Keychain / Linux Secret Service）
            </n-text>
          </template>
        </n-card>

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

        <!-- 通用设置卡片 -->
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
                <SettingsOutline />
              </n-icon>
              <span>通用设置</span>
            </div>
          </template>

          <div class="general-setting-item">
            <div class="general-setting-text">
              <span class="general-setting-label">关闭窗口时最小化到系统托盘</span>
              <n-text
                depth="3"
                style="font-size: 12px;"
              >
                开启后，点击窗口右上角的关闭按钮只会隐藏窗口，程序继续在系统托盘运行；需从托盘图标菜单选择“退出程序”才会真正结束。关闭后，点击关闭按钮将直接退出程序。
              </n-text>
            </div>
            <n-switch
              :value="settings.closeToTray"
              @update:value="handleCloseToTrayChange"
            />
          </div>

          <div class="general-setting-item">
            <div class="general-setting-text">
              <span class="general-setting-label">从托盘唤起窗口的快捷键</span>
              <n-text
                depth="3"
                style="font-size: 12px;"
              >
                在任意界面按下该组合键，即可把最小化到托盘的窗口唤回桌面。点击右侧按钮后按下新的组合键即可修改，按 Esc 取消录制。
              </n-text>
            </div>
            <n-space
              align="center"
              :size="12"
            >
              <n-tag size="medium">
                {{ settings.showHotkey }}
              </n-tag>
              <n-button
                size="small"
                :type="recordingHotkeyTarget === 'show' ? 'warning' : 'default'"
                @click="startRecordingHotkey('show')"
              >
                {{ recordingHotkeyTarget === 'show' ? '请按下组合键…' : '修改快捷键' }}
              </n-button>
            </n-space>
          </div>

          <div class="general-setting-item">
            <div class="general-setting-text">
              <span class="general-setting-label">新建会话的快捷键</span>
              <n-text
                depth="3"
                style="font-size: 12px;"
              >
                在应用窗口获得焦点时按下该组合键，即可快速开始一个新对话。点击右侧按钮后按下新的组合键即可修改，按 Esc 取消录制。
              </n-text>
            </div>
            <n-space
              align="center"
              :size="12"
            >
              <n-tag size="medium">
                {{ settings.newSessionHotkey }}
              </n-tag>
              <n-button
                size="small"
                :type="recordingHotkeyTarget === 'newSession' ? 'warning' : 'default'"
                @click="startRecordingHotkey('newSession')"
              >
                {{ recordingHotkeyTarget === 'newSession' ? '请按下组合键…' : '修改快捷键' }}
              </n-button>
            </n-space>
          </div>

          <div class="general-setting-item general-setting-item--stack">
            <div class="general-setting-text">
              <span class="general-setting-label">全局 System Prompt</span>
              <n-text
                depth="3"
                style="font-size: 12px;"
              >
                对之后发送的每条新消息生效，会自动附加到对话的系统消息中，用于统一设定模型的身份、语气或回答规范；留空则不附加。已发送的历史消息不受影响。
              </n-text>
            </div>
            <n-input
              v-model:value="settings.systemPrompt"
              type="textarea"
              placeholder="例如：你是一个简洁、专业的助手，优先给出可执行的建议，避免空泛的客套话。"
              :autosize="{ minRows: 3, maxRows: 8 }"
            />
          </div>
        </n-card>

        <!-- 关于卡片 -->
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
                <InformationCircleOutline />
              </n-icon>
              <span>关于</span>
            </div>
          </template>

          <!-- 关于内容 -->
          <div class="about-content">
            <div class="about-item">
              <span class="about-label">版本</span>
              <n-tag
                type="success"
                size="small"
              >
                v{{ currentAppVersion || "…" }}
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">许可证</span>
              <n-tag
                type="info"
                size="small"
              >
                MPL-2.0
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">GitHub</span>
              <n-text
                underline
                class="about-link"
              >
                baiyuheniao/BaiyuAISpace2
              </n-text>
            </div>
            
            <div
              class="about-item"
              style="margin-top: 16px;"
            >
              <n-space>
                <n-button
                  type="primary"
                  size="small"
                  :loading="checkingUpdate"
                  @click="checkLatestVersion"
                >
                  检测最新版本
                </n-button>
                <n-button
                  type="primary"
                  size="small"
                  @click="exportLogs"
                >
                  导出日志
                </n-button>
              </n-space>
            </div>
          </div>
        </n-card>

        <!-- 页脚 -->
        <div class="footer-text">
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            Made with ❤️ by Baiyu
          </n-text>
        </div>
      </div>
    </n-layout-content>

    <!-- 新建 LLM API 配置弹窗 -->
    <n-modal
      v-model:show="showCreateModal"
      title="新建 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              已自动填入 {{ PRESET_PROVIDERS[formData.provider]?.name }} 默认地址，可手动修改
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="模型"
          required
        >
          <n-input 
            v-model:value="formData.model" 
            placeholder="例如：gpt-4o, claude-3-5-sonnet, qwen-max..."
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              输入模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="API Key"
          required
        >
          <n-input
            v-model:value="formData.apiKey"
            type="password"
            show-password-on="click"
            placeholder="输入 API Key"
          />
        </n-form-item>

        <n-form-item label="Max Tokens">
          <n-input-number
            v-model:value="formData.maxTokens"
            :min="1"
            :max="1000000"
            placeholder="留空则不限制（Anthropic 除外）"
            style="width: 100%"
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              留空时大多数服务商不会传这个参数，不会截断长回答。Anthropic 接口强制要求该字段，留空则用 32000。
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showCreateModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleCreate"
          >
            创建
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 编辑 LLM API 配置弹窗 -->
    <n-modal
      v-model:show="showEditModal"
      title="编辑 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <n-form-item
          label="配置名称"
          required
        >
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
        </n-form-item>

        <n-form-item
          label="模型"
          required
        >
          <n-input 
            v-model:value="formData.model" 
            placeholder="例如：gpt-4o, claude-3-5-sonnet..."
          />
        </n-form-item>

        <n-form-item label="API Key">
          <n-input 
            v-model:value="formData.apiKey" 
            type="password"
            show-password-on="click"
            :placeholder="formData.provider === 'baidu' ? '请输入 access_token' : '留空表示不修改'"
          />
          <template #feedback>
            <n-text
              v-if="formData.provider === 'baidu'"
              depth="2"
              style="font-size: 12px; color: #444444;"
            >
              百度千帆需要 access_token，而非 API Key。请在
              <n-a
                href="https://console.bce.baidu.com/qianfan/"
                target="_blank"
              >
                百度千帆控制台
              </n-a>
              获取 API Key 和 Secret Key，然后
              <n-a
                href="https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Ck3edn42t"
                target="_blank"
              >
                换取 access_token
              </n-a>
            </n-text>
            <n-text
              v-else
              depth="3"
              style="font-size: 12px;"
            >
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>

        <n-form-item label="Max Tokens">
          <n-input-number
            v-model:value="formData.maxTokens"
            :min="1"
            :max="1000000"
            placeholder="留空则不限制（Anthropic 除外）"
            style="width: 100%"
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              留空时大多数服务商不会传这个参数，不会截断长回答。Anthropic 接口强制要求该字段，留空则用 32000。
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEditModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleUpdate"
          >
            保存
          </n-button>
        </n-space>
      </template>
    </n-modal>

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

    <!-- 检测最新版本弹窗 -->
    <n-modal
      v-model:show="showUpdateModal"
      title="版本检测结果"
      preset="card"
      style="width: 640px; max-width: 90vw;"
    >
      <div class="version-check-body">
        <div class="version-check-current">
          当前版本：<n-tag size="small">v{{ currentAppVersion }}</n-tag>
        </div>

        <template
          v-for="section in [
            { label: '最新正式版', info: latestReleases?.stable },
            { label: '最新 Beta 版', info: latestReleases?.beta },
          ]"
          :key="section.label"
        >
          <div class="version-check-section">
            <div class="version-check-section-header">
              <span class="version-check-section-label">{{ section.label }}</span>
              <template v-if="section.info">
                <n-tag size="small">v{{ section.info.version }}</n-tag>
                <n-tag
                  size="small"
                  :type="isNewerVersion(section.info.version) ? 'success' : 'default'"
                >
                  {{ isNewerVersion(section.info.version) ? "有新版本" : "已是最新" }}
                </n-tag>
                <n-button
                  text
                  size="small"
                  class="version-check-link"
                  @click="openReleasePage(section.info.htmlUrl)"
                >
                  在 GitHub 中查看
                </n-button>
              </template>
            </div>

            <n-empty
              v-if="!section.info"
              description="GitHub 上暂无对应版本发布"
              size="small"
            />
            <div
              v-else
              class="version-check-notes"
              v-html="renderReleaseNotes(section.info.body)"
            />
          </div>
        </template>
      </div>
    </n-modal>

  </n-layout>
</template>

<style scoped lang="scss">
/* 设置主容器 */
.settings-view {
  height: 100%;
  background: $bg;
}

/* 设置内容区域 */
.settings-content {
  height: 100%;
}

/* 内容容器 - 限制最大宽度并居中
   宽屏/超宽屏下分级放宽，避免两侧大片留白（但设置项是一行行的表单/列表，
   放太宽反而拉散标签和操作按钮的视觉关联，所以放宽幅度比列表页克制） */
.settings-container {
  max-width: 700px;
  margin: 0 auto;
  padding: 5rem 2rem 8rem;

  @media (min-width: $bp-wide) {
    max-width: 900px;
  }

  @media (min-width: $bp-ultrawide) {
    max-width: 1000px;
  }
}

/* 页面标题区域 */
.page-header {
  margin-bottom: 4rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.page-title {
  font-family: $font-serif;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

/* 设置卡片样式 */
.settings-card {
  margin-bottom: 20px;
  background: $bg;
  border: $border-soft;
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;

  &:hover {
    transform: translateY(-4px);
    box-shadow: $shadow-hover;
  }
}

/* 卡片标题样式 */
.card-header {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;

  .n-button {
    margin-left: auto;
  }
}

/* 通用设置项 */
.general-setting-item {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
}

.general-setting-item + .general-setting-item {
  margin-top: 20px;
  padding-top: 20px;
  border-top: 1px solid rgba(0, 0, 0, 0.12);
}

.general-setting-item--stack {
  flex-direction: column;
  align-items: stretch;
}

.general-setting-text {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.general-setting-label {
  font-weight: 600;
}

/* 检测最新版本弹窗 */
.version-check-body {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.version-check-current {
  font-size: 14px;
  color: $ink-soft;
}

.version-check-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 16px;
  border-top: $border-faint;
}

.version-check-section-header {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.version-check-section-label {
  font-weight: 600;
}

.version-check-link {
  margin-left: auto;
}

.version-check-notes {
  font-size: 13px;
  line-height: $leading-body;
  color: $ink-soft;
  max-height: 260px;
  overflow-y: auto;

  :deep(h1),
  :deep(h2),
  :deep(h3) {
    font-size: 14px;
    font-weight: 600;
    color: $ink;
    margin: 8px 0 4px;
  }

  :deep(ul),
  :deep(ol) {
    padding-left: 20px;
  }

  :deep(a) {
    color: $ink;
  }

  :deep(code) {
    font-family: $font-mono;
    background: $surface;
    padding: 1px 4px;
  }
}
</style>
