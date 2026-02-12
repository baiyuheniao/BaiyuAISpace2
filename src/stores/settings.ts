/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

export interface LLMProvider {
  id: string;
  name: string;
  apiKey: string;
  baseUrl: string;
  models: string[];
  selectedModel: string;
  modelsUrl?: string;
}

// Hardcoded providers list - this is the source of truth
// New providers added here will automatically appear in the UI
const DEFAULT_PROVIDERS: LLMProvider[] = [
  // 国际主流
  {
    id: "openai",
    name: "OpenAI",
    apiKey: "",
    baseUrl: "https://api.openai.com/v1",
    models: [
      "gpt-5",
      "gpt-5.1",
      "gpt-5.2",
      "gpt-5-mini",
      "gpt-4.1",
      "gpt-4.1-mini",
      "gpt-4o",
      "gpt-4o-mini",
      "o3",
      "o3-mini",
      "o4-mini",
      "gpt-4o-realtime",
      "gpt-4o-audio",
    ],
    selectedModel: "gpt-4o",
    modelsUrl: "https://platform.openai.com/docs/models",
  },
  {
    id: "anthropic",
    name: "Anthropic",
    apiKey: "",
    baseUrl: "https://api.anthropic.com/v1",
    models: [
      "claude-3-5-sonnet-20241022",
      "claude-3-5-haiku-20241022",
      "claude-3-opus-20240229",
      "claude-3-sonnet-20240229",
      "claude-3-haiku-20240307",
    ],
    selectedModel: "claude-3-5-sonnet-20241022",
    modelsUrl: "https://docs.anthropic.com/claude/docs/models-overview",
  },
  {
    id: "google",
    name: "Google Gemini",
    apiKey: "",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    models: [
      "gemini-2.0-pro",
      "gemini-2.0-flash",
      "gemini-2.0-flash-lite",
      "gemini-1.5-pro",
      "gemini-1.5-flash",
      "gemini-1.5-flash-8b",
    ],
    selectedModel: "gemini-1.5-pro",
    modelsUrl: "https://ai.google.dev/gemini-api/docs/models/gemini",
  },
  {
    id: "azure",
    name: "Azure OpenAI",
    apiKey: "",
    baseUrl: "https://your-resource.openai.azure.com/openai/deployments/",
    models: [
      "gpt-4o",
      "gpt-4",
      "gpt-35-turbo",
      "gpt-4-turbo",
    ],
    selectedModel: "gpt-4o",
    modelsUrl: "https://learn.microsoft.com/azure/ai-services/openai/concepts/models",
  },
  {
    id: "mistral",
    name: "Mistral AI",
    apiKey: "",
    baseUrl: "https://api.mistral.ai/v1",
    models: [
      "mistral-large-latest",
      "mistral-medium-latest",
      "mistral-small-latest",
      "codestral-latest",
      "pixtral-large-latest",
      "ministral-8b-latest",
      "ministral-3b-latest",
    ],
    selectedModel: "mistral-large-latest",
    modelsUrl: "https://docs.mistral.ai/getting-started/models/",
  },
  // 中国主流
  {
    id: "moonshot",
    name: "Moonshot (Kimi)",
    apiKey: "",
    baseUrl: "https://api.moonshot.cn/v1",
    models: [
      "kimi-k2.5",
      "kimi-k2-0905-preview",
      "kimi-k2-turbo-preview",
      "kimi-k2-thinking",
      "kimi-k2-thinking-turbo",
      "moonshot-v1-128k",
      "moonshot-v1-32k",
      "moonshot-v1-8k",
    ],
    selectedModel: "kimi-k2.5",
    modelsUrl: "https://platform.moonshot.cn/docs/models",
  },
  {
    id: "zhipu",
    name: "智谱 AI (GLM)",
    apiKey: "",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    models: [
      "glm-4-plus",
      "glm-4",
      "glm-4-air",
      "glm-4-air-250414",
      "glm-4-airx",
      "glm-4-flash",
      "glm-4-flash-250414",
      "glm-4v-plus",
      "glm-4v",
      "glm-4v-flash",
    ],
    selectedModel: "glm-4-air",
    modelsUrl: "https://open.bigmodel.cn/dev/howuse/model",
  },
  {
    id: "aliyun",
    name: "阿里通义千问",
    apiKey: "",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    models: [
      "qwen-max",
      "qwen-max-latest",
      "qwen-plus",
      "qwen-plus-latest",
      "qwen-turbo",
      "qwen-coder-plus",
      "qwen-coder-turbo",
      "qwen-vl-max",
      "qwen-vl-plus",
      "qwen-audio-turbo",
    ],
    selectedModel: "qwen-max",
    modelsUrl: "https://help.aliyun.com/zh/model-studio/models",
  },
  {
    id: "baidu",
    name: "百度文心一言",
    apiKey: "",
    baseUrl: "https://qianfan.baidubce.com/v2",
    models: [
      "ernie-4.0-8k-latest",
      "ernie-4.0-turbo-8k",
      "ernie-3.5-8k",
      "ernie-3.5-128k",
      "ernie-speed-128k",
      "ernie-speed-pro-128k",
      "ernie-lite-8k",
      "ernie-tiny-8k",
    ],
    selectedModel: "ernie-4.0-8k-latest",
    modelsUrl: "https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Nlks5zkzu",
  },
  {
    id: "doubao",
    name: "字节豆包",
    apiKey: "",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    models: [
      "doubao-pro-256k",
      "doubao-pro-128k",
      "doubao-pro-32k",
      "doubao-pro-4k",
      "doubao-lite-128k",
      "doubao-lite-32k",
      "doubao-lite-4k",
      "doubao-vision-pro-32k",
      "doubao-vision-lite-32k",
    ],
    selectedModel: "doubao-pro-32k",
    modelsUrl: "https://www.volcengine.com/docs/82379/1330310",
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    apiKey: "",
    baseUrl: "https://api.deepseek.com/v1",
    models: [
      "deepseek-chat",
      "deepseek-reasoner",
      "deepseek-coder",
    ],
    selectedModel: "deepseek-chat",
    modelsUrl: "https://platform.deepseek.com/api-docs/models",
  },
  {
    id: "siliconflow",
    name: "硅基流动 (SiliconFlow)",
    apiKey: "",
    baseUrl: "https://api.siliconflow.cn/v1",
    models: [
      "Qwen/Qwen2.5-72B-Instruct",
      "Qwen/Qwen2.5-32B-Instruct",
      "Qwen/Qwen2.5-14B-Instruct",
      "Qwen/Qwen2.5-7B-Instruct",
      "meta-llama/Meta-Llama-3.1-70B-Instruct",
      "meta-llama/Meta-Llama-3.1-8B-Instruct",
      "deepseek-ai/DeepSeek-V3",
      "deepseek-ai/DeepSeek-R1",
      "THUDM/glm-4-9b-chat",
      "01-ai/Yi-1.5-34B-Chat-16K",
      "Qwen/QwQ-32B-Preview",
    ],
    selectedModel: "Qwen/Qwen2.5-72B-Instruct",
    modelsUrl: "https://siliconflow.cn/models",
  },
  {
    id: "minimax",
    name: "MiniMax",
    apiKey: "",
    baseUrl: "https://api.minimax.chat/v1",
    models: [
      "abab6.5s-chat",
      "abab6.5-chat",
      "abab6-chat",
      "abab5.5-chat",
      "abab5-chat",
    ],
    selectedModel: "abab6.5-chat",
    modelsUrl: "https://platform.minimaxi.com/document/models",
  },
  {
    id: "yi",
    name: "零一万物 (Yi)",
    apiKey: "",
    baseUrl: "https://api.lingyiwanwu.com/v1",
    models: [
      "yi-large",
      "yi-medium",
      "yi-spark",
      "yi-large-rag",
      "yi-large-fc",
      "yi-medium-200k",
    ],
    selectedModel: "yi-large",
    modelsUrl: "https://platform.lingyiwanwu.com/docs",
  },
  // 自定义
  {
    id: "custom",
    name: "自定义 (OpenAI 兼容)",
    apiKey: "",
    baseUrl: "http://localhost:11434/v1",
    models: ["custom-model"],
    selectedModel: "custom-model",
  },
];

