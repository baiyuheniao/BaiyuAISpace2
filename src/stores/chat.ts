/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * BaiyuAISpace 聊天模块
 * 负责管理聊天会话、消息发送、LLM API 调用、流式响应处理等功能
 */

import { computed, reactive, ref } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSettingsStore } from "./settings";
import { useKnowledgeBaseStore, type RetrievalResult } from "./knowledgeBase";
import { classifyError } from "@/utils/errorMessage";

/** 图片附件（base64 编码，不含 data URL 前缀） */
export interface ImageAttachment {
  data: string; // 原始 base64 字符串
  mediaType: string; // MIME 类型，如 "image/jpeg"
}

/** 视频附件（base64 编码，不含 data URL 前缀，仅 Gemini provider 有效） */
export interface VideoAttachment {
  data: string;
  mediaType: string;
}

/**
 * 前端消息类型
 * 用于在 UI 层表示聊天消息
 */
export interface Message {
  id: string; // 消息唯一标识符 (UUID)
  role: "user" | "assistant" | "system"; // 消息角色: 用户/助手/系统
  content: string; // 消息内容
  timestamp: number; // 时间戳 (毫秒)
  streaming?: boolean; // 是否正在流式输出
  thinking?: string; // 思考过程（思考型模型的 reasoning 增量累积，仅内存态、不入库）
  error?: string; // 错误信息 (如果有)
  files?: Array<{
    // 附件文件列表
    name: string; // 文件名
    size: number; // 文件大小 (字节)
  }>;
  images?: ImageAttachment[]; // 图片附件（已转 base64）
  videos?: VideoAttachment[]; // 视频附件（已转 base64，仅 Gemini）
  toolCalls?: ToolCallInfo[]; // 本轮回复中触发的工具调用（按发生顺序）
  tokenUsage?: TokenUsage; // 服务端返回的本次交互真实 Token 用量
}

export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

/** 单次工具调用的状态信息，用于在消息里展示"正在调用/已完成/失败" */
export interface ToolCallInfo {
  callId: string; // 工具调用 ID
  toolName: string; // 工具名称
  arguments: string; // 调用参数（JSON 字符串）
  status: "calling" | "done" | "error"; // 当前状态
  result?: string; // 调用结果（JSON 字符串，仅 done/error 时有）
}

/**
 * 前端会话类型
 * 表示一个完整的聊天会话
 */
export interface ChatSession {
  workingDirectory: string | null;
  fileAccessMode: "none" | "read" | "write";
  id: string; // 会话唯一标识符
  title: string; // 会话标题
  messages: Message[]; // 消息列表
  createdAt: number; // 创建时间 (毫秒)
  updatedAt: number; // 最后更新时间 (毫秒)
  apiConfigId: string; // 关联的 API 配置 ID
  provider: string; // LLM 提供商 (如 openai, anthropic)
  model: string; // 模型名称 (如 gpt-4, claude-3)
}

/**
 * 流式响应块类型
 * 从后端接收的 SSE 事件数据结构
 */
interface StreamChunk {
  session_id: string; // 所属会话 ID
  message_id: string; // 消息 ID
  content: string; // 增量内容
  is_thinking?: boolean; // 是否思考过程增量（归到 thinking 字段而非正文）
  done: boolean; // 是否完成
}

interface TokenUsageEvent {
  sessionId: string;
  messageId: string;
  usage: TokenUsage;
}

/**
 * 工具调用状态事件类型
 * 从后端接收的 tool-call-status 事件数据结构
 */
interface ToolCallEvent {
  session_id: string; // 所属会话 ID
  message_id: string; // 消息 ID
  call_id: string; // 工具调用 ID
  tool_name: string; // 工具名称
  arguments: string; // 调用参数 (JSON 字符串)
  status: "calling" | "done" | "error"; // 当前状态
  result?: string; // 调用结果 (JSON 字符串)
}

/**
 * 一轮尚未落库的助手回复。后端 message_id 与前端占位消息 ID 不同，
 * 因此在收到首个事件时把二者绑定；之后无论用户切到哪个页面或会话，
 * 都只按 session_id/message_id 更新这一条回复。
 */
interface InFlightReply {
  sessionId: string;
  message: Message;
  backendMessageId?: string;
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
  token_usage?: TokenUsage;
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
  working_directory?: string | null;
  file_access_mode?: "none" | "read" | "write";
  api_config_id: string; // API 配置 ID (数据库字段)
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
  const settings = useSettingsStore(); // 设置 Store (API 配置)
  const kbStore = useKnowledgeBaseStore(); // 知识库 Store

