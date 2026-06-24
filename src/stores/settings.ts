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

    // ============ API 配置状态 ============
    
    // LLM API 配置列表 (支持多配置)
    const apiConfigs = ref<ApiConfig[]>([]);
    
    // 当前激活的 LLM 配置 ID
    const activeConfigId = ref<string | null>(null);

    // Embedding API 配置列表 (与 LLM 配置分开)
    const embeddingApiConfigs = ref<EmbeddingApiConfig[]>([]);
    
    // 当前激活的 Embedding 配置 ID
    const activeEmbeddingApiConfigId = ref<string | null>(null);

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
      customBaseUrl?: string
    ): ApiConfig => {
      const preset = PRESET_PROVIDERS[provider];
      const config: ApiConfig = {
        id: crypto.randomUUID(),
        name,
        provider,
        baseUrl: customBaseUrl || preset?.baseUrl || "",
        model,
        apiKey,
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
      
      // If API key is updated, save to secure storage
      if (updates.apiKey !== undefined && updates.apiKey !== config.apiKey) {
        saveApiKeyToSecureStorage(configId, updates.apiKey);
      }
      
      apiConfigs.value[idx] = { ...config, ...updates };
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
      
      // If API key is updated, save to secure storage
      if (updates.apiKey !== undefined && updates.apiKey !== config.apiKey) {
        saveApiKeyToSecureStorage(`emb_${configId}`, updates.apiKey);
      }
      
      embeddingApiConfigs.value[idx] = { ...config, ...updates };
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
      for (const config of apiConfigs.value) {
        await loadApiKeyForConfig(config.id);
      }
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
    };
  },
  {
    persist: {
      key: "baiyu-aispace-settings",
      paths: ["darkMode", "apiConfigs", "activeConfigId", "embeddingApiConfigs", "activeEmbeddingApiConfigId"],
    },
  }
);
