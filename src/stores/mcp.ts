/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

export interface MCPServer {
  id: string;
  name: string;
  description: string;
  server_type: "stdio" | "sse" | "http";
  command: string;
  args: string[];
  env: Record<string, string>;
  port?: number;
  url?: string;
  api_key?: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface MCPTool {
  server_id: string;
  server_name: string;
  name: string;
  description: string;
  input_schema: Record<string, any>;
}

export interface MCPToolResult {
  tool_name: string;
  result: Record<string, any>;
  error?: string;
}

export const useMCPStore = defineStore("mcp", () => {
  const servers = ref<MCPServer[]>([]);
  const tools = ref<MCPTool[]>([]);
  const isLoading = ref(false);
  const selectedServerId = ref<string | null>(null);

  // Get available tools from all enabled servers
  const availableTools = computed(() => {
    return tools.value.filter((tool) => {
      const server = servers.value.find((s) => s.id === tool.server_id);
      return server?.enabled;
    });
  });

  // Get tools for a specific server
  const getToolsByServer = (serverId: string) => {
    return tools.value.filter((tool) => tool.server_id === serverId);
  };

  // Load all MCP servers
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

  // Load all tools from all servers
  const loadAllTools = async () => {
    try {
      const allTools = await invoke<MCPTool[]>("get_all_mcp_tools");
      tools.value = allTools || [];
    } catch (error) {
      console.error("Failed to load MCP tools:", error);
    }
  };

  // Load tools for a specific server
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

  // Create or update a MCP server
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

  // Update a MCP server
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