  // ============ 响应式状态 ============

  /** 当前活动的会话 */
  const currentSession = ref<ChatSession | null>(null);

  /** 会话/消息写入数据库失败的一次性提醒队列。store 拿不到 NMessageProvider
   * 上下文，没法直接弹窗，改成让 Layout.vue watch 这个队列后弹出，弹完自行清空。
   * 静默丢弃这类失败会让用户误以为记录已保存，其实压根没写进数据库。 */
  const dbSaveErrorNotices = ref<string[]>([]);
  /** 普通 Chat 的终止性错误统一由全局 Layout 弹出，避免依赖当前页面。 */
  const errorNotices = ref<string[]>([]);
  const warningNotices = ref<string[]>([]);

  /** 所有仍在生成的回复，按会话隔离，支持切换会话后后台完成。 */
  const inFlightReplies = reactive(new Map<string, InFlightReply>());

  /** 当前正在查看的会话是否仍在生成。 */
  const isLoading = computed(
    () => !!currentSession.value && inFlightReplies.has(String(currentSession.value.id))
  );

  /** 当前流式输出的完整内容 */
  const currentStreamContent = ref("");

  /** 会话列表 (侧边栏显示) */
  const sessions = ref<ChatSession[]>([]);

  /** SSE 事件监听器取消函数 */
  let unlistenFn: UnlistenFn | null = null;

  /** 工具调用状态事件监听器取消函数 */
  let unlistenToolCallFn: UnlistenFn | null = null;
  let unlistenTokenUsageFn: UnlistenFn | null = null;

  /** RAG (检索增强生成) 是否启用 */
  const ragEnabled = ref(false);

  /** 当前选中的知识库 ID */
  const selectedKnowledgeBaseId = ref<string | null>(null);

  /** 上一次检索结果 */
  const lastRetrievalResult = ref<RetrievalResult | null>(null);

  /** MCP (Model Context Protocol) 是否启用 */
  const mcpEnabled = ref(false);

  /** 手动激活的 Skill ID 列表 */
  const activeSkillIds = ref<string[]>([]);

  /** 是否允许模型自主判断调用其它已启用的 Skill */
  const skillAutonomyEnabled = ref(false);

  /** 是否启用思考模式 (Extended Thinking) */
  const thinkingEnabled = ref(false);

  const toChatSession = (session: DbSession): ChatSession => ({
    id: session.id,
    title: session.title,
    provider: session.provider,
    model: session.model,
    workingDirectory: session.working_directory || null,
    fileAccessMode: session.file_access_mode || "none",
    apiConfigId: session.api_config_id || session.id,
    createdAt: session.created_at,
    updatedAt: session.updated_at,
    messages: mergeInFlightReply(
      session.id,
      session.messages.map((message) => ({
        id: message.id,
        role: message.role as "user" | "assistant" | "system",
        content: message.content,
        timestamp: message.timestamp,
        error: message.error,
        tokenUsage: message.token_usage,
      }))
    ),
  });

  /** 把尚未保存的占位回复并回数据库快照，避免刷新覆盖。 */
  function mergeInFlightReply(sessionId: string, messages: Message[]): Message[] {
    const reply = inFlightReplies.get(String(sessionId));
    if (!reply || messages.some((message) => message.id === reply.message.id)) return messages;
    return [...messages, reply.message];
  }

  function findInFlightReply(
    sessionId: string,
    backendMessageId: string
  ): InFlightReply | undefined {
    const reply = inFlightReplies.get(String(sessionId));
    if (!reply) return undefined;
    if (reply.backendMessageId && reply.backendMessageId !== backendMessageId) return undefined;
    reply.backendMessageId ??= backendMessageId;
    return reply;
  }

  /** 让当前会话和历史列表均持有同一个进行中消息对象。 */
  function syncInFlightReply(sessionId: string) {
    const reply = inFlightReplies.get(String(sessionId));
    if (!reply) return;
    const merge = (session: ChatSession) => {
      if (!session.messages.some((message) => message.id === reply.message.id)) {
        session.messages.push(reply.message);
      }
    };
    const listedSession = sessions.value.find(
      (session) => String(session.id) === String(sessionId)
    );
    if (listedSession) merge(listedSession);
    if (currentSession.value && String(currentSession.value.id) === String(sessionId)) {
      merge(currentSession.value);
    }
  }

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
      console.log("[Chat] get_sessions_cmd returned:", dbSessions.length, "sessions");

