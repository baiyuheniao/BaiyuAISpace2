/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";
import { useKnowledgeBaseStore, type RetrievalResult } from "./knowledgeBase";

export interface Message {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
  streaming?: boolean;
  error?: string;
}

export interface ChatSession {
  id: string;
  title: string;
  messages: Message[];
  createdAt: number;
  updatedAt: number;
  apiConfigId: string;
  provider: string;
  model: string;
}

interface StreamChunk {
  session_id: string;
  message_id: string;
  content: string;
  done: boolean;
}

interface DbMessage {
  id: string;
  role: string;
  content: string;
  timestamp: number;
  error?: string;
}

interface DbSession {
  id: string;
  title: string;
  provider: string;
  model: string;
  created_at: number;
  updated_at: number;
  messages: DbMessage[];
}

export const useChatStore = defineStore("chat", () => {
  const settings = useSettingsStore();
  const kbStore = useKnowledgeBaseStore();
  
  const currentSession = ref<ChatSession | null>(null);
  const isLoading = ref(false);
  const currentStreamContent = ref("");
  const sessions = ref<ChatSession[]>([]);
  let unlistenFn: UnlistenFn | null = null;
  
  const ragEnabled = ref(false);
  const selectedKnowledgeBaseId = ref<string | null>(null);
  const lastRetrievalResult = ref<RetrievalResult | null>(null);

  const loadSessionsFromDb = async () => {
    try {
      const dbSessions = await invoke<DbSession[]>("get_sessions_cmd");
      sessions.value = dbSessions.map(s => ({
        id: s.id,
        title: s.title,
        provider: s.provider,
        model: s.model,
        createdAt: s.created_at,
        updatedAt: s.updated_at,
        messages: s.messages.map(m => ({
          id: m.id,
          role: m.role as "user" | "assistant" | "system",
          content: m.content,
          timestamp: m.timestamp,
          error: m.error,
        })),
        apiConfigId: s.id,
      }));
    } catch (error) {
      console.error("Failed to load sessions:", error);
    }
  };

  const setupStreamListener = async () => {
    if (unlistenFn) {
      unlistenFn();
    }
    
    unlistenFn = await listen<StreamChunk>("stream-chunk", async (event) => {
      const chunk = event.payload;
      
      if (!currentSession.value) return;
      if (chunk.session_id !== currentSession.value.id) return;

      const lastMessage = currentSession.value.messages[currentSession.value.messages.length - 1];
      if (!lastMessage || lastMessage.role !== "assistant") return;

      if (chunk.done) {
        lastMessage.streaming = false;
        isLoading.value = false;
        currentStreamContent.value = "";
        
        await saveMessageToDb(lastMessage);
        await saveSessionToDb();
      } else {
        lastMessage.content += chunk.content;
        currentStreamContent.value = lastMessage.content;
      }
    });
  };

  const saveSessionToDb = async () => {
    if (!currentSession.value) return;
    
    try {
      const dbSession: DbSession = {
        id: currentSession.value.id,
        title: currentSession.value.title,
        provider: currentSession.value.provider,
        model: currentSession.value.model,
        created_at: currentSession.value.createdAt,
        updated_at: Date.now(),
        messages: [],
      };
      await invoke("save_session_cmd", { session: dbSession });
    } catch (error) {
      console.error("Failed to save session:", error);
    }
  };

  const saveMessageToDb = async (message: Message) => {
    if (!currentSession.value) return;
    
    try {
      const dbMessage: DbMessage = {
        id: message.id,
        role: message.role,
        content: message.content,
        timestamp: message.timestamp,
        error: message.error,
      };
      await invoke("save_message_cmd", { 
        sessionId: currentSession.value.id, 
        message: dbMessage 
      });
    } catch (error) {
      console.error("Failed to save message:", error);
    }
  };

  const createSession = async (apiConfigId: string): Promise<ChatSession | null> => {
    const config = settings.apiConfigs.find(c => c.id === apiConfigId);
    if (!config) {
      console.error("API config not found:", apiConfigId);
      return null;
    }

    const session: ChatSession = {
      id: crypto.randomUUID(),
      title: "新对话",
      messages: [],
      createdAt: Date.now(),
      updatedAt: Date.now(),
      apiConfigId,
      provider: config.provider,
      model: config.model,
    };
    currentSession.value = session;
    await setupStreamListener();
    await saveSessionToDb();
    await loadSessionsFromDb();
    return session;
  };

  const loadSession = async (session: ChatSession) => {
    currentSession.value = session;
    await setupStreamListener();
  };

  const sendMessage = async (content: string) => {
    if (!currentSession.value) return;

    const config = settings.apiConfigs.find(c => c.id === currentSession.value!.apiConfigId);
    if (!config) {
      console.error("API config not found for session");
      return;
    }

    let enhancedContent = content;
    let retrievalContext = "";

    if (ragEnabled.value && selectedKnowledgeBaseId.value) {
      const kb = kbStore.knowledgeBases.find(k => k.id === selectedKnowledgeBaseId.value);
      if (kb) {
        const kbConfig = settings.apiConfigs.find(c => c.provider === kb.embedding_provider);
        if (kbConfig?.apiKey) {
          const result = await kbStore.searchKnowledgeBase(
            selectedKnowledgeBaseId.value,
            content,
            kbConfig.apiKey
          );
          
          if (result && result.chunks.length > 0) {
            lastRetrievalResult.value = result;
            retrievalContext = buildRagContext(result);
            enhancedContent = `${retrievalContext}\n\n问题：${content}`;
          }
        }
      }
    }

    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      timestamp: Date.now(),
    };

    currentSession.value.messages.push(userMessage);
    currentSession.value.updatedAt = Date.now();
    isLoading.value = true;
    currentStreamContent.value = "";

    await saveMessageToDb(userMessage);
    await saveSessionToDb();

    try {
      const assistantMessage: Message = {
        id: crypto.randomUUID(),
        role: "assistant",
        content: "",
        timestamp: Date.now(),
        streaming: true,
      };
      currentSession.value.messages.push(assistantMessage);

      const apiMessages = currentSession.value.messages
        .filter(m => !m.streaming && !m.error)
        .map((m, index) => {
          if (ragEnabled.value && index === currentSession.value!.messages.length - 2) {
            return {
              id: m.id,
              role: m.role,
              content: enhancedContent,
              timestamp: m.timestamp,
              error: m.error,
            };
          }
          return {
            id: m.id,
            role: m.role,
            content: m.content,
            timestamp: m.timestamp,
            error: m.error,
          };
        });

      await invoke("stream_message", {
        request: {
          sessionId: currentSession.value.id,
          messages: apiMessages,
          provider: config.provider,
          model: config.model,
          apiKey: config.apiKey,
        },
      });

      if (currentSession.value.messages.length === 2) {
        currentSession.value.title = content.slice(0, 30) + (content.length > 30 ? "..." : "");
        await saveSessionToDb();
        await loadSessionsFromDb();
      }
    } catch (error) {
      const lastMessage = currentSession.value.messages[currentSession.value.messages.length - 1];
      if (lastMessage.role === "assistant") {
        lastMessage.error = String(error);
        lastMessage.streaming = false;
        await saveMessageToDb(lastMessage);
      }
      isLoading.value = false;
      currentStreamContent.value = "";
    }
  };

  const buildRagContext = (result: RetrievalResult): string => {
    if (result.chunks.length === 0) return "";
    
    const contextParts = ["基于以下参考文档回答问题："];
    
    result.chunks.forEach((chunk, index) => {
      contextParts.push(`\n[文档 ${index + 1}: ${chunk.document_filename}]\n${chunk.chunk.content}`);
    });
    
    contextParts.push("\n---");
    return contextParts.join("\n");
  };

  const toggleRag = (enabled: boolean) => {
    ragEnabled.value = enabled;
    if (!enabled) {
      selectedKnowledgeBaseId.value = null;
      lastRetrievalResult.value = null;
    }
  };

  const selectKnowledgeBaseForRag = (kbId: string | null) => {
    selectedKnowledgeBaseId.value = kbId;
  };

  const deleteSession = async (sessionId: string) => {
    try {
      await invoke("delete_session_cmd", { sessionId });
      if (currentSession.value?.id === sessionId) {
        currentSession.value = null;
      }
      await loadSessionsFromDb();
    } catch (error) {
      console.error("Failed to delete session:", error);
    }
  };

  const clearSession = () => {
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
    currentSession.value = null;
    currentStreamContent.value = "";
  };

  return {
    currentSession,
    sessions,
    isLoading,
    currentStreamContent,
    ragEnabled,
    selectedKnowledgeBaseId,
    lastRetrievalResult,
    createSession,
    loadSession,
    sendMessage,
    deleteSession,
    clearSession,
    loadSessionsFromDb,
    toggleRag,
    selectKnowledgeBaseForRag,
  };
});
