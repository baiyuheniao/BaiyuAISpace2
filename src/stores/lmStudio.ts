/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * LM Studio 本地模型管理模块
 *
 * 负责连接 LM Studio 本地服务器并管理其模型 (列表/下载/加载/卸载)。
 * 与 Ollama 不同，LM Studio 是闭源 GUI 桌面应用，没有公开的无人值守
 * 安装方式，因此本模块只连接一个已经在运行的 LM Studio 服务，不负责
 * 下载/安装 LM Studio 本身 -- 用户需要自行从官网下载安装并在其设置里
 * 启动本地服务器 (Developer 标签页 -> Start Server)。
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";

/**
 * LM Studio 模型信息
 * 与后端 LMStudioModelInfo 结构对应
 */
export interface LMStudioModelInfo {
  id: string;
  modelType: string;
  publisher: string | null;
  arch: string | null;
  compatibilityType: string | null;
  quantization: string | null;
  /** "loaded" | "not-loaded" */
  state: string;
  maxContextLength: number | null;
}

/**
 * 模型下载进度事件
 */
export interface LMStudioDownloadProgress {
  modelId: string;
  /** "downloading" | "paused" | "completed" | "failed" */
  status: string;
  downloadedBytes: number | null;
  totalSizeBytes: number | null;
}

