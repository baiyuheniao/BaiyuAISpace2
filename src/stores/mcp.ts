/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * MCP Store - 管理 Model Context Protocol (MCP) 服务器和工具
 * 
 * 功能说明:
 * - MCP 服务器的 CRUD 操作
 * - 服务器类型支持: stdio (标准输入输出), SSE (服务器发送事件), HTTP
 * - 工具列表加载和管理
 * - 工具调用和结果处理
 * 
 * 使用方式:
 * import { useMCPStore } from "@/stores/mcp";
 * const mcp = useMCPStore();
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

// ============ 类型定义 ============

/**
 * MCP 服务器配置
 * 定义一个可连接的 MCP 服务器
 */
export interface MCPServer {
  id: string;                      // 服务器唯一标识符
  name: string;                    // 服务器名称
  description: string;            // 服务器描述
  server_type: "stdio" | "sse" | "http";  // 服务器连接类型
  command: string;                // 启动命令 (stdio 类型使用)
  args: string[];                 // 命令行参数
  env: Record<string, string>;   // 环境变量
  port?: number;                  // 端口号 (SSE/HTTP 类型使用)
  url?: string;                   // 服务器 URL (SSE/HTTP 类型使用)
  api_key?: string;              // API 密钥 (可选)
  enabled: boolean;              // 是否启用
  created_at: number;            // 创建时间戳
  updated_at: number;            // 更新时间戳
}

/**
 * MCP 工具
 * 服务器提供的可调用工具
 */
export interface MCPTool {
  server_id: string;             // 所属服务器 ID
  server_name: string;            // 服务器名称
  name: string;                  // 工具名称
  description: string;           // 工具描述
  input_schema: Record<string, any>;  // 输入参数 schema
}

/**
 * MCP 工具调用结果
 */
export interface MCPToolResult {
  tool_name: string;              // 工具名称
  result: Record<string, any>;   // 调用结果
  error?: string;                 // 错误信息 (如果有)
}

export const useMCPStore = defineStore("mcp", () => {
  // ============ 响应式状态 ============
  
  // MCP 服务器列表
  const servers = ref<MCPServer[]>([]);
  
  // 可用工具列表
  const tools = ref<MCPTool[]>([]);
  
  // 是否正在加载
  const isLoading = ref(false);
  
  // 当前选中的服务器 ID
  const selectedServerId = ref<string | null>(null);

  // ============ 计算属性 ============
  
  // 获取所有已启用服务器的工具
  const availableTools = computed(() => {
    return tools.value.filter((tool) => {
      const server = servers.value.find((s) => s.id === tool.server_id);
      return server?.enabled;
    });
  });

  // ============ 方法函数 ============
  
  // 获取指定服务器的工具列表
  const getToolsByServer = (serverId: string) => {
    return tools.value.filter((tool) => tool.server_id === serverId);
  };

  // 加载所有 MCP 服务器
  const loadServers = async () => {
    try {
      isLoading.value = true;
      const loadedServers = await invoke<MCPServer[]>("list_mcp_servers");
      servers.value = loadedServers || [];
      
      // Load tools for each server
      if (servers.value.length > 0) {
        await loadAllTools();
      }
    } catch (error) {
      console.error("Failed to load MCP servers:", error);
    } finally {
      isLoading.value = false;
    }
  };

  // 加载所有服务器的工具
  const loadAllTools = async () => {
    try {
      const allTools = await invoke<MCPTool[]>("get_all_mcp_tools");
      tools.value = allTools || [];
    } catch (error) {
      console.error("Failed to load MCP tools:", error);
    }
  };

  // 加载指定服务器的工具
  const loadServerTools = async (serverId: string) => {
    try {
      const serverTools = await invoke<MCPTool[]>("get_mcp_tools", {
        server_id: serverId,
      });
      // Filter and merge with existing tools
      tools.value = tools.value.filter((t) => t.server_id !== serverId);
      tools.value.push(...(serverTools || []));
    } catch (error) {
      console.error(`Failed to load tools for server ${serverId}:`, error);
    }
  };

  // 创建或更新 MCP 服务器
  const createServer = async (
    server: Omit<MCPServer, "id" | "created_at" | "updated_at">
  ): Promise<MCPServer | null> => {
    try {
      isLoading.value = true;
      const newServer: MCPServer = {
        ...server,
        id: crypto.randomUUID(),
        created_at: Date.now(),
        updated_at: Date.now(),
      };

      const createdServer = await invoke<MCPServer>("create_mcp_server", {
        server: newServer,
      });

      if (createdServer) {
        servers.value.push(createdServer);
        // Load tools from the new server
        await loadServerTools(createdServer.id);
        return createdServer;
      }
    } catch (error) {
      console.error("Failed to create MCP server:", error);
    } finally {
      isLoading.value = false;
    }
    return null;
  };

  // 更新 MCP 服务器
  const updateServer = async (
    serverId: string,
    updates: Partial<MCPServer>
  ): Promise<void> => {
    try {
      const server = servers.value.find((s) => s.id === serverId);
      if (!server) return;

      const updated = {
        ...server,
        ...updates,
        updated_at: Date.now(),
      };

      const result = await invoke<MCPServer>("create_mcp_server", {
        server: updated,
      });

      const idx = servers.value.findIndex((s) => s.id === serverId);
      if (idx !== -1 && result) {
        servers.value[idx] = result;
      }
    } catch (error) {
      console.error("Failed to update MCP server:", error);
    }
  };

  // Delete a MCP server
  const deleteServer = async (serverId: string): Promise<void> => {
    try {
      await invoke("delete_mcp_server", { server_id: serverId });
      servers.value = servers.value.filter((s) => s.id !== serverId);
      tools.value = tools.value.filter((t) => t.server_id !== serverId);

      if (selectedServerId.value === serverId) {
        selectedServerId.value = null;
      }
    } catch (error) {
      console.error("Failed to delete MCP server:", error);
    }
  };

  // Toggle server enabled state
  const toggleServerEnabled = async (serverId: string): Promise<void> => {
    const server = servers.value.find((s) => s.id === serverId);
    if (!server) return;
    await updateServer(serverId, { enabled: !server.enabled });
  };

  // Call a MCP tool
  const callTool = async (
    serverId: string,
    toolName: string,
    input: Record<string, any>
  ): Promise<MCPToolResult | null> => {
    try {
      const result = await invoke<MCPToolResult>("call_mcp_tool", {
        server_id: serverId,
        tool_name: toolName,
        input,
      });
      return result;
    } catch (error) {
      console.error("Failed to call MCP tool:", error);
    }
    return null;
  };

  // Test MCP server connection
  const testConnection = async (
    serverType: string,
    command?: string,
    url?: string
  ): Promise<boolean> => {
    try {
      return await invoke<boolean>("test_mcp_connection", {
        server_type: serverType,
        command,
        url,
      });
    } catch (error) {
      console.error("MCP connection test failed:", error);
      return false;
    }
  };

  return {
    servers,
    tools,
    isLoading,
    selectedServerId,
    availableTools,
    loadServers,
    loadAllTools,
    loadServerTools,
    getToolsByServer,
    createServer,
    updateServer,
    deleteServer,
    toggleServerEnabled,
    callTool,
    testConnection,
  };
});
