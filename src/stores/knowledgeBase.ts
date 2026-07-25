/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * 知识库 Store - 管理 RAG (检索增强生成) 相关的知识库功能
 * 
 * 功能说明:
 * - 知识库的 CRUD 操作 (创建、读取、更新、删除)
 * - 文档上传和管理
 * - 文本分块 (Chunking) 配置
 * - 相似度检索
 * 
 * 使用方式:
 * import { useKnowledgeBaseStore } from "@/stores/knowledgeBase";
 * const kbStore = useKnowledgeBaseStore();
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useSettingsStore } from "./settings";
import { classifyError } from "@/utils/errorMessage";

// ============ 类型定义 (与 Rust 后端对应) ============

/**
 * 知识库类型
 * 表示一个完整的知识库对象
 */
export interface KnowledgeBase {
  id: string;                       // 知识库唯一标识符
  name: string;                    // 知识库名称
  description: string;             // 知识库描述
  embedding_api_config_id: string; // 关联的 Embedding API 配置 ID
  embedding_provider: string;      // Embedding 服务商 (创建时从配置中快照)
  embedding_model: string;         // Embedding 模型名称 (创建时从配置中快照)
  embedding_base_url: string;      // Embedding API Base URL (创建时从配置中快照)
  chunk_size: number;              // 文本分块大小 (字符数)
  chunk_overlap: number;           // 分块重叠大小
  created_at: number;              // 创建时间戳
  updated_at: number;              // 更新时间戳
  document_count: number;          // 包含的文档数量
}

/**
 * 文档类型
 * 表示知识库中的一个文档
 */
export interface Document {
  id: string;                      // 文档唯一标识符
  kb_id: string;                  // 所属知识库 ID
  filename: string;               // 文件名
  file_type: string;             // 文件类型 (如 pdf, txt, md)
  file_size: number;              // 文件大小 (字节)
  file_hash: string;              // 文件内容哈希 (用于去重)
  content_preview: string;         // 内容预览 (前 200 字符)
  chunk_count: number;            // 分块数量
  status: "processing" | "completed" | "error";  // 处理状态
  error_message?: string;         // 错误信息 (如果有)
  created_at: number;             // 创建时间戳
}

/**
 * 文本块类型
 * 文档分割后的最小检索单元
 */
export interface Chunk {
  id: string;                      // 分块唯一标识符
  document_id: string;            // 所属文档 ID
  kb_id: string;                  // 所属知识库 ID
  content: string;                // 分块内容
  chunk_index: number;            // 分块索引
  token_count: number;            // token 数量
}

/**
 * 检索结果中的分块
 * 包含分块信息和相似度分数
 */
export interface RetrievedChunk {
  chunk: Chunk;                   // 原始分块数据
  score: number;                  // 综合相似度分数
  vector_score?: number;          // 向量相似度分数
  keyword_score?: number;         // 关键词匹配分数
  document_filename: string;      // 来源文档文件名
}

/**
 * 检索结果类型
 */
export interface RetrievalResult {
  query: string;                  // 检索查询文本
  chunks: RetrievedChunk[];       // 检索到的相关分块
  total_chunks: number;           // 符合阈值的总分块数
  warnings?: RetrievalWarning[];  // 可回退错误，例如 Reranker 失败
}

export interface RetrievalWarning {
  code: string;
  message: string;
}

export interface KnowledgeBaseNotice {
  type: "warning" | "error";
  title: string;
  message: string;
}

/**
 * 检索模式类型
 * - vector: 向量检索 (语义相似度)
 * - keyword: 关键词检索
 * - hybrid: 混合检索 (向量 + 关键词)
 */
export type RetrievalMode = "vector" | "keyword" | "hybrid";

/**
 * 创建知识库请求类型
 */
export interface CreateKnowledgeBaseRequest {
  name: string;                   // 知识库名称
  description: string;            // 知识库描述
  embedding_api_config_id: string; // Embedding API 配置 ID
  embedding_provider: string;     // Embedding 服务商 (从选中的配置中取出)
  embedding_model: string;        // Embedding 模型名称 (从选中的配置中取出)
  embedding_base_url: string;     // Embedding API Base URL (从选中的配置中取出)
  chunk_size?: number;           // 分块大小 (可选)
  chunk_overlap?: number;        // 分块重叠 (可选)
}

/**
 * 检索设置类型
 */
export interface RetrievalSettings {
  mode: RetrievalMode;            // 检索模式
  topK: number;                   // 返回结果数量
  similarityThreshold: number;    // 相似度阈值
  enableReranker: boolean;        // 是否启用 Reranker 精排
  rerankerConfigId?: string;      // 选用的 Reranker 配置 ID
  rerankTopN?: number;            // 精排后保留条数（默认等于 topK）
}