// Storage version - increment when providers list changes significantly
const STORAGE_VERSION = "2";
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

// Run version check on module load
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

    // LLM Providers - initialized with default list
    const providers = ref<LLMProvider[]>(JSON.parse(JSON.stringify(DEFAULT_PROVIDERS)));
    
    // Debug: log providers count
    console.log(`[SettingsStore] Initialized with ${providers.value.length} providers:`, 
      providers.value.map(p => p.name).join(", "));

    const activeProvider = ref<string>("openai");

    const currentProvider = computed(() => {
      return providers.value.find((p) => p.id === activeProvider.value) || providers.value[0];
    });

    // Secure API Key storage using system keyring
    const saveApiKeyToSecureStorage = async (providerId: string, apiKey: string) => {
      try {
        await invoke("save_api_key", { provider: providerId, apiKey });
        // Update local state but don't persist to localStorage
        const idx = providers.value.findIndex((p) => p.id === providerId);
        if (idx !== -1) {
          providers.value[idx].apiKey = apiKey;
        }
      } catch (error) {
        console.error("Failed to save API key to secure storage:", error);
      }
    };

    const loadApiKeyFromSecureStorage = async (providerId: string): Promise<string | null> => {
      try {
        const apiKey = await invoke<string | null>("get_api_key", { provider: providerId });
        if (apiKey) {
          const idx = providers.value.findIndex((p) => p.id === providerId);
          if (idx !== -1) {
            providers.value[idx].apiKey = apiKey;
          }
        }
        return apiKey;
      } catch (error) {
        console.error("Failed to load API key from secure storage:", error);
        return null;
      }
    };

    const loadAllApiKeys = async () => {
      for (const provider of providers.value) {
        await loadApiKeyFromSecureStorage(provider.id);
      }
    };

    const updateProvider = async (providerId: string, updates: Partial<LLMProvider>) => {
      const idx = providers.value.findIndex((p) => p.id === providerId);
      if (idx !== -1) {
        // If updating API key, save to secure storage
        if (updates.apiKey !== undefined) {
          await saveApiKeyToSecureStorage(providerId, updates.apiKey);
        }
        // Update other fields normally
        const { apiKey, ...otherUpdates } = updates;
        providers.value[idx] = { ...providers.value[idx], ...otherUpdates };
      }
    };

    const setActiveProvider = (providerId: string) => {
      activeProvider.value = providerId;
    };

    return {
      darkMode,
      toggleTheme,
      initTheme,
      providers,
      activeProvider,
      currentProvider,
      updateProvider,
      setActiveProvider,
      loadAllApiKeys,
      saveApiKeyToSecureStorage,
      loadApiKeyFromSecureStorage,
    };
  },
  {
    persist: {
      key: "baiyu-aispace-settings",
      // Note: apiKey is NOT persisted to localStorage, only to system keyring
      // providers is NOT persisted - use the hardcoded DEFAULT_PROVIDERS list
      // Only persist user selections: darkMode and activeProvider
      paths: ["darkMode", "activeProvider"],
    },
  }
);
