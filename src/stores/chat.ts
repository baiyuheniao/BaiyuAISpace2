/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * BaiyuAISpace 聊天模块
 * 负责管理聊天会话、消息发送、LLM API 调用、流式响应处理等功能
 */

import { ref } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";
import { useKnowledgeBaseStore, type RetrievalResult } from "./knowledgeBase";
import { useMCPStore, type MCPTool } from "./mcp";

/**
 * 前端消息类型
 * 用于在 UI 层表示聊天消息
 */
export interface Message {
  id: string;                    // 消息唯一标识符 (UUID)
  role: "user" | "assistant" | "system";  // 消息角色: 用户/助手/系统
  content: string;               // 消息内容
  timestamp: number;              // 时间戳 (毫秒)
  streaming?: boolean;            // 是否正在流式输出
  error?: string;                 // 错误信息 (如果有)
  files?: Array<{                // 附件文件列表
    name: string;                 // 文件名
    size: number;                 // 文件大小 (字节)
  }>;
}

/**
 * 前端会话类型
 * 表示一个完整的聊天会话
 */
export interface ChatSession {
  id: string;                    // 会话唯一标识符
  title: string;                 // 会话标题
  messages: Message[];            // 消息列表
  createdAt: number;              // 创建时间 (毫秒)
  updatedAt: number;              // 最后更新时间 (毫秒)
  apiConfigId: string;           // 关联的 API 配置 ID
  provider: string;               // LLM 提供商 (如 openai, anthropic)
  model: string;                  // 模型名称 (如 gpt-4, claude-3)
}

/**
 * 流式响应块类型
 * 从后端接收的 SSE 事件数据结构
 */
interface StreamChunk {
  session_id: string;             // 所属会话 ID
  message_id: string;             // 消息 ID
  content: string;                // 增量内容
  done: boolean;                  // 是否完成
}

/**
 * 数据库消息类型
 * 与后端数据库结构对应的消息类型 (snake_case 命名)
 */
interface DbMessage {
  id: string;
  role: string;
  content: string;
  timestamp: number;
  error?: string;
}

/**
 * 数据库会话类型
 * 与后端数据库结构对应的会话类型 (snake_case 命名)
 */
interface DbSession {
  id: string;
  title: string;
  provider: string;
  model: string;
  api_config_id: string;           // API 配置 ID (数据库字段)
  created_at: number;
  updated_at: number;
  messages: DbMessage[];
}

/**
 * 聊天 Store
 * 使用 Pinia 管理聊天状态和业务逻辑
 */
