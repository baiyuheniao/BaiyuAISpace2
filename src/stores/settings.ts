/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

/**
 * 设置 Store - 管理应用全局设置
 * 
 * 功能说明:
 * - 主题模式切换 (深色/浅色)
 * - API 配置管理 (LLM 和 Embedding)
 * - API 密钥的安全存储 (通过 Tauri 后端)
 * 
 * 使用方式:
 * import { useSettingsStore } from "@/stores/settings";
 * const settings = useSettingsStore();
 */

// 预设的 LLM 提供商配置
// key: 提供商标识符
// name: 显示名称
// baseUrl: API 基础 URL
export const PRESET_PROVIDERS: Record<string, { name: string; baseUrl: string }> = {
  openai: {
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
  },
  anthropic: {
    name: "Anthropic",
    baseUrl: "https://api.anthropic.com/v1",
  },
  google: {
    name: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
  },
  azure: {
    name: "Azure OpenAI",
    baseUrl: "https://your-resource.openai.azure.com/openai/deployments/",
  },
  mistral: {
    name: "Mistral AI",
    baseUrl: "https://api.mistral.ai/v1",
  },
  moonshot: {
    name: "Moonshot (Kimi)",
    baseUrl: "https://api.moonshot.cn/v1",
  },
  zhipu: {
    name: "智谱 AI (GLM)",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
  },
  aliyun: {
    name: "阿里通义千问",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  },
  baidu: {
    name: "百度文心一言",
    baseUrl: "https://qianfan.baidubce.com/v2",
  },
  doubao: {
    name: "字节豆包",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
  },
  deepseek: {
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com/v1",
  },
  siliconflow: {
    name: "硅基流动 (SiliconFlow)",
    baseUrl: "https://api.siliconflow.cn/v1",
  },
  minimax: {
    name: "MiniMax",
    baseUrl: "https://api.minimax.io/v1",
  },
  yi: {
    name: "零一万物 (Yi)",
    baseUrl: "https://api.lingyiwanwu.com/v1",
  },
  local: {
    name: "本地模型 (Ollama)",
    baseUrl: "http://localhost:11434/v1",
  },
  openclaw: {
    name: "OpenClaw (本地网关)",
    baseUrl: "http://127.0.0.1:18789/v1",
  },
  custom: {
    name: "自定义 (OpenAI 兼容)",
    baseUrl: "http://localhost:11434/v1",
  },
};

/**
 * LLM API 配置接口
 * 用于配置各种大语言模型的 API 连接信息
 */
export interface ApiConfig {
  id: string;                      // 配置唯一标识符
  name: string;                    // 自定义配置名称 (如 "我的 GPT-4")
  provider: string;                // 提供商标识符 (对应 PRESET_PROVIDERS 的 key)
  baseUrl: string;                 // API 基础 URL
  model: string;                   // 模型名称 (如 gpt-4, claude-3-opus)
  apiKey: string;                  // API 密钥 (会存储到系统安全存储)
  maxTokens?: number;              // 最大输出 token 数（不填则后端默认 4096）
  createdAt: number;               // 创建时间戳
}

/**
 * Embedding API 配置接口
 * 用于配置文本嵌入模型的 API (知识库向量化用)
 * 结构与 ApiConfig 相同
 */
export interface EmbeddingApiConfig {
  id: string;
  name: string;
  provider: string;
  baseUrl: string;
  model: string;
  apiKey: string;
  createdAt: number;
}

/**
 * Reranker API 配置接口
 * 用于配置 Cohere-compatible Reranker 模型 (RAG 精排用)
 */
export interface RerankerApiConfig {
  id: string;
  name: string;
  provider: string;
  baseUrl: string;  // e.g. https://api.cohere.com
  model: string;    // e.g. rerank-multilingual-v3.0
  apiKey: string;
  createdAt: number;
}

// 存储版本号 - 当数据结构变更时需要递增
const STORAGE_VERSION = "6";
const STORAGE_VERSION_KEY = "baiyu-aispace-version";

/**
 * 检查并处理存储版本
 * 如果版本号变化,清除旧数据以避免兼容性问题
 */
const checkStorageVersion = () => {
  const storedVersion = localStorage.getItem(STORAGE_VERSION_KEY);
  if (storedVersion !== STORAGE_VERSION) {
    console.log(`Storage version changed from ${storedVersion} to ${STORAGE_VERSION}, clearing old data...`);
    localStorage.removeItem("baiyu-aispace-settings");
    localStorage.setItem(STORAGE_VERSION_KEY, STORAGE_VERSION);
  }
};

checkStorageVersion();

