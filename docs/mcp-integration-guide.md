# MCP 服务集成开发指南

## 概述

本项目支持 **Model Context Protocol (MCP)** 的完整集成，允许用户在聊天中调用来自不同 MCP 服务器的工具和功能。

## MCP 服务类型

### 1. Stdio (标准输入输出)
适用于本地可执行程序或脚本
```
启动命令: /path/to/mcp-server
参数: --config config.json --port 8000
```

### 2. SSE (Server-Sent Events)
用于基于 HTTP 的流式通信
```
URL: http://localhost:8000
```

### 3. HTTP API
标准 HTTP REST API 服务
```
URL: https://api.example.com
端口: 8080 (可选)
API Key: xxx (如需认证)
```

## 后端实现细节

### 核心文件结构

```
src-tauri/src/
├── commands/
│   ├── mcp.rs          # MCP 命令实现
│   └── mod.rs          # 模块注册
├── main.rs             # Tauri 命令注册
```

### MCP 命令 API

#### 1. `create_mcp_server`
创建或更新 MCP 服务器配置

**参数:**
```rust
MCPServer {
    id: String,
    name: String,
    description: String,
    server_type: "stdio" | "sse" | "http",
    command: String,           // 用于 stdio
    args: Vec<String>,         // 用于 stdio
    env: HashMap<String, String>,
    port: Option<u16>,         // 用于 HTTP/SSE
    url: Option<String>,       // 用于 HTTP/SSE
    api_key: Option<String>,   // 用于认证
    enabled: bool,
    created_at: i64,
    updated_at: i64,
}
```

#### 2. `list_mcp_servers`
获取所有已配置的 MCP 服务器列表

#### 3. `delete_mcp_server`
删除 MCP 服务器配置

#### 4. `get_mcp_tools`
获取特定 MCP 服务器提供的工具列表

#### 5. `get_all_mcp_tools`
获取所有启用的 MCP 服务器提供的工具

#### 6. `call_mcp_tool`
调用 MCP 工具

**参数:**
```rust
{
    server_id: String,
    tool_name: String,
    input: serde_json::Value,
}
```

**返回:**
```rust
MCPToolResult {
    tool_name: String,
    result: serde_json::Value,
    error: Option<String>,
}
```

#### 7. `test_mcp_connection`
测试与 MCP 服务器的连接

## 前端实现细节

### Pinia 存储 (`src/stores/mcp.ts`)

```typescript
export const useMCPStore = defineStore("mcp", () => {
  // State
  const servers = ref<MCPServer[]>([]);
  const tools = ref<MCPTool[]>([]);
  
  // Methods
  const createServer = async (server: MCPServer) => {...};
  const updateServer = async (serverId, updates) => {...};
  const deleteServer = async (serverId) => {...};
  const toggleServerEnabled = async (serverId) => {...};
  const callTool = async (serverId, toolName, input) => {...};
  const testConnection = async (type, command?, url?) => {...};
  
  // Computed
  const availableTools = computed(() => {...});
});
```

### UI 组件 (`src/views/MCPView.vue`)

MCPView 提供完整的 MCP 服务管理界面：

- **服务列表**: 显示所有已配置的 MCP 服务
- **添加服务**: 模态表单支持三种服务类型
- **测试连接**: 验证服务配置有效性
- **可用工具**: 展示所有已启用服务提供的工具
- **启用/禁用**: 快速切换服务状态

## 在 Chat 中调用 MCP 工具

### 实现步骤

1. **修改 `ChatInput.vue`**（可选）
   - 添加工具选择器
   - 显示可用的 MCP 工具列表

2. **修改 `chat.ts` 存储**
   - 在 `sendMessage` 时检查是否需要调用 MCP 工具
   - 构建包含工具定义的 system prompt

3. **修改 `llm.rs` 后端**
   - 在 LLM 调用中支持函数调用 (function calling)
   - 解析 LLM 返回的工具调用请求
   - 调用对应的 MCP 工具
   - 将工具结果返回给 LLM 进行推理