      // 转换为前端格式 (snake_case -> camelCase)
      sessions.value = dbSessions.map(toChatSession);
      console.log(
        "[Chat] sessions.value updated, first session messages:",
        sessions.value[0]?.messages?.length
      );

      // 如果有当前会话，同步更新当前会话的数据。toChatSession 会保留尚未
      // 落库的回复，避免设置页/历史页刷新用旧快照把流式消息覆盖掉。
      if (currentSession.value) {
        const currentId = String(currentSession.value.id);
        const freshCurrent = sessions.value.find((s) => String(s.id) === currentId);
        if (freshCurrent) {
          currentSession.value = { ...freshCurrent };
          console.log(
            "[Chat] Updated currentSession with fresh data, messages:",
            freshCurrent.messages.length
          );
        }
      }
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
      console.log(
        "[Stream] Received chunk, session_id:",
        chunk.session_id,
        "currentSession:",
        currentSession.value?.id,
        "done:",
        chunk.done
      );

      const reply = findInFlightReply(chunk.session_id, chunk.message_id);
      if (!reply) {
        console.log("[Stream] No matching in-flight reply, ignored");
        return;
      }

      // 处理流结束信号
      if (chunk.done) {
        console.log("[Stream] Stream done for session:", chunk.session_id);
        currentStreamContent.value = "";
        reply.message.streaming = false;
        syncInFlightReply(reply.sessionId);
        console.log(
          "[Stream] Saving message to DB:",
          reply.message.id,
          "content length:",
          reply.message.content.length
        );
        inFlightReplies.delete(reply.sessionId);
        await saveMessageToDb(reply.sessionId, reply.message);
        return;
      }