export const useChatStore = defineStore("chat", () => {
  // 引用其他 Store
  const settings = useSettingsStore();      // 设置 Store (API 配置)
  const kbStore = useKnowledgeBaseStore();  // 知识库 Store

  // ============ 响应式状态 ============
  
  /** 当前活动的会话 */
  const currentSession = ref<ChatSession | null>(null);
  
  /** 是否正在加载/生成回复 */
  const isLoading = ref(false);
  
  /** 当前流式输出的完整内容 */
  const currentStreamContent = ref("");
  
  /** 会话列表 (侧边栏显示) */
  const sessions = ref<ChatSession[]>([]);
  
  /** SSE 事件监听器取消函数 */
  let unlistenFn: UnlistenFn | null = null;
  
  /** RAG (检索增强生成) 是否启用 */
  const ragEnabled = ref(false);
  
  /** 当前选中的知识库 ID */
  const selectedKnowledgeBaseId = ref<string | null>(null);
  
  /** 上一次检索结果 */
  const lastRetrievalResult = ref<RetrievalResult | null>(null);
  
  /** MCP (Model Context Protocol) 是否启用 */
  const mcpEnabled = ref(false);

  // ============ 会话管理函数 ============

  /**
   * 从数据库加载所有会话
   * 调用后端 get_sessions_cmd 获取会话列表
   * 
   * @returns void
   */
  const loadSessionsFromDb = async () => {
    try {
      // 从后端获取会话列表
      const dbSessions = await invoke<DbSession[]>("get_sessions_cmd");
      
      // 转换为前端格式 (snake_case -> camelCase)
      sessions.value = dbSessions.map(s => ({
        id: s.id,
        title: s.title,
        provider: s.provider,
        model: s.model,
        // 如果 api_config_id 为空，使用会话 ID 作为后备 (兼容旧数据)
        apiConfigId: s.api_config_id || s.id,
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

  /**
   * 设置流式响应监听器
   * 监听后端发送的 stream-chunk 事件
   * 
   * @returns void
   */
  const setupStreamListener = async () => {
    // 如果已有监听器，先取消
    if (unlistenFn) {
      unlistenFn();
    }
    
    // 监听 SSE 流式事件
    unlistenFn = await listen<StreamChunk>("stream-chunk", async (event) => {
      const chunk = event.payload;
      
      // 如果没有当前会话或会话 ID 不匹配，忽略
      if (!currentSession.value) return;
      if (chunk.session_id !== currentSession.value.id) return;

      // 如果用户已停止，不再处理后续内容
      if (!isLoading.value && !chunk.done) return;

      // 获取最后一条消息
      const lastMessage = currentSession.value.messages[currentSession.value.messages.length - 1];
      if (!lastMessage || lastMessage.role !== "assistant") return;

      // 处理流式响应完成
      if (chunk.done) {
        lastMessage.streaming = false;
        isLoading.value = false;
        currentStreamContent.value = "";
        
        // 保存到数据库
        await saveMessageToDb(lastMessage);
        await saveSessionToDb();
      } else {
        // 累加内容 (打字机效果)
        lastMessage.content += chunk.content;
        currentStreamContent.value = lastMessage.content;
      }
    });
  };

  /**
   * 保存当前会话到数据库
   * 包含会话基本信息，不包含消息内容
   * 
   * @returns void
   */
  const saveSessionToDb = async () => {
    if (!currentSession.value) return;
    
    try {
      const dbSession: DbSession = {
        id: currentSession.value.id,
        title: currentSession.value.title,
        provider: currentSession.value.provider,
        model: currentSession.value.model,
        api_config_id: currentSession.value.apiConfigId,  // 保存 API 配置关联
        created_at: currentSession.value.createdAt,
        updated_at: Date.now(),
        messages: [],
      };
      await invoke("save_session_cmd", { session: dbSession });
    } catch (error) {
      console.error("Failed to save session:", error);
    }
  };

  /**
   * 保存单条消息到数据库
   * 
   * @param message - 要保存的消息对象
   * @returns void
   */
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

  /**
   * 创建新会话
   * 
   * @param apiConfigId - API 配置 ID
   * @returns 新创建的会话对象，失败返回 null
   */
  const createSession = async (apiConfigId: string): Promise<ChatSession | null> => {
    // 查找对应的 API 配置
    const config = settings.apiConfigs.find(c => c.id === apiConfigId);
    if (!config) {
      console.error("API config not found:", apiConfigId);
      return null;
    }

    // 构建新会话对象
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
    
    // 设置为当前会话
    currentSession.value = session;
    
    // 设置流式监听、保存并刷新列表
    await setupStreamListener();
    await saveSessionToDb();
    await loadSessionsFromDb();
    
    return session;
  };

  /**
   * 加载已有会话
   * 
   * @param session - 要加载的会话对象
   * @returns void
   */
  const loadSession = async (session: ChatSession) => {
    currentSession.value = session;
    await setupStreamListener();
  };

  /**
   * 错误类型分类
   * 根据错误信息返回用户友好的错误类型和消息
   * 
   * @param error - 原始错误对象
   * @returns 包含错误类型和用户友好消息的对象
   */
  const classifyError = (error: unknown): { type: string; message: string } => {
    const errorStr = String(error);
    
    // 认证错误
    if (errorStr.includes("API key") || errorStr.includes("Unauthorized") || errorStr.includes("401")) {
      return { type: "auth", message: "API 密钥无效或已过期，请检查设置" };
    } 
    // 网络错误
    else if (errorStr.includes("network") || errorStr.includes("Failed to fetch")) {
      return { type: "network", message: "网络连接错误，请检查网络设置" };
    } 
    // 超时错误
    else if (errorStr.includes("timeout")) {
      return { type: "timeout", message: "请求超时，请重试或调整超时设置" };
    } 
    // 配置错误
    else if (errorStr.includes("provider") || errorStr.includes("Invalid")) {
      return { type: "config", message: "API 配置错误，请检查服务商和模型" };
    } 
    // 未知错误
    else {
      return { type: "unknown", message: `错误: ${errorStr}` };
    }
  };

  /**
   * 执行 MCP 工具调用
   * 
   * @param toolName - 工具名称
   * @param toolInput - 工具输入参数
   * @returns 工具执行结果 (字符串格式)
   */
  const executeMcpTool = async (toolName: string, toolInput: Record<string, unknown>): Promise<string> => {
    const mcp = useMCPStore();
    const tool = mcp.availableTools.find(t => t.name === toolName);
    
    if (!tool) {
      return `错误: 工具 "${toolName}" 不存在`;
    }

    try {
      // 调用后端 MCP 命令执行工具
      const result = await invoke<any>("call_mcp_tool", {
        tool_name: toolName,
        input: toolInput,
      });

      // 检查返回结果中的 success 标志
      if (typeof result === 'object' && result !== null) {
        const resultObj = result as Record<string, unknown>;
        const anyRes = result as any;
        
        // 处理错误标志
        if ('success' in resultObj) {
          if (anyRes.success === false && anyRes.error) {
            return `工具执行失败: ${anyRes.error}`;
          }
        }

        // 提取结果字段
        if ('result' in resultObj) {
          return JSON.stringify(anyRes.result, null, 2);
        }
        
        // 如果没有特定结果字段，返回整个响应
        return JSON.stringify(anyRes, null, 2);
      } else {
        return String(result);
      }
    } catch (err) {
      return `调用工具时出错: ${String(err)}`;
    }
  };

  /**
   * 处理助手消息中的 MCP 工具调用
   * 从响应中解析工具调用指令并执行
   * 
   * @param assistantMessage - 助手消息对象
   * @returns void
   */
  const handleMcpCalls = async (assistantMessage: Message): Promise<void> => {
    const mcp = useMCPStore();
    // 如果 MCP 未启用或没有可用工具，直接返回
    if (!mcpEnabled.value || mcp.availableTools.length === 0) return;

    // 简单的模式匹配来检测工具调用
    // 格式: [使用工具: tool_name with input: {...}]
    const toolCallPattern = /\[使用工具: ([\w_-]+) with input: ({[^}]+})\]/g;
    const matches = Array.from(assistantMessage.content.matchAll(toolCallPattern));

    if (matches.length === 0) return; // 未检测到工具调用

    // 执行每个工具调用并收集结果
    const toolResults: string[] = [];
    for (const match of matches) {
      const toolName = match[1];
      try {
        const toolInput = JSON.parse(match[2]) as Record<string, unknown>;
        const result = await executeMcpTool(toolName, toolInput);
        toolResults.push(`[工具 ${toolName} 结果]: ${result}`);
      } catch (err) {
        toolResults.push(`[工具 ${toolName} 错误]: ${String(err)}`);
      }
    }

    // 如果执行了工具，将结果追加到助手消息
    if (toolResults.length > 0) {
      assistantMessage.content += "\n\n" + toolResults.join("\n");
    }
  };

  /**
   * 发送消息 (核心函数)
   * 处理用户消息发送、LLM 调用、流式响应等完整流程
   * 
   * @param content - 消息内容
   * @param attachedFiles - 附件文件列表 (可选)
   * @returns void
   */
  const sendMessage = async (content: string, attachedFiles?: Array<{ name: string; size: number }>) => {
    // 检查是否有当前会话
    if (!currentSession.value) return;

    // 查找当前会话关联的 API 配置
    const config = settings.apiConfigs.find(c => c.id === currentSession.value!.apiConfigId);
    if (!config) {
      console.error("API config not found for session");
      alert("未找到 API 配置，请检查设置");
      return;
    }

    // 检查 API 密钥是否已加载
    if (!config.apiKey) {
      console.error("API key not loaded for config:", config.id);
      alert("API 密钥未加载，请重启应用或重新设置");
      return;
    }

    const mcp = useMCPStore();

    // 初始化内容变量
    let enhancedContent = content;
    let retrievalContext = "";

    // ============ RAG 检索增强 ============
    if (ragEnabled.value && selectedKnowledgeBaseId.value) {
      const kb = kbStore.knowledgeBases.find(k => k.id === selectedKnowledgeBaseId.value);
      if (kb) {
        const embeddingConfig = settings.embeddingApiConfigs.find(c => c.id === kb.embedding_api_config_id);
        if (embeddingConfig?.apiKey) {
          // 执行知识库检索
          const result = await kbStore.searchKnowledgeBase(
            selectedKnowledgeBaseId.value,
            content,
            embeddingConfig.provider,
            embeddingConfig.model,
            embeddingConfig.apiKey
          );
          
          // 如果检索到相关内容，构建增强上下文
          if (result && result.chunks.length > 0) {
            lastRetrievalResult.value = result;
            retrievalContext = buildRagContext(result);
            enhancedContent = `${retrievalContext}\n\n问题：${content}`;
          }
        }
      }
    }

    // 构建用户消息对象
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      timestamp: Date.now(),
      files: attachedFiles && attachedFiles.length > 0 ? attachedFiles : undefined,
    };

    // 添加到当前会话
    currentSession.value.messages.push(userMessage);
    currentSession.value.updatedAt = Date.now();
    isLoading.value = true;
    currentStreamContent.value = "";

    // 保存到数据库
    await saveMessageToDb(userMessage);
    await saveSessionToDb();

    try {
      // 创建助手消息占位
      const assistantMessage: Message = {
        id: crypto.randomUUID(),
        role: "assistant",
        content: "",
        timestamp: Date.now(),
        streaming: true,
      };
      currentSession.value.messages.push(assistantMessage);

      // ============ 构建 API 消息列表 ============
      let apiMessages = currentSession.value.messages
        // 过滤掉流式中和有错误的消息
        .filter(m => !m.streaming && !m.error)
        .map((m, index) => {
          // 如果启用了 RAG，在最后一条用户消息中添加检索上下文
          if (ragEnabled.value && index === currentSession.value!.messages.length - 2) {
            return {
              id: m.id,
              role: m.role,
              content: enhancedContent,  // 使用增强后的内容
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

      // ============ MCP 系统提示 ============
      if (mcpEnabled.value && mcp.availableTools.length > 0) {
        const mcpSystemPrompt = buildMcpSystemPrompt(mcp.availableTools);
        
        // 如果已有系统消息，追加 MCP 信息
        if (apiMessages.length > 0 && apiMessages[0].role === "system") {
          apiMessages[0] = {
            ...apiMessages[0],
            content: apiMessages[0].content + "\n\n" + mcpSystemPrompt,
          };
        } else {
          // 在开头插入新的系统消息
          apiMessages.unshift({
            id: crypto.randomUUID(),
            role: "system",
            content: mcpSystemPrompt,
            timestamp: Date.now(),
            error: undefined,
          });
        }
      }

      // ============ 构建请求 payload ============
      const requestPayload = {
        sessionId: currentSession.value.id,
        messages: apiMessages,
        provider: config.provider,
        model: config.model,
        apiKey: config.apiKey,
        baseUrl: config.baseUrl,
        enableMcp: mcpEnabled.value,
      } as const;

      // 开发模式下打印调试日志 (隐藏 API 密钥)
      const maskedApiKey = requestPayload.apiKey
        ? String(requestPayload.apiKey).replace(/.(?=.{4})/g, '*')
        : null;

      if (import.meta.env.DEV) {
        console.debug('STREAM_REQUEST (masked):', {
        sessionId: requestPayload.sessionId,
        provider: requestPayload.provider,
        model: requestPayload.model,
        baseUrl: requestPayload.baseUrl,
        enableMcp: requestPayload.enableMcp,
        apiKey: maskedApiKey,
        messagesCount: requestPayload.messages?.length ?? 0,
        });
      }

      // ============ 调用后端流式消息 API ============
      try {
        await invoke('stream_message', { request: requestPayload });
      } catch (e) {
        if (import.meta.env.DEV) console.error('stream_message error', e);
        throw e;
      }

      // ============ 处理 MCP 工具调用 ============
      const assistantMsgRef = currentSession.value.messages[currentSession.value.messages.length - 1];
      if (assistantMsgRef.role === "assistant") {
        await handleMcpCalls(assistantMsgRef);
      }

      // ============ 更新会话标题 ============
      // 如果是第一条对话 (用户消息 + 助手回复)，更新标题
      if (currentSession.value.messages.length === 2) {
        currentSession.value.title = content.slice(0, 30) + (content.length > 30 ? "..." : "");
        await saveSessionToDb();
        await loadSessionsFromDb();
      }
    } catch (error) {
      // ============ 错误处理 ============
      const errorInfo = classifyError(error);
      const lastMessage = currentSession.value.messages[currentSession.value.messages.length - 1];
      
      // 将错误信息保存到消息中
      if (lastMessage.role === "assistant") {
        lastMessage.error = errorInfo.message;
        lastMessage.streaming = false;
        await saveMessageToDb(lastMessage);
      }
      
      console.error(`[${errorInfo.type}] ${error}`);
      isLoading.value = false;
      currentStreamContent.value = "";
    }
  };

  /**
   * 构建 RAG 上下文
   * 将检索到的文档片段格式化为提示上下文
   * 
   * @param result - 检索结果
   * @returns 格式化的上下文字符串
   */
  const buildRagContext = (result: RetrievalResult): string => {
    if (result.chunks.length === 0) return "";
    
    const contextParts = ["基于以下参考文档回答问题："];
    
    result.chunks.forEach((chunk, index) => {
      contextParts.push(`\n[文档 ${index + 1}: ${chunk.document_filename}]\n${chunk.chunk.content}`);
    });
    
    contextParts.push("\n---");
    return contextParts.join("\n");
  };

  /**
   * 构建 MCP 工具定义
   * 将可用工具格式化为 JSON 字符串，用于系统提示
   * 
   * @param availableTools - 可用的 MCP 工具列表
   * @returns 格式化的工具定义字符串
   */
  const buildMcpToolDefinitions = (availableTools: MCPTool[]): string => {
    if (availableTools.length === 0) return "";

    const toolsJson = availableTools.map((tool) => ({
      type: "function",
      function: {
        name: tool.name,
        description: tool.description,
        parameters: tool.input_schema,
      },
    }));

    const toolDefString = JSON.stringify(toolsJson, null, 2);
    
    return `## 可用工具

你可以使用以下工具来完成任务。每个工具都有特定的功能和参数要求。

\`\`\`json
${toolDefString}
\`\`\`

使用工具时，你可以调用它们来获取信息或执行操作。`;
  };

  /**
   * 构建 MCP 系统提示
   * 包含工具定义和如何使用工具的说明
   * 
   * @param availableTools - 可用的 MCP 工具列表
   * @returns 系统提示字符串
   */
  const buildMcpSystemPrompt = (availableTools: MCPTool[]): string => {
    const toolDefs = buildMcpToolDefinitions(availableTools);
    
    return `你是一个有能力的AI助手，可以使用各种工具来帮助用户完成任务。

${toolDefs}

当你需要使用工具时，请清楚地说明你打算使用哪个工具以及为什么。你可以组合多个工具来解决更复杂的问题。`;
  };

  /**
   * 切换 RAG 开关状态
   * 
   * @param enabled - 是否启用 RAG
   * @returns void
   */
  const toggleRag = (enabled: boolean) => {
    ragEnabled.value = enabled;
    // 如果关闭 RAG，清除相关状态
    if (!enabled) {
      selectedKnowledgeBaseId.value = null;
      lastRetrievalResult.value = null;
    }
  };

  /**
   * 选择知识库用于 RAG
   * 
   * @param kbId - 知识库 ID，null 表示取消选择
   * @returns void
   */
  const selectKnowledgeBaseForRag = (kbId: string | null) => {
    selectedKnowledgeBaseId.value = kbId;
  };

  /**
   * 删除会话
   * 
   * @param sessionId - 要删除的会话 ID
   * @returns void
   */
  const deleteSession = async (sessionId: string) => {
    try {
      await invoke("delete_session_cmd", { sessionId });
      // 如果删除的是当前会话，清空当前会话
      if (currentSession.value?.id === sessionId) {
        currentSession.value = null;
      }
      // 刷新会话列表
      await loadSessionsFromDb();
    } catch (error) {
      console.error("Failed to delete session:", error);
    }
  };

  /**
   * 清除当前会话
   * 取消事件监听器，清空当前会话状态
   * 
   * @returns void
   */
  const clearSession = () => {
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
    currentSession.value = null;
    currentStreamContent.value = "";
  };

  // ============ 流式中断功能 ============
  const stopStream = () => {
    if (isLoading.value) {
      isLoading.value = false;
      const lastMessage = currentSession.value.messages[currentSession.value.messages.length - 1];
      if (lastMessage && lastMessage.role === "assistant") {
        lastMessage.streaming = false;
      }
      currentStreamContent.value = "";
      console.log("[Stream] Stopped by user");
    }
  };

  // ============ 返回公共接口 ============
  return {
    // 状态
    currentSession,
    sessions,
    isLoading,
    currentStreamContent,
    ragEnabled,
    selectedKnowledgeBaseId,
    lastRetrievalResult,
    mcpEnabled,
    
    // 方法
    createSession,           // 创建新会话
    loadSession,             // 加载会话
    sendMessage,             // 发送消息
    deleteSession,           // 删除会话
    clearSession,            // 清除当前会话
    loadSessionsFromDb,      // 加载会话列表
    toggleRag,               // 切换 RAG
    selectKnowledgeBaseForRag,  // 选择知识库
    classifyError,           // 错误分类
    executeMcpTool,         // 执行 MCP 工具
    stopStream,              // 停止流式输出
  };
});