### 流程示意

```
用户输入
    ↓
检查是否启用 RAG/MCP
    ↓
构建增强的 prompt
    ├─ RAG: 添加检索到的上下文
    └─ MCP: 添加可用工具定义
    ↓
调用 LLM (支持 function calling)
    ↓
LLM 响应
    ├─ 文本内容 → 直接显示
    └─ 工具调用 → 调用对应 MCP 工具
    ↓
将工具结果反馈给 LLM
    ↓
LLM 基于工具结果继续生成内容
    ↓
显示最终结果
```

## 集成 LLM 函数调用（TODO）

### OpenAI 格式示例

```json
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "search_wikipedia",
        "description": "搜索 Wikipedia 获取信息",
        "parameters": {
          "type": "object",
          "properties": {
            "query": {
              "type": "string",
              "description": "搜索关键词"
            }
          },
          "required": ["query"]
        }
      }
    }
  ]
}
```

### Anthropic 格式示例

```json
{
  "tools": [
    {
      "name": "search_wikipedia",
      "description": "搜索 Wikipedia 获取信息",
      "input_schema": {
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "搜索关键词"
          }
        },
        "required": ["query"]
      }
    }
  ]
}
```

## 常见 MCP 服务器实现

### 1. Python 示例

```python
import json
import sys

def handle_message(msg):
    if msg["method"] == "resources/list":
        return {
            "resources": [
                {
                    "uri": "file:///docs",
                    "name": "文档资源",
                    "description": "本地文档"
                }
            ]
        }
    elif msg["method"] == "tools/list":
        return {
            "tools": [
                {
                    "name": "search",
                    "description": "搜索工具",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        }
                    }
                }
            ]
        }

if __name__ == "__main__":
    for line in sys.stdin:
        msg = json.loads(line)
        result = handle_message(msg)
        print(json.dumps(result))
```

### 2. Node.js 示例

```javascript
const readline = require('readline');

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout
});

rl.on('line', (line) => {
  const msg = JSON.parse(line);
  
  let result;
  if (msg.method === 'tools/list') {
    result = {
      tools: [
        {
          name: 'fetch_url',
          description: '获取网页内容',
          inputSchema: {
            type: 'object',
            properties: {
              url: { type: 'string' }
            }
          }
        }
      ]
    };
  }
  
  console.log(JSON.stringify(result));
});
```

## 数据持久化

目前 MCP 服务器配置存储在内存中（TODO: 实现 SQLite 存储）

### 计划实现

```sql
CREATE TABLE mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    server_type TEXT NOT NULL,
    command TEXT,
    args TEXT,
    env TEXT,
    port INTEGER,
    url TEXT,
    api_key_encrypted TEXT,
    enabled BOOLEAN,
    created_at INTEGER,
    updated_at INTEGER
);

CREATE TABLE mcp_tools (
    id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    input_schema TEXT,
    FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
```

## 安全考虑

1. **API Key 保护**
   - 使用系统密钥链加密存储（与 LLM API Key 相同）
   - 避免日志中出现敏感信息

2. **命令执行安全**
   - 验证 stdio 命令路径
   - 限制可执行的命令范围
   - 实现超时机制防止无限运行

3. **网络安全**
   - 验证 HTTPS 证书
   - 实现请求超时
   - 限制重定向次数

## 测试 MCP 服务

### 使用 nc 测试 HTTP 服务

```bash
curl -X GET http://localhost:8000/tools
```

### 使用 strace 调试 stdio 服务

```bash
strace -e trace=write,read ./mcp-server
```

## 下一步实现

- [ ] SQLite 持久化存储
- [ ] LLM function calling 集成
- [ ] MCP 工具的完整生命周期管理
- [ ] 并发工具调用支持
- [ ] 工具调用超时和重试机制
- [ ] MCP 服务的自动启动/停止
- [ ] 工具调用日志和监控