export const useSettingsStore = defineStore(
  "settings",
  () => {
    // ============ 主题相关状态 ============
    
    // 深色模式开关
    const darkMode = ref(true);
    
    // 切换深色/浅色主题
    const toggleTheme = () => {
      darkMode.value = !darkMode.value;
      applyTheme();
    };

    // 初始化主题设置
    const initTheme = () => {
      applyTheme();
    };

    // 应用主题到 HTML 元素
    const applyTheme = () => {
      const html = document.documentElement;
      if (darkMode.value) {
        html.classList.add("dark");
      } else {
        html.classList.remove("dark");
      }
    };

    // ============ 系统托盘相关状态 ============

    // 同步失败的一次性提醒队列。store 拿不到 NMessageProvider 上下文，没法直接
    // 弹窗，改成让 Layout.vue watch 这个队列后弹出——静默失败会让用户以为设置
    // 已生效，实际上后端根本不知道。
    const syncErrorNotices = ref<string[]>([]);

    // 关闭窗口按钮的行为：true = 最小化到系统托盘，false = 直接退出程序
    const closeToTray = ref(true);

    // 设置关闭按钮行为，并同步给 Rust 后端（窗口关闭事件在后端拦截，需要后端知道当前设置）
    const setCloseToTray = async (enabled: boolean) => {
      closeToTray.value = enabled;
      await syncCloseToTray();
    };

    // 将当前 closeToTray 值同步给后端（应用启动时调用一次，之后每次修改再调用）
    const syncCloseToTray = async () => {
      try {
        await invoke("set_close_to_tray", { enabled: closeToTray.value });
      } catch (error) {
        console.error("Failed to sync close-to-tray setting:", error);
        syncErrorNotices.value.push(`"关闭窗口时最小化到托盘"设置未能同步生效：${error}`);
      }
    };

    // 从托盘唤起主窗口的全局快捷键（Tauri accelerator 格式，如 "Ctrl+Alt+Space"）
    const showHotkey = ref("Ctrl+Alt+Space");

    // 设置唤起快捷键，并同步给后端注册（失败会抛出，调用方需自行提示用户）
    const setShowHotkey = async (accelerator: string) => {
      await invoke("set_show_hotkey", { accelerator });
      showHotkey.value = accelerator;
    };

    // 将当前 showHotkey 值同步给后端（应用启动时调用一次）
    const syncShowHotkey = async () => {
      try {
        await invoke("set_show_hotkey", { accelerator: showHotkey.value });
      } catch (error) {
        console.error("Failed to sync show-hotkey setting:", error);
        syncErrorNotices.value.push(`唤起快捷键 ${showHotkey.value} 注册失败，可能已被其他程序占用：${error}`);
      }
    };

    // 新建会话的应用内快捷键（纯前端 window keydown 监听，只在应用窗口
    // 获得焦点时生效——不同于上面 showHotkey 那个要注册进操作系统的
    // 全局快捷键，这个不需要经后端，直接改本地状态即可）
    const newSessionHotkey = ref("Ctrl+K");

    const setNewSessionHotkey = (accelerator: string) => {
      newSessionHotkey.value = accelerator;
    };

    // 切换全屏的应用内快捷键（同样是纯前端监听，不经后端）
    const fullscreenHotkey = ref("F11");

    const setFullscreenHotkey = (accelerator: string) => {
      fullscreenHotkey.value = accelerator;
    };

    // 全局默认 System Prompt，发送每次对话请求时会自动附加到系统消息中
    const systemPrompt = ref("");

    // 服务商返回限流/过载类错误时的自动重试次数与间隔秒数，随每次对话请求
    // 一起传给后端；默认值需与 src-tauri/src/commands/constants.rs 里的
    // DEFAULT_LLM_RETRY_COUNT / DEFAULT_LLM_RETRY_INTERVAL_SECS 保持一致。
    const retryCount = ref(3);
    const retryIntervalSecs = ref(2);

    // ============ API 配置状态 ============
    
    // LLM API 配置列表 (支持多配置)
    const apiConfigs = ref<ApiConfig[]>([]);
    
    // 当前激活的 LLM 配置 ID
    const activeConfigId = ref<string | null>(null);

    // Embedding API 配置列表 (与 LLM 配置分开)
    const embeddingApiConfigs = ref<EmbeddingApiConfig[]>([]);

    // 当前激活的 Embedding 配置 ID
    const activeEmbeddingApiConfigId = ref<string | null>(null);

    // Reranker API 配置列表
    const rerankerApiConfigs = ref<RerankerApiConfig[]>([]);

    // ============ 计算属性 ============

    // 获取当前激活的 LLM 配置
    const activeConfig = computed(() => {
      if (!activeConfigId.value) return null;
      return apiConfigs.value.find((c) => c.id === activeConfigId.value) || null;
    });

    // 获取当前激活的 Embedding 配置
    const activeEmbeddingApiConfig = computed(() => {
      if (!activeEmbeddingApiConfigId.value) return null;
      return embeddingApiConfigs.value.find((c) => c.id === activeEmbeddingApiConfigId.value) || null;
    });

    // 获取 Embedding 配置下拉选项
    const embeddingApiConfigOptions = computed(() => {
      return embeddingApiConfigs.value.map((config) => ({
        label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider} - ${config.model})`,
        value: config.id,
      }));
    });

    // 获取 Reranker 配置下拉选项
    const rerankerApiConfigOptions = computed(() => {
      return rerankerApiConfigs.value.map((config) => ({
        label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider} - ${config.model})`,
        value: config.id,
      }));
    });

    // 获取预设提供商下拉选项
    const presetProviderOptions = computed(() => {
      return Object.entries(PRESET_PROVIDERS).map(([key, value]) => ({
        label: value.name,
        value: key,
      }));
    });

    // 获取 API 配置下拉选项 (聊天页面使用)
    const apiConfigOptions = computed(() => {
      return apiConfigs.value.map((config) => ({
        label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider})`,
        value: config.id,
      }));
    });

    // ============ 方法函数 ============

    /**
     * 创建新的 LLM API 配置
     * @param name 配置名称
     * @param provider 提供商标识符
     * @param model 模型名称
     * @param apiKey API 密钥
     * @param customBaseUrl 自定义 API 地址 (可选)
     */
    const createApiConfig = (
      name: string,
      provider: string,
      model: string,
      apiKey: string,
      customBaseUrl?: string,
      maxTokens?: number
    ): ApiConfig => {
      const preset = PRESET_PROVIDERS[provider];
      const config: ApiConfig = {
        id: crypto.randomUUID(),
        name,
        provider,
        baseUrl: customBaseUrl || preset?.baseUrl || "",
        model,
        apiKey,
        maxTokens,
        createdAt: Date.now(),
      };
      apiConfigs.value.push(config);
      
      // Save API key to secure storage
      saveApiKeyToSecureStorage(config.id, apiKey);
      
      // If first config, set as active
      if (apiConfigs.value.length === 1) {
        activeConfigId.value = config.id;
      }
      
      return config;
    };

    // 更新现有 LLM API 配置
    const updateApiConfig = (configId: string, updates: Partial<ApiConfig>) => {
      const idx = apiConfigs.value.findIndex((c) => c.id === configId);
      if (idx === -1) return;

      const config = apiConfigs.value[idx];
      const { apiKey, ...safeUpdates } = updates;

      // 编辑页约定“留空表示不修改”：空值既不能覆盖内存状态，也不能写入系统密钥链。
      if (typeof apiKey === "string" && apiKey.trim()) {
        if (apiKey !== config.apiKey) {
          saveApiKeyToSecureStorage(configId, apiKey);
        }
        apiConfigs.value[idx] = { ...config, ...safeUpdates, apiKey };
      } else {
        apiConfigs.value[idx] = { ...config, ...safeUpdates };
      }
    };

    // 删除 LLM API 配置
    const deleteApiConfig = (configId: string) => {
      apiConfigs.value = apiConfigs.value.filter((c) => c.id !== configId);
      
      // If active config is deleted, switch to another
      if (activeConfigId.value === configId) {
        activeConfigId.value = apiConfigs.value.length > 0 ? apiConfigs.value[0].id : null;
      }
      
      // 删除安全存储中的密钥
      deleteApiKeyFromSecureStorage(configId);
    };

    // 设置当前激活的配置
    const setActiveConfig = (configId: string | null) => {
      activeConfigId.value = configId;
    };

    // 创建新的 Embedding API 配置
    const createEmbeddingApiConfig = (
      name: string,
      provider: string,
      model: string,
      apiKey: string,
      customBaseUrl?: string
    ): EmbeddingApiConfig => {
      const preset = PRESET_PROVIDERS[provider];
      const config: EmbeddingApiConfig = {
        id: crypto.randomUUID(),
        name,
        provider,
        baseUrl: customBaseUrl || preset?.baseUrl || "",
        model,
        apiKey,
        createdAt: Date.now(),
      };
      embeddingApiConfigs.value.push(config);
      
      // Save API key to secure storage with prefix
      saveApiKeyToSecureStorage(`emb_${config.id}`, apiKey);
      
      // If first config, set as active
      if (embeddingApiConfigs.value.length === 1) {
        activeEmbeddingApiConfigId.value = config.id;
      }
      
      return config;
    };

    // 更新现有 Embedding API 配置
    const updateEmbeddingApiConfig = (configId: string, updates: Partial<EmbeddingApiConfig>) => {
      const idx = embeddingApiConfigs.value.findIndex((c) => c.id === configId);
      if (idx === -1) return;

      const config = embeddingApiConfigs.value[idx];
      const { apiKey, ...safeUpdates } = updates;

      if (typeof apiKey === "string" && apiKey.trim()) {
        if (apiKey !== config.apiKey) {
          saveApiKeyToSecureStorage(`emb_${configId}`, apiKey);
        }
        embeddingApiConfigs.value[idx] = { ...config, ...safeUpdates, apiKey };
      } else {
        embeddingApiConfigs.value[idx] = { ...config, ...safeUpdates };
      }
    };

    // 删除 Embedding API 配置
    const deleteEmbeddingApiConfig = (configId: string) => {
      embeddingApiConfigs.value = embeddingApiConfigs.value.filter((c) => c.id !== configId);
      
      // If active config is deleted, switch to another
      if (activeEmbeddingApiConfigId.value === configId) {
        activeEmbeddingApiConfigId.value = embeddingApiConfigs.value.length > 0 ? embeddingApiConfigs.value[0].id : null;
      }
      
      // Delete from secure storage
      deleteApiKeyFromSecureStorage(`emb_${configId}`);
    };

    // Set active embedding API config
    const setActiveEmbeddingApiConfig = (configId: string | null) => {
      activeEmbeddingApiConfigId.value = configId;
    };

    // 创建新的 Reranker API 配置
    const createRerankerApiConfig = (
      name: string,
      provider: string,
      model: string,
      apiKey: string,
      customBaseUrl?: string
    ): RerankerApiConfig => {
      const preset = PRESET_PROVIDERS[provider];
      const config: RerankerApiConfig = {
        id: crypto.randomUUID(),
        name,
        provider,
        baseUrl: customBaseUrl || preset?.baseUrl || "",
        model,
        apiKey,
        createdAt: Date.now(),
      };
      rerankerApiConfigs.value.push(config);
      saveApiKeyToSecureStorage(`reranker_${config.id}`, apiKey);
      return config;
    };

    // 更新 Reranker API 配置
    const updateRerankerApiConfig = (configId: string, updates: Partial<RerankerApiConfig>) => {
      const idx = rerankerApiConfigs.value.findIndex((c) => c.id === configId);
      if (idx === -1) return;

      const config = rerankerApiConfigs.value[idx];
      const { apiKey, ...safeUpdates } = updates;

      if (typeof apiKey === "string" && apiKey.trim()) {
        if (apiKey !== config.apiKey) {
          saveApiKeyToSecureStorage(`reranker_${configId}`, apiKey);
        }
        rerankerApiConfigs.value[idx] = { ...config, ...safeUpdates, apiKey };
      } else {
        rerankerApiConfigs.value[idx] = { ...config, ...safeUpdates };
      }
    };

    // 删除 Reranker API 配置
    const deleteRerankerApiConfig = (configId: string) => {
      rerankerApiConfigs.value = rerankerApiConfigs.value.filter((c) => c.id !== configId);
      deleteApiKeyFromSecureStorage(`reranker_${configId}`);
    };

    // 加载 Reranker API 密钥
    const loadRerankerApiKeyForConfig = async (configId: string): Promise<string | null> => {
      try {
        const apiKey = await invoke<string | null>("get_api_key", { provider: `reranker_${configId}` });
        if (apiKey) {
          const idx = rerankerApiConfigs.value.findIndex((c) => c.id === configId);
          if (idx !== -1) rerankerApiConfigs.value[idx].apiKey = apiKey;
        }
        return apiKey;
      } catch {
        return null;
      }
    };

    const loadAllRerankerApiKeys = async () => {
      for (const config of rerankerApiConfigs.value) {
        await loadRerankerApiKeyForConfig(config.id);
      }
    };

    // 从安全存储加载指定配置的 Embedding API 密钥
    const loadEmbeddingApiKeyForConfig = async (configId: string): Promise<string | null> => {
      try {
        const apiKey = await invoke<string | null>("get_api_key", { provider: `emb_${configId}` });
        if (apiKey) {
          const idx = embeddingApiConfigs.value.findIndex((c) => c.id === configId);
          if (idx !== -1) {
            embeddingApiConfigs.value[idx].apiKey = apiKey;
          }
        }
        return apiKey;
      } catch (error) {
        console.error("Failed to load embedding API key:", error);
        return null;
      }
    };

    // 加载所有 Embedding API 密钥
    const loadAllEmbeddingApiKeys = async () => {
      for (const config of embeddingApiConfigs.value) {
        await loadEmbeddingApiKeyForConfig(config.id);
      }
    };

    // 从安全存储加载指定配置的 API 密钥
    const loadApiKeyForConfig = async (configId: string): Promise<string | null> => {
      try {
        const apiKey = await invoke<string | null>("get_api_key", { provider: configId });
        if (apiKey) {
          const idx = apiConfigs.value.findIndex((c) => c.id === configId);
          if (idx !== -1) {
            apiConfigs.value[idx].apiKey = apiKey;
          }
        }
        return apiKey;
      } catch (error) {
        console.error("Failed to load API key:", error);
        return null;
      }
    };

    // 加载所有 API 密钥
    const loadAllApiKeys = async () => {
      await Promise.all([
        Promise.all(apiConfigs.value.map((config) => loadApiKeyForConfig(config.id))),
        loadAllEmbeddingApiKeys(),
        loadAllRerankerApiKeys(),
      ]);
    };

    // 保存 API 密钥到安全存储
    const saveApiKeyToSecureStorage = async (configId: string, apiKey: string) => {
      try {
        await invoke("save_api_key", { provider: configId, apiKey });
      } catch (error) {
        console.error("Failed to save API key:", error);
      }
    };

    // 从安全存储删除 API 密钥
    const deleteApiKeyFromSecureStorage = async (configId: string) => {
      try {
        await invoke("delete_api_key", { provider: configId });
      } catch (error) {
        console.error("Failed to delete API key:", error);
      }
    };

    // 获取提供商的默认 API 地址
    const getDefaultBaseUrl = (provider: string): string => {
      return PRESET_PROVIDERS[provider]?.baseUrl || "";
    };

    return {
      darkMode,
      toggleTheme,
      initTheme,
      syncErrorNotices,
      closeToTray,
      setCloseToTray,
      syncCloseToTray,
      showHotkey,
      setShowHotkey,
      syncShowHotkey,
      newSessionHotkey,
      setNewSessionHotkey,
      fullscreenHotkey,
      setFullscreenHotkey,
      systemPrompt,
      retryCount,
      retryIntervalSecs,
      apiConfigs,
      activeConfigId,
      activeConfig,
      presetProviderOptions,
      apiConfigOptions,
      createApiConfig,
      updateApiConfig,
      deleteApiConfig,
      setActiveConfig,
      loadAllApiKeys,
      getDefaultBaseUrl,
      // Embedding API configs
      embeddingApiConfigs,
      activeEmbeddingApiConfigId,
      activeEmbeddingApiConfig,
      embeddingApiConfigOptions,
      createEmbeddingApiConfig,
      updateEmbeddingApiConfig,
      deleteEmbeddingApiConfig,
      setActiveEmbeddingApiConfig,
      loadEmbeddingApiKeyForConfig,
      loadAllEmbeddingApiKeys,
      // Reranker API configs
      rerankerApiConfigs,
      rerankerApiConfigOptions,
      createRerankerApiConfig,
      updateRerankerApiConfig,
      deleteRerankerApiConfig,
      loadRerankerApiKeyForConfig,
      loadAllRerankerApiKeys,
    };
  },
  {
    persist: {
      key: "baiyu-aispace-settings",
      paths: ["darkMode", "closeToTray", "showHotkey", "newSessionHotkey", "fullscreenHotkey", "systemPrompt", "retryCount", "retryIntervalSecs", "apiConfigs", "activeConfigId", "embeddingApiConfigs", "activeEmbeddingApiConfigId", "rerankerApiConfigs"],
      // apiKey lives in secure storage (see saveApiKeyToSecureStorage) and is
      // only kept in these arrays in-memory for request building. Without
      // this serializer it would otherwise round-trip into plaintext
      // localStorage on every mutation; loadAllApiKeys()/loadAllEmbeddingApiKeys()
      // re-populate it from secure storage on startup, so stripping it here is safe.
      serializer: {
        serialize: (state: Record<string, unknown>) => {
          const stripApiKey = (configs: unknown) =>
            Array.isArray(configs) ? configs.map(({ apiKey: _apiKey, ...rest }) => rest) : configs;
          return JSON.stringify({
            ...state,
            apiConfigs: stripApiKey(state.apiConfigs),
            embeddingApiConfigs: stripApiKey(state.embeddingApiConfigs),
            rerankerApiConfigs: stripApiKey(state.rerankerApiConfigs),
          });
        },
        deserialize: (raw: string) => JSON.parse(raw),
      },
    },
  }
);