export const MIN_SIMILARITY_THRESHOLD = 0.3;
export const MAX_SIMILARITY_THRESHOLD = 0.8;
export const DEFAULT_SIMILARITY_THRESHOLD = 0.5;
const RETRIEVAL_SETTINGS_VERSION = 1;

export const normalizeSimilarityThreshold = (value: number): number => {
  const finiteValue = Number.isFinite(value) ? value : DEFAULT_SIMILARITY_THRESHOLD;
  return Math.min(
    MAX_SIMILARITY_THRESHOLD,
    Math.max(MIN_SIMILARITY_THRESHOLD, finiteValue),
  );
};

export const useKnowledgeBaseStore = defineStore("knowledgeBase", () => {
  // ============ 响应式状态 ============
  
  // 知识库列表
  const knowledgeBases = ref<KnowledgeBase[]>([]);
  
  // 当前选中的知识库
  const currentKb = ref<KnowledgeBase | null>(null);
  
  // 当前知识库的文档列表
  const documents = ref<Document[]>([]);
  
  // 是否正在加载
  const loading = ref(false);
  
  // 文档导入进度
  const importProgress = ref<{ current: number; total: number } | null>(null);

  // Store 无法直接使用 Naive UI 的通知上下文，统一塞进队列，由 Layout 在
  // 左下角弹出。Reranker 回退是 warning，完整检索失败是 error。
  const retrievalNotices = ref<KnowledgeBaseNotice[]>([]);
  
  // 检索设置
  const retrievalSettings = ref<RetrievalSettings>({
    mode: "hybrid",
    topK: 5,
    similarityThreshold: DEFAULT_SIMILARITY_THRESHOLD,
    enableReranker: false,
  });
  const retrievalSettingsVersion = ref(0);

  const migrateRetrievalSettings = () => {
    if (retrievalSettingsVersion.value < RETRIEVAL_SETTINGS_VERSION) {
      // 0.7 是旧版本的默认值，也正是这次把正确片段全部过滤掉的根因。
      // 一次性迁移到 0.5；之后用户在新范围内的选择会原样保留。
      if (Math.abs(retrievalSettings.value.similarityThreshold - 0.7) < 0.0001) {
        retrievalSettings.value.similarityThreshold = DEFAULT_SIMILARITY_THRESHOLD;
      }
      retrievalSettingsVersion.value = RETRIEVAL_SETTINGS_VERSION;
    }

    retrievalSettings.value.similarityThreshold = normalizeSimilarityThreshold(
      retrievalSettings.value.similarityThreshold,
    );
  };

  // ============ 计算属性 ============
  
  // 获取当前知识库的文档列表
  const currentKbDocuments = computed(() => {
    if (!currentKb.value) return [];
    return documents.value.filter((d) => d.kb_id === currentKb.value!.id);
  });

  // ============ 方法函数 ============

  // 加载所有知识库列表
  const loadKnowledgeBases = async () => {
    migrateRetrievalSettings();
    loading.value = true;
    try {
      const result = await invoke<KnowledgeBase[]>("list_knowledge_bases");
      knowledgeBases.value = result;
    } catch (error) {
      console.error("Failed to load knowledge bases:", error);
    } finally {
      loading.value = false;
    }
  };

  const createKnowledgeBase = async (
    request: CreateKnowledgeBaseRequest
  ): Promise<KnowledgeBase | null> => {
    try {
      console.log("[KB] Creating knowledge base with request:", JSON.stringify(request));
      const result = await invoke<KnowledgeBase>("create_knowledge_base", {
        request,
      });
      knowledgeBases.value.unshift(result);
      return result;
    } catch (error) {
      console.error("[KB] Failed to create knowledge base:", error);
      return null;
    }
  };

  const deleteKnowledgeBase = async (kbId: string): Promise<boolean> => {
    try {
      await invoke("delete_knowledge_base", { kbId });
      knowledgeBases.value = knowledgeBases.value.filter((kb) => kb.id !== kbId);
      if (currentKb.value?.id === kbId) {
        currentKb.value = null;
      }
      return true;
    } catch (error) {
      console.error("Failed to delete knowledge base:", error);
      return false;
    }
  };

  const setCurrentKb = async (kb: KnowledgeBase | null) => {
    currentKb.value = kb;
    if (kb) {
      await loadDocuments(kb.id);
    } else {
      documents.value = [];
    }
  };

  const loadDocuments = async (kbId: string) => {
    try {
      const result = await invoke<Document[]>("list_documents", { kbId });
      documents.value = result;
    } catch (error) {
      console.error("Failed to load documents:", error);
    }
  };

  /**
   * Import document to knowledge base
   * Note: API key is no longer passed from frontend (#32).
   * Backend retrieves it from secure storage using the KB's embedding_api_config_id.
   */
  const importDocument = async (
    kbId: string,
    filePath: string,
  ): Promise<boolean> => {
    try {
      await invoke("import_document", {
        kbId,
        filePath,
      });
      await loadDocuments(kbId);
      await loadKnowledgeBases(); // Refresh document count
      return true;
    } catch (error) {
      console.error("Failed to import document:", error);
      // 后端在失败时仍会把文档行写成 status='error' + error_message（方便定位原因，
      // 比如 embedding 模型的单次输入长度限制），所以这里也要刷新列表，
      // 否则这条失败记录永远不会出现在 UI 里，用户只能看到一个空泛的"导入失败"提示
      await loadDocuments(kbId);
      await loadKnowledgeBases();
      return false;
    }
  };

  const selectAndImportDocument = async (
    kbId: string,
  ): Promise<boolean> => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Documents",
            extensions: [
              "pdf",
              "docx",
              "doc",
              "xlsx",
              "xls",
              "csv",
              "pptx",
              "md",
              "markdown",
              "html",
              "htm",
              "txt",
              "rs",
              "js",
              "ts",
              "py",
              "java",
              "c",
              "cpp",
              "h",
              "go",
            ],
          },
        ],
      });

      if (selected && typeof selected === "string") {
        return await importDocument(kbId, selected);
      }
      return false;
    } catch (error) {
      console.error("Failed to select file:", error);
      return false;
    }
  };

  const deleteDocument = async (docId: string, kbId: string): Promise<boolean> => {
    try {
      await invoke("delete_document", { docId, kbId });
      documents.value = documents.value.filter((d) => d.id !== docId);
      await loadKnowledgeBases(); // Refresh document count
      return true;
    } catch (error) {
      console.error("Failed to delete document:", error);
      return false;
    }
  };

  /**
   * Search knowledge base
   * Note: API key is no longer passed from frontend (#32).
   * Backend retrieves it from secure storage using the KB's embedding_api_config_id.
   */
  const searchKnowledgeBase = async (
    kbId: string,
    query: string,
  ): Promise<RetrievalResult | null> => {
    migrateRetrievalSettings();
    try {
      // Build optional reranker params
      const rerankerParams: Record<string, unknown> = {};
      if (retrievalSettings.value.enableReranker && retrievalSettings.value.rerankerConfigId) {
        const settingsStore = useSettingsStore();
        const cfg = settingsStore.rerankerApiConfigs.find(
          (c) => c.id === retrievalSettings.value.rerankerConfigId
        );
        if (cfg) {
          rerankerParams.rerankerConfigId = cfg.id;
          rerankerParams.rerankerBaseUrl = cfg.baseUrl;
          rerankerParams.rerankerModel = cfg.model;
          rerankerParams.rerankTopN = retrievalSettings.value.rerankTopN ?? retrievalSettings.value.topK;
        }
      }

      const result = await invoke<RetrievalResult>("search_knowledge_base", {
        request: {
          kbId,
          query,
          topK: retrievalSettings.value.topK,
          retrievalMode: retrievalSettings.value.mode,
          similarityThreshold: normalizeSimilarityThreshold(
            retrievalSettings.value.similarityThreshold,
          ),
          windowSize: 1, // fetch ±1 adjacent chunks to give LLM richer context
          ...rerankerParams,
        },
      });
      for (const warning of result.warnings ?? []) {
        retrievalNotices.value.push({
          type: "warning",
          title: warning.code === "reranker_fallback" ? "精排暂时不可用" : "知识库检索已降级",
          message: warning.message,
        });
      }
      return result;
    } catch (error) {
      console.error("Failed to search knowledge base:", error);
      retrievalNotices.value.push({
        type: "error",
        title: "知识库检索失败",
        message: `${classifyError(error).message}。本次将不使用知识库内容。`,
      });
      return null;
    }
  };

  const updateRetrievalSettings = (settings: Partial<RetrievalSettings>) => {
    retrievalSettings.value = { ...retrievalSettings.value, ...settings };
    retrievalSettings.value.similarityThreshold = normalizeSimilarityThreshold(
      retrievalSettings.value.similarityThreshold,
    );
  };

  // Format file size
  const formatFileSize = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  // Format date
  const formatDate = (timestamp: number): string => {
    const date = new Date(timestamp);
    return date.toLocaleDateString("zh-CN", {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  return {
    // State
    knowledgeBases,
    currentKb,
    documents,
    loading,
    importProgress,
    retrievalSettings,
    retrievalSettingsVersion,
    retrievalNotices,
    
    // Getters
    currentKbDocuments,
    
    // Actions
    loadKnowledgeBases,
    createKnowledgeBase,
    deleteKnowledgeBase,
    setCurrentKb,
    loadDocuments,
    importDocument,
    selectAndImportDocument,
    deleteDocument,
    searchKnowledgeBase,
    updateRetrievalSettings,
    formatFileSize,
    formatDate,
  };
}, {
  persist: {
    paths: ["retrievalSettings", "retrievalSettingsVersion"],
  },
});