export const useLMStudioStore = defineStore(
  "lmStudio",
  () => {
    // ============ 响应式状态 ============

    /** LM Studio 服务器地址 (不含 /v1，由各命令自行拼接路径) */
    const baseUrl = ref("http://localhost:1234");

    /** 可选的 API Key (仅当用户在 LM Studio 中开启了 "Require API key" 时需要) */
    const apiKey = ref("");

    /** LM Studio 服务是否在线 */
    const isOnline = ref(false);

    /** 模型列表 (已下载 / 已加载) */
    const models = ref<LMStudioModelInfo[]>([]);

    /** 是否正在加载模型列表 */
    const isLoadingModels = ref(false);

    /** 是否正在下载模型 */
    const isPulling = ref(false);

    /** 当前正在下载的模型 ID */
    const pullingModelId = ref("");

    /** 下载进度信息 */
    const downloadProgress = ref<LMStudioDownloadProgress | null>(null);

    /** 下载进度监听器取消函数 */
    let unlistenDownload: UnlistenFn | null = null;

    // ============ 计算属性 ============

    /** 下载进度百分比 */
    const downloadPercent = computed(() => {
      if (!downloadProgress.value) return null;
      const { downloadedBytes, totalSizeBytes } = downloadProgress.value;
      if (downloadedBytes == null || totalSizeBytes == null || totalSizeBytes === 0) {
        return null;
      }
      return Math.round((downloadedBytes / totalSizeBytes) * 100);
    });

    /** 当前已加载到内存中的模型 */
    const loadedModels = computed(() => models.value.filter((m) => m.state === "loaded"));

    // ============ 方法函数 ============

    /**
     * 检查 LM Studio 服务状态
     */
    const checkStatus = async () => {
      try {
        isOnline.value = await invoke<boolean>("check_lmstudio_status", {
          baseUrl: baseUrl.value,
        });
      } catch (error) {
        console.error("Failed to check LM Studio status:", error);
        isOnline.value = false;
      }
    };

    /**
     * 加载模型列表
     */
    const loadModels = async () => {
      if (!isOnline.value) {
        await checkStatus();
        if (!isOnline.value) return;
      }

      isLoadingModels.value = true;
      try {
        models.value = await invoke<LMStudioModelInfo[]>("list_lmstudio_models", {
          baseUrl: baseUrl.value,
          apiKey: apiKey.value || undefined,
        });
      } catch (error) {
        console.error("Failed to load LM Studio models:", error);
        models.value = [];
      } finally {
        isLoadingModels.value = false;
      }
    };

    /**
     * 设置 LM Studio 服务地址
     */
    const setBaseUrl = async (url: string) => {
      baseUrl.value = url;
      await checkStatus();
      if (isOnline.value) {
        await loadModels();
      }
    };

    /**
     * 设置下载进度监听器
     */
    const setupDownloadListener = async () => {
      if (unlistenDownload) {
        unlistenDownload();
      }

      unlistenDownload = await listen<LMStudioDownloadProgress>(
        "lmstudio-download-progress",
        (event) => {
          downloadProgress.value = event.payload;
        }
      );
    };

    /**
     * 下载模型
     * @param modelId 模型标识 (LM Studio 模型目录名或 HuggingFace 引用)
     */
    const pullModel = async (modelId: string) => {
      if (isPulling.value) return;

      isPulling.value = true;
      pullingModelId.value = modelId;
      downloadProgress.value = null;

      try {
        await setupDownloadListener();

        await invoke("pull_lmstudio_model", {
          modelId,
          baseUrl: baseUrl.value,
          apiKey: apiKey.value || undefined,
        });

        await loadModels();
      } catch (error) {
        console.error("Failed to pull LM Studio model:", error);
        throw error;
      } finally {
        isPulling.value = false;
        pullingModelId.value = "";
        downloadProgress.value = null;
        if (unlistenDownload) {
          unlistenDownload();
          unlistenDownload = null;
        }
      }
    };

    /**
     * 加载模型到内存
     */
    const loadModel = async (modelId: string) => {
      try {
        await invoke("load_lmstudio_model", {
          modelId,
          baseUrl: baseUrl.value,
          apiKey: apiKey.value || undefined,
        });
        await loadModels();
      } catch (error) {
        console.error("Failed to load LM Studio model:", error);
        throw error;
      }
    };

    /**
     * 从内存卸载模型
     */
    const unloadModel = async (modelId: string) => {
      try {
        await invoke("unload_lmstudio_model", {
          modelId,
          baseUrl: baseUrl.value,
          apiKey: apiKey.value || undefined,
        });
        await loadModels();
      } catch (error) {
        console.error("Failed to unload LM Studio model:", error);
        throw error;
      }
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
     * 一键部署：确保模型已加载 -> 创建聊天 API 配置
     * 复用 "local" provider，因为 LM Studio 同样暴露了无需鉴权的
     * OpenAI 兼容 /v1/chat/completions 接口，和 Ollama 走的是同一套
     * 聊天请求代码路径
     */
    const oneClickDeploy = async (modelId: string) => {
      if (!isOnline.value) {
        throw new Error("LM Studio 服务未连接");
      }

      const model = models.value.find((m) => m.id === modelId);
      if (model && model.state !== "loaded") {
        await loadModel(modelId);
      }

      const chatBaseUrl = `${baseUrl.value.replace(/\/$/, "")}/v1`;

      const settings = useSettingsStore();
      const existingConfig = settings.apiConfigs.find(
        (c) => c.provider === "local" && c.model === modelId && c.baseUrl === chatBaseUrl
      );

      if (!existingConfig) {
        const configName = `lmstudio-${modelId}`;
        settings.createApiConfig(
          configName,
          "local",
          modelId,
          "", // LM Studio 默认无需 API Key
          chatBaseUrl
        );
      }

      return modelId;
    };

    // ============ 返回公共接口 ============
    return {
      // 状态
      baseUrl,
      apiKey,
      isOnline,
      models,
      isLoadingModels,
      isPulling,
      pullingModelId,
      downloadProgress,

      // 计算属性
      downloadPercent,
      loadedModels,

      // 方法
      checkStatus,
      loadModels,
      setBaseUrl,
      pullModel,
      loadModel,
      unloadModel,
      formatSize,
      oneClickDeploy,
    };
  },
  {
    persist: {
      key: "baiyu-aispace-lmstudio",
      paths: ["baseUrl", "apiKey"],
    },
  }
);
