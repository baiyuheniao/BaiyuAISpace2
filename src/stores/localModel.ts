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
import { useSettingsStore } from "./settings";

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
 * Ollama 安装检测信息
 */
export interface OllamaInstallInfo {
  installed: boolean;
  installPath: string | null;
  version: string | null;
}

/**
 * Ollama 服务状态
 */
export interface OllamaServiceStatus {
  running: boolean;
  managedByApp: boolean;
}

/**
 * 模型搜索结果
 */
export interface ModelSearchResult {
  name: string;
  description: string;
  tags: string[];
  sizeInfo: string;
}

/**
 * Ollama 安装进度事件
 */
export interface OllamaInstallProgress {
  stage: "downloading" | "installing" | "completed" | "error";
  progressPercent: number;
  downloadedBytes: number;
  totalBytes: number | null;
  message: string;
}

/**
 * Ollama 下载镜像源
 */
export interface OllamaDownloadMirror {
  id: string;
  name: string;
  url: string;
  description: string;
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

    // ============ Ollama 安装 & 服务管理状态 ============

    /** Ollama 是否已安装 */
    const ollamaInstalled = ref(false);

    /** Ollama 安装路径 */
    const ollamaInstallPath = ref<string | null>(null);

    /** Ollama 安装版本（来自本地检测） */
    const ollamaInstalledVersion = ref<string | null>(null);

    /** Ollama 服务是否由本应用管理 */
    const serviceManagedByApp = ref(false);

    /** 是否正在下载/安装 Ollama */
    const isInstallingOllama = ref(false);

    /** Ollama 安装进度 */
    const ollamaInstallProgress = ref<OllamaInstallProgress | null>(null);

    /** 安装进度监听器取消函数 */
    let unlistenInstall: UnlistenFn | null = null;

    /** 可用的下载镜像列表 */
    const downloadMirrors = ref<OllamaDownloadMirror[]>([]);

    /** 当前选中的下载镜像 ID */
    const selectedMirrorId = ref(
      // Linux defaults to install script, others to github
      navigator.platform.toLowerCase().includes("linux") ? "install_script" : "github"
    );

    /** 模型搜索结果 */
    const modelSearchResults = ref<ModelSearchResult[]>([]);

    /** 是否正在搜索模型 */
    const isSearchingModels = ref(false);

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

    // ============ Ollama 安装 & 服务管理方法 ============

    /**
     * 检测 Ollama 是否已安装
     */
    const detectInstallation = async () => {
      try {
        const info = await invoke<OllamaInstallInfo>("detect_ollama_installation");
        ollamaInstalled.value = info.installed;
        ollamaInstallPath.value = info.installPath;
        ollamaInstalledVersion.value = info.version;
        return info;
      } catch (error) {
        console.error("Failed to detect Ollama installation:", error);
        ollamaInstalled.value = false;
        return { installed: false, installPath: null, version: null } as OllamaInstallInfo;
      }
    };