      // 累加内容 (打字机效果)。思考型模型的思考增量单独归到 thinking 字段，
      // 由 ChatMessage.vue 的"思考过程"折叠区展示，不混入正文、也不入库
      if (chunk.is_thinking) {
        reply.message.thinking = (reply.message.thinking ?? "") + chunk.content;
      } else {
        reply.message.content += chunk.content;
        if (String(currentSession.value?.id) === String(reply.sessionId)) {
          currentStreamContent.value = reply.message.content;
        }
      }
      syncInFlightReply(reply.sessionId);
    });
  };

  /**
   * 设置工具调用状态监听器
   * 监听后端发送的 tool-call-status 事件，把调用状态写进当前流式消息的
   * toolCalls 数组，供 ChatMessage.vue 展示"正在调用工具/工具调用结果"
   *
   * @returns void
   */
  const setupToolCallListener = async () => {
    if (unlistenToolCallFn) {
      unlistenToolCallFn();
    }

    unlistenToolCallFn = await listen<ToolCallEvent>("tool-call-status", (event) => {
      const evt = event.payload;
      const reply = findInFlightReply(evt.session_id, evt.message_id);
      if (!reply) return;
      const message = reply.message;

      if (!message.toolCalls) {
        message.toolCalls = [];
      }
      const existing = message.toolCalls.find((tc) => tc.callId === evt.call_id);
      if (existing) {
        existing.status = evt.status;
        existing.result = evt.result;
      } else {
        message.toolCalls.push({
          callId: evt.call_id,
          toolName: evt.tool_name,
          arguments: evt.arguments,
          status: evt.status,
          result: evt.result,
        });
      }
      if (evt.status === "error") {
        const notice = `工具「${evt.tool_name}」执行失败；模型会尝试基于已有结果继续。`;
        if (!warningNotices.value.includes(notice)) warningNotices.value.push(notice);
      }
      syncInFlightReply(reply.sessionId);
    });
  };

  /** 将后端最终 SSE chunk 中的 usage 绑定到本轮 assistant 消息。 */
  const setupTokenUsageListener = async () => {
    if (unlistenTokenUsageFn) unlistenTokenUsageFn();
    unlistenTokenUsageFn = await listen<TokenUsageEvent>("token-usage", (event) => {
      const usageEvent = event.payload;
      const reply = findInFlightReply(usageEvent.sessionId, usageEvent.messageId);
      if (!reply) return;
      reply.message.tokenUsage = usageEvent.usage;
      syncInFlightReply(reply.sessionId);
    });
  };

  /**
   * 保存当前会话到数据库
   * 包含会话基本信息，不包含消息内容
   *
   * @returns void
   */
  const saveSessionToDb = async (session = currentSession.value) => {
    if (!session) return;

    try {
      const dbSession: DbSession = {
        id: session.id,
        title: session.title,
        provider: session.provider,
        model: session.model,
        api_config_id: session.apiConfigId, // 保存 API 配置关联
        working_directory: session.workingDirectory,
        file_access_mode: session.fileAccessMode,
        created_at: session.createdAt,
        updated_at: Date.now(),
        messages: [],
      };
      await invoke("save_session_cmd", { session: dbSession });
    } catch (error) {
      console.error("Failed to save session:", error);
      dbSaveErrorNotices.value.push(`会话保存失败：${classifyError(error).message}`);
    }
  };

  /**
   * 保存单条消息到数据库
   *
   * @param sessionId - 所属会话，不能从 currentSession 推断（用户可能已切换页面）
   * @param message - 要保存的消息对象
   * @returns void
   */
  const saveMessageToDb = async (sessionId: string, message: Message) => {
    try {
      const dbMessage: DbMessage = {
        id: message.id,
        role: message.role,
        content: message.content,
        timestamp: message.timestamp,
        error: message.error,
        token_usage: message.tokenUsage,
      };
      await invoke("save_message_cmd", {
        sessionId,
        message: dbMessage,
      });
    } catch (error) {
      console.error("Failed to save message:", error);
      dbSaveErrorNotices.value.push(`消息保存失败：${classifyError(error).message}`);
    }
  };

  /**
   * 创建新会话
   *
   * @param apiConfigId - API 配置 ID
   * @returns 新创建的会话对象，失败返回 null
   */
  const setWorkingDirectory = async (
    directory: string | null,
    mode: "none" | "read" | "write" = "none"
  ) => {
    if (!currentSession.value) return;
    currentSession.value.workingDirectory = directory;
    currentSession.value.fileAccessMode = directory ? mode : "none";
    await saveSessionToDb();
    const index = sessions.value.findIndex((session) => session.id === currentSession.value?.id);
    if (index >= 0) sessions.value[index] = { ...currentSession.value };
  };

  const createSession = async (apiConfigId: string): Promise<ChatSession | null> => {
    // 查找对应的 API 配置
    const config = settings.apiConfigs.find((c) => c.id === apiConfigId);
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
      workingDirectory: null,
      fileAccessMode: "none",
    };

    // 设置为当前会话
    currentSession.value = session;
    lastRetrievalResult.value = null;

    // 设置流式监听
    // 注意：这里不写库。空会话在发出第一条消息前只存在于内存里，
    // sendMessage() 里已经会在追加第一条用户消息后调用 saveSessionToDb()，
    // 否则每次点"新建对话"都会在历史记录里留下一条"新对话/0条消息"的僵尸记录
    await setupStreamListener();
    await setupToolCallListener();
    await setupTokenUsageListener();

    return session;
  };

  /**
   * 加载已有会话
   *
   * @param session - 要加载的会话对象
   * @returns void
   */
  const loadSession = async (session: ChatSession) => {
    console.log(
      "[Chat] loadSession called:",
      session.id,
      "messages count:",
      session.messages?.length
    );

    // 清理当前视图的临时展示状态；其他会话的流式回复继续后台完成。
    currentStreamContent.value = "";

    // 尝试从数据库重新加载会话数据（确保消息最新）
    let sessionWithMessages = session;
    try {
      const dbSessions = await invoke<DbSession[]>("get_sessions_cmd");
      console.log("[Chat] Loaded sessions from DB:", dbSessions.length);
      const freshSession = dbSessions.find((s) => String(s.id) === String(session.id));
      console.log(
        "[Chat] Found fresh session:",
        freshSession?.id,
        "messages:",
        freshSession?.messages?.length
      );
      if (freshSession) {
        // 创建新对象确保响应式更新，使用数据库中的最新数据
        sessionWithMessages = toChatSession(freshSession);
        console.log(
          "[Chat] Created new session object with messages:",
          sessionWithMessages.messages.length
        );
      }
    } catch (error) {
      console.warn("Failed to reload session from DB, using cached data:", error);
    }

    // 设置当前会话并设置流式监听器
    currentSession.value = sessionWithMessages;
    lastRetrievalResult.value = null;
    console.log("[Chat] currentSession set, messages:", currentSession.value?.messages?.length);
    await setupStreamListener();
    await setupToolCallListener();
    await setupTokenUsageListener();
  };

  /**
   * 校验当前会话可用于生成回复的 API 配置
   * sendMessage / regenerateMessage / editUserMessage 共用同一份校验逻辑
   *
   * @returns 校验通过的配置对象，失败返回 null（已弹出 alert 提示）
   */
  const resolveActiveConfig = () => {
    if (!currentSession.value) return null;

    // 优先使用当前激活的 API 配置（允许在不新建会话的情况下切换 API）
    const effectiveConfigId = settings.activeConfigId ?? currentSession.value.apiConfigId;
    const config = settings.apiConfigs.find((c) => c.id === effectiveConfigId);
    if (!config) {
      console.error("API config not found for session");
      alert("未找到 API 配置，请检查设置");
      return null;
    }
    // 若与会话绑定的配置不同，同步更新当前会话（影响 History 显示）
    if (effectiveConfigId !== currentSession.value.apiConfigId) {
      currentSession.value.apiConfigId = config.id;
      currentSession.value.provider = config.provider;
      currentSession.value.model = config.model;
    }

    return config;
  };

  /**
   * 基于当前会话已有的消息列表向 LLM 请求一次新回复 (核心生成函数)
   * 发送新消息、编辑用户消息后重新生成、点击"重新生成"共用这一段逻辑——
   * 三者的差异只在调用前如何整理 currentSession.value.messages，生成本身
   * (占位消息、构建 API 消息数组、system prompt 注入、流式调用、标题更新、
   * 错误处理) 完全一致，不应该抄三份。
   *
   * @param contentOverride - 仅用于 sendMessage 的 RAG/文档上下文注入：某条
   *   消息在聊天气泡里显示原始输入，但发给模型的那一份要换成注入过上下文的
   *   增强内容。不传则每条消息都按 m.content 原样发送。
   * @returns void
   */
  const generateReply = async (contentOverride?: { messageId: string; content: string }) => {
    if (!currentSession.value) return;

    const session = currentSession.value;
    const sessionId = String(session.id);
    if (inFlightReplies.has(sessionId)) return;

    const config = resolveActiveConfig();
    if (!config) return;

    currentStreamContent.value = "";

    try {
      // 创建助手消息占位
      const assistantMessage: Message = {
        id: crypto.randomUUID(),
        role: "assistant",
        content: "",
        timestamp: Date.now(),
        streaming: true,
      };
      session.messages.push(assistantMessage);
      const reply: InFlightReply = { sessionId, message: assistantMessage };
      inFlightReplies.set(sessionId, reply);
      syncInFlightReply(sessionId);

      // ============ 构建 API 消息列表 ============
      const apiMessages = session.messages
        // 过滤掉流式中和有错误的消息
        .filter((m) => !m.streaming && !m.error)
        .map((m) => ({
          id: m.id,
          role: m.role,
          content:
            contentOverride && m.id === contentOverride.messageId
              ? contentOverride.content
              : m.content,
          timestamp: m.timestamp,
          error: m.error,
          images: m.images ?? [],
          videos: m.videos ?? [],
        }));

      // ============ 全局 System Prompt ============
      const globalSystemPrompt = settings.systemPrompt.trim();
      if (globalSystemPrompt) {
        if (apiMessages.length > 0 && apiMessages[0].role === "system") {
          apiMessages[0] = {
            ...apiMessages[0],
            content: globalSystemPrompt + "\n\n" + apiMessages[0].content,
          };
        } else {
          apiMessages.unshift({
            id: crypto.randomUUID(),
            role: "system",
            content: globalSystemPrompt,
            timestamp: Date.now(),
            error: undefined,
            images: [],
            videos: [],
          });
        }
      }

      // ============ 构建请求 payload ============
      // MCP 工具不再以文本形式塞进 system prompt——后端会在 enableMcp 开启时
      // 通过各 provider 的原生 tools 字段声明工具并执行多轮调用循环，前端
      // 再注入一份 JSON 文本只会让每轮请求重复付一份 token。
      const requestPayload = {
        sessionId,
        messages: apiMessages,
        provider: config.provider,
        model: config.model,
        // 密钥不离开前端进程：后端按配置 ID 从系统 keyring 取用。
        apiConfigId: config.id,
        baseUrl: config.baseUrl,
        enableMcp: mcpEnabled.value,
        activeSkillIds: activeSkillIds.value,
        enableSkillAutonomy: skillAutonomyEnabled.value,
        enableThinking: thinkingEnabled.value,
        maxTokens: config.maxTokens ?? null,
        retryCount: settings.retryCount,
        retryIntervalSecs: settings.retryIntervalSecs,
        maxToolRounds: settings.maxToolRounds,
        workingDirectory: session.workingDirectory,
        fileAccessMode: session.fileAccessMode,
        terminalShell: localStorage.getItem("baiyu.mcp.terminal-shell") || "powershell",
      };

      // 开发模式下打印调试日志 (隐藏 API 密钥)
      if (import.meta.env.DEV) {
        console.debug("STREAM_REQUEST (masked):", {
          sessionId: requestPayload.sessionId,
          provider: requestPayload.provider,
          model: requestPayload.model,
          baseUrl: requestPayload.baseUrl,
          enableMcp: requestPayload.enableMcp,
          apiConfigId: requestPayload.apiConfigId,
          messagesCount: requestPayload.messages?.length ?? 0,
        });
      }

      // ============ 调用后端流式消息 API ============
      try {
        console.log(
          "[generateReply] Calling stream_message, sessionId:",
          requestPayload.sessionId,
          "messageCount:",
          requestPayload.messages.length
        );
        await invoke("stream_message", { request: requestPayload });
        console.log("[generateReply] stream_message completed");
      } catch (e) {
        console.error("[generateReply] stream_message error:", e);
        if (import.meta.env.DEV) console.error("stream_message error", e);
        throw e;
      }

      // ============ 更新会话标题 ============
      // 如果是第一条对话 (用户消息 + 助手回复)，更新标题
      if (session.messages.length === 2) {
        const firstUserMessage = session.messages[0];
        session.title =
          firstUserMessage.content.slice(0, 30) +
          (firstUserMessage.content.length > 30 ? "..." : "");
        await saveSessionToDb(session);
        // 只局部更新 sessions 列表的标题，不调用 loadSessionsFromDb()
        // 流式回复已由 inFlightReplies 与数据库快照合并，这里无需整表刷新。
        const sid = session.id;
        const newTitle = session.title;
        const existingIdx = sessions.value.findIndex((s) => s.id === sid);
        if (existingIdx !== -1) {
          sessions.value[existingIdx] = { ...sessions.value[existingIdx], title: newTitle };
        } else {
          // 新会话还不在列表里（首次发消息），插到最前面
          sessions.value.unshift({
            id: session.id,
            title: newTitle,
            provider: session.provider,
            model: session.model,
            workingDirectory: session.workingDirectory,
            fileAccessMode: session.fileAccessMode,
            apiConfigId: session.apiConfigId,
            createdAt: session.createdAt,
            updatedAt: Date.now(),
            messages: [],
          });
        }
      }
    } catch (error) {
      // ============ 错误处理 ============
      const errorInfo = classifyError(error);
      const reply = inFlightReplies.get(sessionId);

      // 将错误信息保存到本轮绑定的助手消息中，不受当前浏览会话影响。
      if (reply) {
        reply.message.error = errorInfo.message;
        reply.message.streaming = false;
        syncInFlightReply(sessionId);
        inFlightReplies.delete(sessionId);
        await saveMessageToDb(sessionId, reply.message);
      }

      console.error(`[${errorInfo.type}] ${error}`);
      const notice = errorInfo.message;
      if (!errorNotices.value.includes(notice)) errorNotices.value.push(notice);
      currentStreamContent.value = "";
    }
  };

  const buildEnhancedContent = async (
    content: string,
    documentContents?: Array<{ name: string; content: string }>
  ): Promise<string> => {
    const contextParts: string[] = [];
    lastRetrievalResult.value = null;

    if (ragEnabled.value && selectedKnowledgeBaseId.value) {
      const kbExists = kbStore.knowledgeBases.some((kb) => kb.id === selectedKnowledgeBaseId.value);
      if (!kbExists) {
        kbStore.retrievalNotices.push({
          type: "error",
          title: "知识库不可用",
          message: "所选知识库不存在或已被删除，本次将不使用知识库内容。",
        });
      } else {
        const result = await kbStore.searchKnowledgeBase(selectedKnowledgeBaseId.value, content);
        if (result) {
          // 空结果也要覆盖上一次状态，让界面明确显示“检索到 0 个片段”，
          // 而不是继续展示上一轮的旧数量。
          lastRetrievalResult.value = result;
          const ragContext = buildRagContext(result);
          if (ragContext) contextParts.push(ragContext);
        }
      }
    }

    if (documentContents && documentContents.length > 0) {
      const docParts = documentContents.map((d) => `[文档: ${d.name}]\n${d.content}`);
      contextParts.push(`[用户附加文档]\n${docParts.join("\n---\n")}`);
    }

    return contextParts.length > 0 ? `${contextParts.join("\n\n")}\n\n问题：${content}` : content;
  };

  /**
   * 发送消息 (核心函数)
   * 处理用户消息发送、LLM 调用、流式响应等完整流程
   *
   * @param content - 消息内容
   * @param attachedFiles - 附件文件列表 (可选, 仅元数据)
   * @param images - 图片附件 (可选, 含 base64 数据)
   * @returns void
   */
  const sendMessage = async (
    content: string,
    attachedFiles?: Array<{ name: string; size: number }>,
    images?: ImageAttachment[],
    videos?: VideoAttachment[],
    documentContents?: Array<{ name: string; content: string }>
  ) => {
    // 检查是否有当前会话
    if (!currentSession.value) return;
    if (!resolveActiveConfig()) return;

    const enhancedContent = await buildEnhancedContent(content, documentContents);

    // 构建用户消息对象——聊天气泡展示原始输入，RAG/文档增强内容只通过
    // generateReply 的 contentOverride 参数注入发给模型的那份拷贝，不写进
    // 消息本身（写进去用户编辑这条消息时会看到一堆检索上下文，体验很差）
    const userMessage: Message = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      timestamp: Date.now(),
      files: attachedFiles && attachedFiles.length > 0 ? attachedFiles : undefined,
      images: images && images.length > 0 ? images : undefined,
      videos: videos && videos.length > 0 ? videos : undefined,
    };

    // 添加到当前会话
    currentSession.value.messages.push(userMessage);
    currentSession.value.updatedAt = Date.now();

    // 保存到数据库（先保存 session，再保存 message，满足外键约束）
    await saveSessionToDb();
    await saveMessageToDb(currentSession.value.id, userMessage);

    await generateReply(
      enhancedContent !== content
        ? { messageId: userMessage.id, content: enhancedContent }
        : undefined
    );
  };

  /**
   * 手动创建当前会话的上下文交接摘要。
   *
   * 完整消息仍留在 SQLite 和界面中；后端只记录“从哪条消息开始保留原文”，
   * 后续请求会自动使用“摘要 + 最近消息”。图片/视频不参加摘要请求，避免
   * base64 附件本身把压缩请求撑爆。
   */
  const compactContext = async (focus = "") => {
    const session = currentSession.value;
    const config = resolveActiveConfig();
    if (!session || !config || isLoading.value) return false;

    const completedMessages = session.messages.filter(
      (message) => !message.streaming && !message.error && message.content.trim(),
    );
    if (completedMessages.length < 4) {
      const notice = "至少完成两轮对话后，才能压缩上下文。";
      if (!warningNotices.value.includes(notice)) warningNotices.value.push(notice);
      return false;
    }

    try {
      await invoke("compact_chat_context", {
        request: {
          sessionId: String(session.id),
          messages: completedMessages.map((message) => ({
            id: message.id,
            role: message.role,
            content: message.content,
            timestamp: message.timestamp,
            error: message.error,
            // 压缩器只需要文本证据；不要通过 IPC 传输附件 base64。
            images: [],
            videos: [],
          })),
          provider: config.provider,
          model: config.model,
          apiConfigId: config.id,
          baseUrl: config.baseUrl,
          maxTokens: config.maxTokens ?? null,
          focus,
        },
      });
      const notice = "已压缩活动上下文；完整聊天记录仍保留。";
      if (!warningNotices.value.includes(notice)) warningNotices.value.push(notice);
      return true;
    } catch (error) {
      console.error("Failed to compact chat context:", error);
      const notice = `上下文压缩失败：${classifyError(error).message}`;
      if (!errorNotices.value.includes(notice)) errorNotices.value.push(notice);
      return false;
    }
  };

  /**
   * 从数据库批量删除消息（编辑/重新生成截断旧分支时用）
   * 失败塞进 dbSaveErrorNotices 队列走统一弹窗，理由同 saveMessageToDb——
   * store 里拿不到 NMessageProvider 上下文，没法直接弹窗
   *
   * @param messages - 要删除的消息列表
   * @returns void
   */
  const deleteMessagesFromDb = async (messages: Message[]) => {
    for (const m of messages) {
      try {
        await invoke("delete_message_cmd", { messageId: m.id });
      } catch (error) {
        console.error("Failed to delete message:", error);
        dbSaveErrorNotices.value.push(`旧消息清理失败：${classifyError(error).message}`);
      }
    }
  };

  /**
   * 编辑一条已发送的用户消息并重新生成回复
   * 截断该消息之后的所有旧消息（含旧的 AI 回复），更新消息内容，再重新请求一次生成——
   * 与 ChatGPT/Claude 官方客户端的"编辑并重新生成"行为一致，不支持只改文字不重新生成，
   * 因为旧回复是针对旧内容生成的，留着会造成上下文和回复对不上。
   *
   * @param messageId - 要编辑的用户消息 ID
   * @param newContent - 编辑后的新内容
   * @returns void
   */
  const editUserMessage = async (messageId: string, newContent: string) => {
    if (!currentSession.value) return;
    if (isLoading.value) return;

    const idx = currentSession.value.messages.findIndex((m) => m.id === messageId);
    if (idx === -1) return;
    const target = currentSession.value.messages[idx];
    if (target.role !== "user") return;

    const trimmed = newContent.trim();
    if (!trimmed) return;

    // 截断该消息之后的所有消息（旧回复分支作废）
    const removed = currentSession.value.messages.splice(idx + 1);

    target.content = trimmed;
    target.error = undefined;
    currentSession.value.updatedAt = Date.now();

    // 先删库里的旧消息，再保存编辑后的内容
    await deleteMessagesFromDb(removed);
    await saveMessageToDb(currentSession.value.id, target);
    await saveSessionToDb();

    const enhancedContent = await buildEnhancedContent(trimmed);
    await generateReply(
      enhancedContent !== trimmed ? { messageId: target.id, content: enhancedContent } : undefined
    );
  };

  /**
   * 重新生成指定的 AI 回复
   * 删除该回复（及其后的所有消息，理论上只会有它自己），基于剩余上下文重新请求一次生成
   *
   * @param messageId - 要重新生成的 assistant 消息 ID
   * @returns void
   */
  const regenerateMessage = async (messageId: string) => {
    if (!currentSession.value) return;
    if (isLoading.value) return;

    const idx = currentSession.value.messages.findIndex((m) => m.id === messageId);
    if (idx === -1) return;
    const target = currentSession.value.messages[idx];
    if (target.role !== "assistant") return;

    const removed = currentSession.value.messages.splice(idx);
    await deleteMessagesFromDb(removed);

    const lastUserMessage = [...currentSession.value.messages]
      .reverse()
      .find((message) => message.role === "user");
    if (!lastUserMessage) return;

    const enhancedContent = await buildEnhancedContent(lastUserMessage.content);
    await generateReply(
      enhancedContent !== lastUserMessage.content
        ? { messageId: lastUserMessage.id, content: enhancedContent }
        : undefined
    );
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
      contextParts.push(
        `\n[文档 ${index + 1}: ${chunk.document_filename}]\n${chunk.chunk.content}`
      );
    });

    contextParts.push("\n---");
    return contextParts.join("\n");
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
   * 切换某个 Skill 的手动激活状态
   */
  const toggleSkillActive = (skillId: string) => {
    const idx = activeSkillIds.value.indexOf(skillId);
    if (idx === -1) {
      activeSkillIds.value.push(skillId);
    } else {
      activeSkillIds.value.splice(idx, 1);
    }
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
  const stopStream = async () => {
    if (!isLoading.value || !currentSession.value) return;

    const sessionId = String(currentSession.value.id);

    // Call backend to cancel the stream
    try {
      await invoke("cancel_stream", { sessionId });
      console.log("[Stream] Cancellation request sent to backend");
    } catch (error) {
      // Log warning but continue with frontend cleanup
      console.warn("[Stream] Failed to cancel stream on backend:", error);
    }

    // 保存本会话的部分结果；随后到达的 done 事件因已无 in-flight 记录会被忽略。
    const reply = inFlightReplies.get(sessionId);
    if (reply) {
      reply.message.streaming = false;
      syncInFlightReply(sessionId);
      inFlightReplies.delete(sessionId);
      await saveMessageToDb(sessionId, reply.message);
    }
    currentStreamContent.value = "";
    console.log("[Stream] Stopped by user");
  };

  // ============ 返回公共接口 ============
  return {
    setWorkingDirectory,
    // 状态
    currentSession,
    sessions,
    isLoading,
    currentStreamContent,
    ragEnabled,
    dbSaveErrorNotices,
    errorNotices,
    warningNotices,
    selectedKnowledgeBaseId,
    lastRetrievalResult,
    mcpEnabled,
    activeSkillIds,
    skillAutonomyEnabled,
    thinkingEnabled,

    // 方法
    createSession, // 创建新会话
    loadSession, // 加载会话
    sendMessage, // 发送消息
    compactContext, // 生成摘要并切换活动上下文，不删除历史记录
    editUserMessage, // 编辑用户消息并重新生成
    regenerateMessage, // 重新生成 AI 回复
    deleteSession, // 删除会话
    clearSession, // 清除当前会话
    toggleSkillActive, // 切换 Skill 手动激活状态
    loadSessionsFromDb, // 加载会话列表
    toggleRag, // 切换 RAG
    selectKnowledgeBaseForRag, // 选择知识库
    classifyError, // 错误分类
    stopStream, // 停止流式输出
  };
});
