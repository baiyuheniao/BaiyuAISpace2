/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * BaiyuAISpace 本地模型管理模块
 * 负责管理通过 Ollama 部署的本地模型，包括模型列表、下载、删除等功能
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * 本地模型信息
 * 与后端 LocalModelInfo 结构对应
 */
export interface LocalModelInfo {
  name: string;
  model: string;
  modifiedAt: string;
  size: number;
  digest: string;
  details: ModelDetails | null;
}

/**
 * 模型详细信息
 */
export interface ModelDetails {
  parentModel: string | null;
  format: string | null;
  family: string | null;
  families: string[] | null;
  parameterSize: string | null;
  quantizationLevel: string | null;
}

/**
 * 模型下载源
 */
export interface ModelSource {
  id: string;
  name: string;
  baseUrl: string;
  description: string;
}

/**
 * 下载进度事件
 */
export interface DownloadProgress {
  modelName: string;
  status: string;
  digest: string;
  total: number | null;
  completed: number | null;
}

/**
 * 本地模型 Store
 * 使用 Pinia 管理本地模型状态和业务逻辑
 */
export const useLocalModelStore = defineStore(
  "localModel",
  () => {
    // ============ 响应式状态 ============

    /** Ollama 服务基础 URL */
    const ollamaBaseUrl = ref("http://localhost:11434");

    /** Ollama 服务是否在线 */
    const isOnline = ref(false);

    /** Ollama 版本号 */
    const ollamaVersion = ref("");

    /** 本地模型列表 */
    const localModels = ref<LocalModelInfo[]>([]);

    /** 可用的模型下载源列表 */
    const modelSources = ref<ModelSource[]>([]);

    /** 当前选中的下载源 ID */
    const selectedSourceId = ref("ollama");

    /** 是否正在加载模型列表 */
    const isLoadingModels = ref(false);

    /** 是否正在下载模型 */
    const isPulling = ref(false);

    /** 当前正在下载的模型名称 */
    const pullingModelName = ref("");

    /** 下载进度信息 */
    const downloadProgress = ref<DownloadProgress | null>(null);

    /** 下载进度监听器取消函数 */
    let unlistenDownload: UnlistenFn | null = null;

    // ============ 计算属性 ============

    /** 当前选中的下载源 */
    const selectedSource = computed(() => {
      return modelSources.value.find((s) => s.id === selectedSourceId.value) || null;
    });

    /** 下载进度百分比 */
    const downloadPercent = computed(() => {
      if (!downloadProgress.value || !downloadProgress.value.total) return null;
      const { completed, total } = downloadProgress.value;
      if (completed == null || total == null) return null;
      return Math.round((completed / total) * 100);
    });

    /** 模型列表下拉选项（用于聊天页面选择） */
    const localModelOptions = computed(() => {
      return localModels.value.map((m) => ({
        label: m.name,
        value: m.name,
      }));
    });

    // ============ 方法函数 ============

    /**
     * 检查 Ollama 服务状态
     */
    const checkStatus = async () => {
      try {
        isOnline.value = await invoke<boolean>("check_ollama_status", {
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
        if (isOnline.value) {
          await fetchVersion();
        }
      } catch (error) {
        console.error("Failed to check Ollama status:", error);
        isOnline.value = false;
      }
    };

    /**
     * 获取 Ollama 版本号
     */
    const fetchVersion = async () => {
      try {
        ollamaVersion.value = await invoke<string>("get_ollama_version", {
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
      } catch (error) {
        console.error("Failed to get Ollama version:", error);
        ollamaVersion.value = "";
      }
    };

    /**
     * 加载本地模型列表
     */
    const loadModels = async () => {
      if (!isOnline.value) {
        await checkStatus();
        if (!isOnline.value) return;
      }

      isLoadingModels.value = true;
      try {
        localModels.value = await invoke<LocalModelInfo[]>("list_local_models", {
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
      } catch (error) {
        console.error("Failed to load local models:", error);
        localModels.value = [];
      } finally {
        isLoadingModels.value = false;
      }
    };

    /**
     * 加载模型下载源列表
     */
    const loadModelSources = async () => {
      try {
        modelSources.value = await invoke<ModelSource[]>("get_model_sources_cmd");
      } catch (error) {
        console.error("Failed to load model sources:", error);
        modelSources.value = [];
      }
    };

    /**
     * 设置 Ollama 服务地址
     */
    const setOllamaBaseUrl = async (url: string) => {
      ollamaBaseUrl.value = url;
      await checkStatus();
      if (isOnline.value) {
        await loadModels();
      }
    };

    /**
     * 设置默认下载源
     */
    const setSelectedSourceId = (sourceId: string) => {
      selectedSourceId.value = sourceId;
    };

    /**
     * 拉取（下载）模型
     * @param modelName 模型名称
     * @param sourceId 下载源 ID（可选，使用当前选中的源）
     */
    const pullModel = async (modelName: string, sourceId?: string) => {
      if (isPulling.value) return;

      isPulling.value = true;
      pullingModelName.value = modelName;
      downloadProgress.value = null;

      try {
        await setupDownloadListener();

        await invoke("pull_local_model", {
          request: {
            modelName,
            sourceId: sourceId || selectedSourceId.value,
            insecure: false,
          },
          ollamaBaseUrl: ollamaBaseUrl.value,
        });

        // Download completed, refresh model list
        await loadModels();
      } catch (error) {
        console.error("Failed to pull model:", error);
        throw error;
      } finally {
        isPulling.value = false;
        pullingModelName.value = "";
        downloadProgress.value = null;
        if (unlistenDownload) {
          unlistenDownload();
          unlistenDownload = null;
        }
      }
    };

    /**
     * 删除本地模型
     */
    const deleteModel = async (modelName: string) => {
      try {
        await invoke("delete_local_model", {
          request: { modelName },
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
        // Refresh model list after deletion
        await loadModels();
      } catch (error) {
        console.error("Failed to delete model:", error);
        throw error;
      }
    };

    /**
     * 设置下载进度监听器
     */
    const setupDownloadListener = async () => {
      if (unlistenDownload) {
        unlistenDownload();
      }

      unlistenDownload = await listen<DownloadProgress>(
        "download-progress",
        (event) => {
          downloadProgress.value = event.payload;
        }
      );
    };

    /**
     * 格式化文件大小
     */
    const formatSize = (bytes: number): string => {
      if (bytes === 0) return "0 B";
      const units = ["B", "KB", "MB", "GB", "TB"];
      const i = Math.floor(Math.log(bytes) / Math.log(1024));
      const size = bytes / Math.pow(1024, i);
      return `${size.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
    };

    /**
     * 获取模型显示名称（去掉标签后缀）
     */
    const getModelDisplayName = (modelName: string): string => {
      const colonIndex = modelName.indexOf(":");
      return colonIndex > 0 ? modelName.substring(0, colonIndex) : modelName;
    };

    // ============ 返回公共接口 ============
    return {
      // 状态
      ollamaBaseUrl,
      isOnline,
      ollamaVersion,
      localModels,
      modelSources,
      selectedSourceId,
      isLoadingModels,
      isPulling,
      pullingModelName,
      downloadProgress,

      // 计算属性
      selectedSource,
      downloadPercent,
      localModelOptions,

      // 方法
      checkStatus,
      fetchVersion,
      loadModels,
      loadModelSources,
      setOllamaBaseUrl,
      setSelectedSourceId,
      pullModel,
      deleteModel,
      formatSize,
      getModelDisplayName,
    };
  },
  {
    persist: {
      key: "baiyu-aispace-local-model",
      paths: ["ollamaBaseUrl", "selectedSourceId"],
    },
  }
);
