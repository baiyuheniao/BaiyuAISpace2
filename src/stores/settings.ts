/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

// Preset providers with default base URLs
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
    baseUrl: "https://api.minimax.chat/v1",
  },
  yi: {
    name: "零一万物 (Yi)",
    baseUrl: "https://api.lingyiwanwu.com/v1",
  },
  custom: {
    name: "自定义 (OpenAI 兼容)",
    baseUrl: "http://localhost:11434/v1",
  },
};

// API Configuration interface
export interface ApiConfig {
  id: string;
  name: string; // Custom name for this config
  provider: string; // Key from PRESET_PROVIDERS
  baseUrl: string;
  model: string; // User manually inputs model name
  apiKey: string;
  createdAt: number;
}

// Embedding API Configuration interface - same structure as ApiConfig
export interface EmbeddingApiConfig {
  id: string;
  name: string;
  provider: string;
  baseUrl: string;
  model: string; // User manually inputs embedding model name
  apiKey: string;
  createdAt: number;
}

// Storage version - increment when schema changes
const STORAGE_VERSION = "6";
const STORAGE_VERSION_KEY = "baiyu-aispace-version";

// Check and clear old storage if version mismatch
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
    // Theme
    const darkMode = ref(true);
    const toggleTheme = () => {
      darkMode.value = !darkMode.value;
      applyTheme();
    };

    const initTheme = () => {
      applyTheme();
    };

    const applyTheme = () => {
      const html = document.documentElement;
      if (darkMode.value) {
        html.classList.add("dark");
      } else {
        html.classList.remove("dark");
      }
    };

    // API Configurations - multiple configs supported
    const apiConfigs = ref<ApiConfig[]>([]);
    const activeConfigId = ref<string | null>(null);

    // Embedding API Configurations - separate from LLM configs
    const embeddingApiConfigs = ref<EmbeddingApiConfig[]>([]);
    const activeEmbeddingApiConfigId = ref<string | null>(null);

    // Get active config
    const activeConfig = computed(() => {
      if (!activeConfigId.value) return null;
      return apiConfigs.value.find((c) => c.id === activeConfigId.value) || null;
    });

    // Get active embedding API config
    const activeEmbeddingApiConfig = computed(() => {
      if (!activeEmbeddingApiConfigId.value) return null;
      return embeddingApiConfigs.value.find((c) => c.id === activeEmbeddingApiConfigId.value) || null;
    });

    // Get embedding API config options for dropdown
    const embeddingApiConfigOptions = computed(() => {
      return embeddingApiConfigs.value.map((config) => ({
        label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider} - ${config.model})`,
        value: config.id,
      }));
    });

    // Get preset provider options for dropdown
    const presetProviderOptions = computed(() => {
      return Object.entries(PRESET_PROVIDERS).map(([key, value]) => ({
        label: value.name,
        value: key,
      }));
    });

    // Get all API config options for dropdown (used in chat)
    const apiConfigOptions = computed(() => {
      return apiConfigs.value.map((config) => ({
        label: `${config.name} (${PRESET_PROVIDERS[config.provider]?.name || config.provider})`,
        value: config.id,
      }));
    });

    // Create a new API config
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

    // Update an existing API config
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

    // Delete an API config
    const deleteApiConfig = (configId: string) => {
      apiConfigs.value = apiConfigs.value.filter((c) => c.id !== configId);
      
      // If active config is deleted, switch to another
      if (activeConfigId.value === configId) {
        activeConfigId.value = apiConfigs.value.length > 0 ? apiConfigs.value[0].id : null;
      }
      
      // Delete from secure storage
      deleteApiKeyFromSecureStorage(configId);
    };

    // Set active config
    const setActiveConfig = (configId: string | null) => {
      activeConfigId.value = configId;
    };

    // Create a new Embedding API config
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

    // Update an existing Embedding API config
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

    // Delete an Embedding API config
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

    // Load embedding API key from secure storage
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

    // Load all embedding API keys from secure storage
    const loadAllEmbeddingApiKeys = async () => {
      for (const config of embeddingApiConfigs.value) {
        await loadEmbeddingApiKeyForConfig(config.id);
      }
    };

    // Load API key from secure storage for a config
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

    // Load all API keys from secure storage
    const loadAllApiKeys = async () => {
      for (const config of apiConfigs.value) {
        await loadApiKeyForConfig(config.id);
      }
    };

    // Save API key to secure storage
    const saveApiKeyToSecureStorage = async (configId: string, apiKey: string) => {
      try {
        await invoke("save_api_key", { provider: configId, apiKey });
      } catch (error) {
        console.error("Failed to save API key:", error);
      }
    };

    // Delete API key from secure storage
    const deleteApiKeyFromSecureStorage = async (configId: string) => {
      try {
        await invoke("delete_api_key", { provider: configId });
      } catch (error) {
        console.error("Failed to delete API key:", error);
      }
    };

    // Get default base URL for a provider
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
