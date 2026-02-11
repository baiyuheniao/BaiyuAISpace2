/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";

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
  provider: string;
  model: string;
}

interface StreamChunk {
  session_id: string;
  message_id: string;
  content: string;
  done: boolean;
}

// Database interfaces
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
  
  // Current session
  const currentSession = ref<ChatSession | null>(null);
  const isLoading = ref(false);
  const currentStreamContent = ref("");
  const sessions = ref<ChatSession[]>([]);
  let unlistenFn: UnlistenFn | null = null;

  // Load all sessions from database
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
      }));
    } catch (error) {
      console.error("Failed to load sessions:", error);
    }
  };

  // Setup stream listener
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
        
        // Save the completed message to database
        await saveMessageToDb(lastMessage);
        await saveSessionToDb();
      } else {
        lastMessage.content += chunk.content;
        currentStreamContent.value = lastMessage.content;
      }
    });
  };

  // Save session to database
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

  // Save message to database
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

  // Create new session
  const createSession = async (provider: string, model: string): Promise<ChatSession> => {
    const session: ChatSession = {
      id: crypto.randomUUID(),
      title: "新对话",
      messages: [],
      createdAt: Date.now(),
      updatedAt: Date.now(),
      provider,
      model,
    };
    currentSession.value = session;
    await setupStreamListener();
    await saveSessionToDb();
    await loadSessionsFromDb();
    return session;
  };

  // Load session
  const loadSession = async (session: ChatSession) => {
    currentSession.value = session;
    await setupStreamListener();
  };

  // Send message with streaming
  const sendMessage = async (content: string) => {
    if (!currentSession.value) return;

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

    // Save user message immediately
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

      // Get API key from settings
      const providerConfig = settings.providers.find(
        p => p.id === currentSession.value!.provider
      );
      const apiKey = providerConfig?.apiKey || "";

      // Call Rust backend with streaming
      await invoke("stream_message", {
        request: {
          sessionId: currentSession.value.id,
          messages: currentSession.value.messages
            .filter(m => !m.streaming && !m.error)
            .map((m) => ({
              id: m.id,
              role: m.role,
              content: m.content,
              timestamp: m.timestamp,
              error: m.error,
            })),
          provider: currentSession.value.provider,
          model: currentSession.value.model,
          apiKey: apiKey,
        },
      });

      // Update title if first message
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

  // Delete session
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

  // Clear current session
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
    createSession,
    loadSession,
    sendMessage,
    deleteSession,
    clearSession,
    loadSessionsFromDb,
  };
});