    /**
     * 启动 Ollama 服务
     * 如果已运行则直接返回，否则后台启动并等待就绪
     */
    const startService = async () => {
      try {
        const status = await invoke<OllamaServiceStatus>("start_ollama_service", {
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
        isOnline.value = status.running;
        serviceManagedByApp.value = status.managedByApp;
        if (status.running) {
          await fetchVersion();
          await loadModels();
        }
        return status;
      } catch (error) {
        console.error("Failed to start Ollama service:", error);
        throw error;
      }
    };

    /**
     * 停止由本应用管理的 Ollama 服务
     */
    const stopService = async () => {
      try {
        await invoke("stop_ollama_service");
        isOnline.value = false;
        serviceManagedByApp.value = false;
        ollamaVersion.value = "";
        localModels.value = [];
      } catch (error) {
        console.error("Failed to stop Ollama service:", error);
        throw error;
      }
    };

    /**
     * 获取 Ollama 服务状态
     */
    const getServiceStatus = async () => {
      try {
        const status = await invoke<OllamaServiceStatus>("get_ollama_service_status", {
          ollamaBaseUrl: ollamaBaseUrl.value,
        });
        isOnline.value = status.running;
        serviceManagedByApp.value = status.managedByApp;
        return status;
      } catch (error) {
        console.error("Failed to get Ollama service status:", error);
        isOnline.value = false;
        return { running: false, managedByApp: false } as OllamaServiceStatus;
      }
    };

    /**
     * 下载并安装 Ollama
     * @param mirrorId 下载镜像 ID（可选）
     */
    const downloadAndInstallOllama = async (mirrorId?: string) => {
      if (isInstallingOllama.value) return;

      isInstallingOllama.value = true;
      ollamaInstallProgress.value = null;

      try {
        // Setup progress listener
        if (unlistenInstall) {
          unlistenInstall();
        }
        unlistenInstall = await listen<OllamaInstallProgress>(
          "ollama-install-progress",
          (event) => {
            ollamaInstallProgress.value = event.payload;
          }
        );

        // Step 1: Download
        const installerPath = await invoke<string>("download_ollama", {
          mirrorId: mirrorId || selectedMirrorId.value,
        });

        // Step 2: Install
        await invoke("install_ollama", { installerPath });

        // Step 3: Re-detect installation
        await detectInstallation();

        // Step 4: Auto-start service
        if (ollamaInstalled.value) {
          await startService();
        }
      } catch (error) {
        console.error("Failed to download/install Ollama:", error);
        throw error;
      } finally {
        isInstallingOllama.value = false;
        if (unlistenInstall) {
          unlistenInstall();
          unlistenInstall = null;
        }
      }
    };

    /**
     * 加载下载镜像列表
     */
    const loadDownloadMirrors = async () => {
      try {
        downloadMirrors.value = await invoke<OllamaDownloadMirror[]>("get_ollama_download_mirrors_cmd");
      } catch (error) {
        console.error("Failed to load download mirrors:", error);
        downloadMirrors.value = [];
      }
    };

    /**
     * 搜索 Ollama 模型
     * @param query 搜索关键词
     */
    const searchModels = async (query: string) => {
      if (!query.trim()) {
        modelSearchResults.value = [];
        return;
      }

      isSearchingModels.value = true;
      try {
        modelSearchResults.value = await invoke<ModelSearchResult[]>("search_ollama_models", {
          query,
        });
      } catch (error) {
        console.error("Failed to search models:", error);
        modelSearchResults.value = [];
      } finally {
        isSearchingModels.value = false;
      }
    };

    /**
     * 一键本地部署：启动服务 → 下载模型 → 创建 API 配置
     * @param modelName 模型名称
     */
    const oneClickDeploy = async (modelName: string) => {
      // Ensure service is running
      if (!isOnline.value) {
        await startService();
        if (!isOnline.value) {
          throw new Error("无法启动 Ollama 服务");
        }
      }

      // Pull model
      await pullModel(modelName, "ollama");

      // Auto-create API config for this model if not exists
      const settings = useSettingsStore();
      const existingConfig = settings.apiConfigs.find(
        (c) => c.provider === "local" && c.model === modelName
      );

      if (!existingConfig) {
        const configName = `local-${modelName}`;
        settings.createApiConfig(
          configName,
          "local",
          modelName,
          "", // Local Ollama doesn't need API key
          ollamaBaseUrl.value.replace(/\/$/, "")
        );
      }

      return modelName;
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

      // Ollama 安装 & 服务管理状态
      ollamaInstalled,
      ollamaInstallPath,
      ollamaInstalledVersion,
      serviceManagedByApp,
      isInstallingOllama,
      ollamaInstallProgress,
      downloadMirrors,
      selectedMirrorId,
      modelSearchResults,
      isSearchingModels,

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

      // Ollama 安装 & 服务管理方法
      detectInstallation,
      startService,
      stopService,
      getServiceStatus,
      downloadAndInstallOllama,
      loadDownloadMirrors,
      searchModels,
      oneClickDeploy,
    };
  },
  {
    persist: {
      key: "baiyu-aispace-local-model",
      paths: ["ollamaBaseUrl", "selectedSourceId"],
    },
  }
);
