/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

// Types matching Rust backend
export interface KnowledgeBase {
  id: string;
  name: string;
  description: string;
  embedding_api_config_id: string; // Reference to EmbeddingApiConfig in settings
  chunk_size: number;
  chunk_overlap: number;
  created_at: number;
  updated_at: number;
  document_count: number;
}

export interface Document {
  id: string;
  kb_id: string;
  filename: string;
  file_type: string;
  file_size: number;
  file_hash: string;
  content_preview: string;
  chunk_count: number;
  status: "processing" | "completed" | "error";
  error_message?: string;
  created_at: number;
}

export interface Chunk {
  id: string;
  document_id: string;
  kb_id: string;
  content: string;
  chunk_index: number;
  token_count: number;
}

export interface RetrievedChunk {
  chunk: Chunk;
  score: number;
  vector_score?: number;
  keyword_score?: number;
  document_filename: string;
}

export interface RetrievalResult {
  query: string;
  chunks: RetrievedChunk[];
  total_chunks: number;
}

export type RetrievalMode = "vector" | "keyword" | "hybrid";

export interface CreateKnowledgeBaseRequest {
  name: string;
  description: string;
  embedding_api_config_id: string;
  chunk_size?: number;
  chunk_overlap?: number;
}

export interface RetrievalSettings {
  mode: RetrievalMode;
  topK: number;
  similarityThreshold: number;
}

export const useKnowledgeBaseStore = defineStore("knowledgeBase", () => {
  // State
  const knowledgeBases = ref<KnowledgeBase[]>([]);
  const currentKb = ref<KnowledgeBase | null>(null);
  const documents = ref<Document[]>([]);
  const loading = ref(false);
  const importProgress = ref<{ current: number; total: number } | null>(null);
  
  // Retrieval settings
  const retrievalSettings = ref<RetrievalSettings>({
    mode: "hybrid",
    topK: 5,
    similarityThreshold: 0.7,
  });

  // Getters
  const currentKbDocuments = computed(() => {
    if (!currentKb.value) return [];
    return documents.value.filter((d) => d.kb_id === currentKb.value!.id);
  });

  // Actions
  const loadKnowledgeBases = async () => {
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
      const result = await invoke<KnowledgeBase>("create_knowledge_base", {
        request,
      });
      knowledgeBases.value.unshift(result);
      return result;
    } catch (error) {
      console.error("Failed to create knowledge base:", error);
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

  const importDocument = async (
    kbId: string,
    filePath: string,
    embeddingProvider: string,
    embeddingModel: string,
    apiKey: string
  ): Promise<boolean> => {
    try {
      await invoke("import_document", {
        kbId,
        filePath,
        embeddingProvider,
        embeddingModel,
        apiKey,
      });
      await loadDocuments(kbId);
      await loadKnowledgeBases(); // Refresh document count
      return true;
    } catch (error) {
      console.error("Failed to import document:", error);
      return false;
    }
  };

  const selectAndImportDocument = async (
    kbId: string,
    embeddingProvider: string,
    embeddingModel: string,
    apiKey: string
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
        return await importDocument(kbId, selected, embeddingProvider, embeddingModel, apiKey);
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

  const searchKnowledgeBase = async (
    kbId: string,
    query: string,
    embeddingProvider: string,
    embeddingModel: string,
    apiKey: string
  ): Promise<RetrievalResult | null> => {
    try {
      const result = await invoke<RetrievalResult>("search_knowledge_base", {
        request: {
          kbId,
          query,
          topK: retrievalSettings.value.topK,
          retrievalMode: retrievalSettings.value.mode,
          similarityThreshold: retrievalSettings.value.similarityThreshold,
        },
        embeddingProvider,
        embeddingModel,
        apiKey,
      });
      return result;
    } catch (error) {
      console.error("Failed to search knowledge base:", error);
      return null;
    }
  };

  const getEmbeddingModels = async (): Promise<
    Array<{ provider: string; model: string; dim: number }>
  > => {
    try {
      const result = await invoke<[string, string, number][]>("get_embedding_models");
      return result.map(([provider, model, dim]) => ({ provider, model, dim }));
    } catch (error) {
      console.error("Failed to get embedding models:", error);
      return [];
    }
  };

  const updateRetrievalSettings = (settings: Partial<RetrievalSettings>) => {
    retrievalSettings.value = { ...retrievalSettings.value, ...settings };
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
    getEmbeddingModels,
    updateRetrievalSettings,
    formatFileSize,
    formatDate,
  };
});
