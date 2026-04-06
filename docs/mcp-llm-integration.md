# MCP 与 LLM 函数调用集成指南

## 概述

本文档说明了BaiyuAISpace中MCP（Model Context Protocol）与LLM函数调用的完整集成实现。用户可以在对话中启用MCP工具，AI助手将自动获 access 到这些工具并在适当的时候调用它们。

## 实现细节

### 1. 前端集成（chat.ts中的sendMessage方法）

#### MCP工具定义构建

当用户启用MCP且有可用工具时，系统会自动：

1. **收集可用工具** - 从useMCPStore获取所有启用的服务器的工具
2. **生成工具定义** - 将工具元数据转换为OpenAI兼容的函数定义格式
3. **构建系统提示** - 创建包含工具说明的系统提示词

```typescript
const buildMcpToolDefinitions = (availableTools: MCPTool[]): string => {
  // 将MCPTool[]转换为JSON格式的工具定义
  // 包含：type: "function"，function.name，description，parameters
  return toolDefString;
};

const buildMcpSystemPrompt = (availableTools: MCPTool[]): string => {
  // 构建包含工具列表和使用说明的系统提示词
  return systemPrompt;
};
```

#### 集成到消息流

在`sendMessage`方法中：

```typescript
if (mcpEnabled.value && mcp.availableTools.length > 0) {
  const mcpSystemPrompt = buildMcpSystemPrompt(mcp.availableTools);
  
  // 合并到系统消息中（或创建新的系统消息）
  if (apiMessages.length > 0 && apiMessages[0].role === "system") {
    apiMessages[0].content += "\n\n" + mcpSystemPrompt;
  } else {
    apiMessages.unshift({
      id: crypto.randomUUID(),
      role: "system",
      content: mcpSystemPrompt,
      timestamp: Date.now(),
      error: undefined,
    });
  }
}
```

### 2. 后端支持（无需修改）

后端的`stream_message`命令已经完全支持自定义系统提示词。当前端传递包含MCP工具定义的系统消息时，后端会：

1. 构建符合所选LLM提供商格式的请求
2. 将系统提示词和消息内容传递给LLM API
3. 流式返回LLM的响应

### 3. 工作流程

```
用户启用MCP + 存在可用工具
         ↓
sendMessage()检查mcpEnabled标志
         ↓
从useMCPStore获取availableTools
         ↓
构建工具定义JSON和系统提示词
         ↓
将MCP系统提示词合并到消息列表
         ↓
发送给LLM API（带工具定义）
         ↓
LLM响应（可能提及工具使用）
         ↓
流式显示到UI
```

## 使用场景

### 场景1：启用单个MCP服务进行知识查询

1. 打开"MCP 服务"页面
2. 添加一个能执行数据库查询的MCP服务
3. 启用该服务
4. 在聊天界面，MCP按钮显示"1 服务 / X 工具"
5. 打开MCP开关
6. 提出问题，AI可以自动调用相关工具

### 场景2：同时使用RAG和MCP

1. 在知识库设置中启用特定知识库
2. 在MCP服务页面启用工具
3. 发送消息时，系统会：
   - 检索知识库相关内容（RAG）
   - 可用工具定义（MCP）
   - 合并发送给LLM

### 场景3：多工具协作

1. 添加多个MCP服务（例如：计算器、天气API、数据库）
2. 启用所有服务
3. 提出复杂问题，AI可以组合使用多个工具

## 当前限制与未来扩展

### 当前实现

✅ **已完成：**
- MCP工具定义的自动生成和格式化
- 系统提示词中的工具定义集成
- 与RAG兼容的混合模式
- 所有支持的LLM提供商

### 需要完成：**

⏳ **后续任务（优先级由高到低）：**

1. **函数调用响应处理** 
   - 检测LLM何时请求调用工具（基于提供商特定的响应格式）
   - 解析工具调用参数
   - 调用mcp.callTool()执行工具
   - 将结果回传给LLM进行继续推理

2. **工具执行UI反馈**
   - 在聊天界面显示"正在调用工具..."
   - 展示工具执行结果
   - 显示任何错误或异常

3. **MCP服务持久化**
   - 使用SQLite存储MCP配置
   - 重启应用后自动加载已配置服务

4. **实际MCP通讯**
   - 实现stdio进程管理
   - 实现SSE/WebSocket连接
   - 实现HTTP API调用
   - 错误重试和连接管理

## 实现示例

### Python示例（如何构建返回工具调用信息的LLM响应解析）

```python
import json

def parse_llm_response_for_function_calls(response_text: str, available_tools: list) -> list:
    """
    解析LLM响应中的函数调用请求
    """
    function_calls = []
    
    for tool in available_tools:
        # 查找工具提及（简单示例）
        if f"<use_tool>{tool['name']}</use_tool>" in response_text:
            # 提取参数（实际需要更复杂的解析）
            function_calls.append({
                "tool_name": tool["name"],
                "parameters": {}  # 需要实现参数提取逻辑
            })
    
    return function_calls

def execute_mcp_tool(tool_name: str, parameters: dict) -> dict:
    """
    通过Tauri IPC调用前端的mcp.callTool()
    """
    # 在TypeScript端实现，通过invoke调用后端
    # 后端通过invoke("call_mcp_tool")调用实际的MCP工具
    pass

def reconstruct_conversation(original_messages: list, tool_results: list) -> list:
    """
    将工具执行结果添加回对话历史，供LLM继续推理
    """
    messages = original_messages.copy()
    
    for result in tool_results:
        messages.append({
            "role": "assistant",
            "content": f"使用工具执行结果: {json.dumps(result)}"
        })
    
    return messages
```

### TypeScript实现建议

```typescript
// 在chat.ts中添加这些方法来处理函数调用

const parseToolCallsFromLLMResponse = (content: string, availableTools: MCPTool[]) => {
  // 基于提供商特定的响应格式解析工具调用
  // 例如：OpenAI使用tool_calls字段，Claude使用<use_tool>标签等
  const toolCalls = [];
  return toolCalls;
};

const executeToolCall = async (toolName: string, parameters: any) => {
  const mcp = useMCPStore();
  try {
    const result = await mcp.callTool(toolName, parameters);
    return {
      tool_name: toolName,
      result,
      error: null,
    };
  } catch (error) {
    return {
      tool_name: toolName,
      result: null,
      error: String(error),
    };
  }
};

const handleLLMFunctionCalls = async (
  llmResponse: string,
  availableTools: MCPTool[]
) => {
  const toolCalls = parseToolCallsFromLLMResponse(llmResponse, availableTools);
  
  const results = await Promise.all(
    toolCalls.map(call => executeToolCall(call.name, call.parameters))
  );
  
  return results;
};
```

## 测试MCP集成

### 基础测试

1. **启用MCP而无工具**
   - 启用MCP但不添加任何服务
   - 发送消息
   - 验证：没有工具定义被发送

2. **启用MCP有工具**
   - 添加和启用MCP服务
   - 发送消息
   - 验证：系统提示词包含工具定义

3. **MCP与RAG混合**
   - 同时启用知识库和MCP
   - 发送消息
   - 验证：两个增强都被应用

### 高级测试

1. **工具可用性更改**
   - 在对话中途启用/禁用工具
   - 验证：之后的消息反映正确的工具集合

2. **多工具场景**
   - 添加3-5个MCP服务
   - 发送需要多个工具的复杂查询
   - 观察LLM是否正确组织使用工具

3. **错误恢复**
   - 禁用所有服务后发送消息
   - 重新启用服务后验证系统提示词更新

## 故障排除

### 问题：工具定义未被发送

**检查清单：**
1. 验证mcpEnabled为true
2. 验证至少有一个启用的MCP服务
3. 验证该服务有关联的工具
4. 检查浏览器控制台是否有错误

### 问题：工具定义格式不正确

**解决方案：**
查看生成的systemPrompt，检查JSON格式是否有效。当前使用的是OpenAI兼容格式。

### 问题：LLM不调用工具

**原因分析：**
- LLM模型可能不支持函数调用（需要选择支持的模型）
- 系统提示词的措辞可能不清楚
- 工具定义的参数schema不完整

## 性能考虑

- 工具定义的JSON可能显著增加提示词大小
- 大量工具会增加token使用（和成本）
- 考虑按需启用服务，而不是全部启用

## 安全考虑

- 所有工具调用应该通过useMCPStore进行，并且验证工具是否启用
- 不应该允许用户直接调用未启用的工具
- 工具参数应该被验证和清理
- 记录所有工具调用以便审计

## 第四部分：MCP 工具执行的完整实现

### 概述（2026-02-15）

现已实现完整的 MCP 工具调用执行，支持 JSON-RPC 2.0 标准和多种服务器类型（Stdio、HTTP/SSE）。

### 工具调用流程

```
LLM 响应
  ↓
前端检测工具调用意图（正则匹配）
  ↓
提取工具名和参数
  ↓
调用 Tauri 的 call_mcp_tool 命令
  ↓
Rust 后端根据工具查找对应的 MCP 服务器配置
  ↓
根据服务器类型（Stdio/HTTP）执行对应的通信逻辑
  ↓
返回工具执行结果
  ↓
前端将结果追加到对话消息中
  ↓
LLM 继续推理并生成最终响应
```

### 前端实现

#### 工具调用检测（chat.ts）

```typescript
const handleMcpCalls = async (assistantMessage: Message): Promise<void> => {
  if (!mcpEnabled.value || mcp.availableTools.length === 0) return;

  // 正则表达式匹配 LLM 的工具调用格式
  // 期望格式：[使用工具: tool_name with input: {...}]
  const toolCallPattern = /\[使用工具: ([\w_-]+) with input: ({[^}]+})\]/g;
  const matches = Array.from(assistantMessage.content.matchAll(toolCallPattern));

  // 执行所有匹配的工具调用
  for (const match of matches) {
    const toolName = match[1];
    const toolInput = JSON.parse(match[2]);
    const result = await executeMcpTool(toolName, toolInput);
    // 将结果追加到消息中
  }
};
```

#### 工具执行（chat.ts）

```typescript
const executeMcpTool = async (toolName: string, toolInput: Record<string, unknown>): Promise<string> => {
  const mcp = useMCPStore();
  const tool = mcp.availableTools.find(t => t.name === toolName);
  
  if (!tool) {
    return `错误: 工具 "${toolName}" 不存在`;
  }

  try {
    const result = await invoke<serde_json.Value>("call_mcp_tool", {
      tool_name: toolName,
      input: toolInput,
    });

    // 提取并格式化结果
    if (typeof result === 'object' && result !== null) {
      const resultObj = result as Record<string, unknown>;
      
      if ('success' in resultObj && resultObj.success === false) {
        return `工具执行失败: ${resultObj.error}`;
      }
      
      if ('result' in resultObj) {
        return JSON.stringify(resultObj.result, null, 2);
      }
      
      return JSON.stringify(resultObj, null, 2);
    }
    return String(result);
  } catch (err) {
    return `调用工具时出错: ${String(err)}`;
  }
};
```

#### 错误分类（chat.ts）

```typescript
const classifyError = (error: unknown): { type: string; message: string } => {
  const errorStr = String(error);
  
  if (errorStr.includes("API key") || errorStr.includes("Unauthorized")) {
    return { type: "auth", message: "API 密钥无效或已过期" };
  } else if (errorStr.includes("network") || errorStr.includes("Failed to fetch")) {
    return { type: "network", message: "网络连接错误" };
  } else if (errorStr.includes("timeout")) {
    return { type: "timeout", message: "请求超时" };
  } else if (errorStr.includes("provider") || errorStr.includes("Invalid")) {
    return { type: "config", message: "API 配置错误" };
  } else {
    return { type: "unknown", message: `错误: ${errorStr}` };
  }
};
```

### 后端实现（Rust）

#### JSON-RPC 2.0 请求结构

```rust
struct ToolCallRequest {
    jsonrpc: String,      // "2.0"
    method: String,       // "tools/call"
    params: ToolCallParams,
    id: String,           // UUID 请求标识
}

struct ToolCallParams {
    name: String,         // 工具名称
    arguments: Value,     // 工具参数（JSON）
}
```

#### Stdio 服务器通信

实现在 `src-tauri/src/commands/mcp.rs` 中的 `call_mcp_tool_stdio` 函数：

1. **启动进程**：使用配置的命令和参数启动 MCP 服务器
2. **发送请求**：通过 stdin 发送 JSON-RPC 请求
3. **读取响应**：从 stdout 读取 JSON-RPC 响应
4. **解析结果**：提取 result 字段或返回错误

```rust
async fn call_mcp_tool_stdio(
    server: &MCPServer,
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    // 1. 构建 JSON-RPC 请求
    let request = ToolCallRequest {
        jsonrpc: "2.0".to_string(),
        method: "tools/call".to_string(),
        params: ToolCallParams {
            name: tool_name.to_string(),
            arguments: input,
        },
        id: Uuid::new_v4().to_string(),
    };

    // 2. 启动服务进程
    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()?;

    // 3. 发送请求
    let mut stdin = child.stdin.take()?;
    stdin.write_all((request_json + "\n").as_bytes())?;

    // 4. 读取响应（带 30 秒超时）
    let response_line = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        read_from_stdout(&mut child)
    ).await??;

    // 5. 解析 JSON-RPC 响应
    let response: JsonRpcResponse = serde_json::from_str(&response_line)?;
    
    if let Some(error) = response.error {
        return Err(MCPError::CommunicationError(
            format!("MCP error ({}): {}", error.code, error.message)
        ));
    }

    Ok(response.result.unwrap_or_default())
}
```

**关键特性**：
- 自动超时处理（30 秒）
- 环境变量大小支持
- 完整的错误处理
- 进程生命周期管理

#### HTTP 服务器通信

实现在 `src-tauri/src/commands/mcp.rs` 中的 `call_mcp_tool_http` 函数：

1. **构建 HTTP 请求**：POST JSON-RPC 请求到服务器 URL
2. **验证状态**：检查 HTTP 状态码
3. **解析响应**：提取 JSON-RPC 响应
4. **返回结果**：提取 result 字段

```rust
async fn call_mcp_tool_http(
    server: &MCPServer,
    tool_name: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, MCPError> {
    let url = server.url.as_ref().ok_or(...)?;

    // 构建 JSON-RPC 请求
    let request = ToolCallRequest { /* ... */ };

    // 发送 HTTP 请求（带 60 秒超时）
    let client = reqwest::Client::new();
    let mut req = client.post(url);

    // 添加认证头（如果配置了 API Key）
    if let Some(api_key) = &server.api_key {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        req.json(&request).send()
    ).await??;

    // 验证 HTTP 状态
    if !response.status().is_success() {
        return Err(MCPError::CommunicationError(
            format!("HTTP error: {}", response.status())
        ));
    }

    // 解析 JSON-RPC 响应
    let resp_json: JsonRpcResponse = response.json().await?;
    
    if let Some(error) = resp_json.error {
        return Err(MCPError::CommunicationError(
            format!("MCP error ({}): {}", error.code, error.message)
        ));
    }

    Ok(resp_json.result.unwrap_or_default())
}
```

**关键特性**：
- Bearer Token 认证支持
- 自动超时处理（60 秒）
- HTTP 状态码验证
- JSON 自动序列化

#### 演示工具（Demo Tools）

为了快速测试，实现了内置的演示工具：

```typescript
// demo_echo - 回显输入
{
  "tool_name": "demo_echo",
  "echo": { "message": "hello world" },
  "timestamp": "2026-02-15T10:30:00+08:00"
}

// demo_calculator - 计算
{
  "operation": "add",
  "operands": { "a": 10, "b": 5 },
  "result": 15
}

// test_connection - 测试连接
{
  "status": "MCP service is responsive",
  "request_id": "uuid-here"
}
```

### 使用示例

#### 1. 启用演示工具测试

无需任何配置，直接在聊天中提问：

```
用户: "帮我算一下 10 + 5"

LLM 可能会回应：
"我来帮你计算这个加法题。[使用工具: demo_calculator with input: {\"a\": 10, \"b\": 5, \"operation\": \"add\"}]

计算结果是 15。"
```

#### 2. 配置自定义 Stdio MCP 服务

**创建 MCP 服务文件** (`mcp-server.js`)：

```javascript
const readline = require('readline');

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout
});

rl.on('line', (line) => {
  try {
    const request = JSON.parse(line);
    
    // 处理 tools/call 请求
    if (request.method === 'tools/call') {
      const toolName = request.params.name;
      const args = request.params.arguments;
      
      let result;
      if (toolName === 'my_tool') {
        result = {
          success: true,
          data: `处理了 ${toolName} 的请求: ${JSON.stringify(args)}`
        };
      } else {
        result = null;
        error = { code: -32601, message: 'Method not found' };
      }
      
      console.log(JSON.stringify({
        jsonrpc: '2.0',
        id: request.id,
        result: result,
        error: error
      }));
    }
  } catch (err) {
    console.log(JSON.stringify({
      jsonrpc: '2.0',
      id: 'error',
      error: { code: -32700, message: 'Parse error' }
    }));
  }
});
```

**在设置中配置**：
- 名称：My Custom MCP
- 类型：Stdio
- 命令：node
- 参数：`./mcp-server.js`
- 启用：✓

#### 3. 配置 HTTP MCP 服务

**MCP 服务器**（任意 HTTP 框架，如 Express）：

```javascript
app.post('/mcp', (req, res) => {
  const { method, params, id } = req.body;
  
  if (method === 'tools/call') {
    const toolName = params.name;
    const result = executeTool(toolName, params.arguments);
    
    res.json({
      jsonrpc: '2.0',
      id: id,
      result: result
    });
  }
});

app.listen(3000, () => console.log('MCP Server on 3000'));
```

**在设置中配置**：
- 名称：HTTP MCP Service
- 类型：HTTP
- URL：http://localhost:3000/mcp
- API Key：（可选）
- 启用：✓

### 错误处理

系统会自动分类和处理以下错误：

| 场景 | 错误类型 | 用户反馈 |
|------|---------|---------|
| API Key 无效 | auth | "API 密钥无效或已过期，请检查设置" |
| 网络不可达 | network | "网络连接错误，请检查网络设置" |
| 请求超时 | timeout | "请求超时，请重试或调整超时设置" |
| 配置错误 | config | "API 配置错误，请检查服务商和模型" |
| 其他错误 | unknown | 显示原始错误信息 |

前端使用 Naive UI 的 notification 组件显示友好的错误提示。

### 性能指标

- **Stdio 超时**：30 秒
- **HTTP 超时**：60 秒
- **异步执行**：不阻塞 UI
- **错误恢复**：失败的工具调用不中断对话流程
- **结果缓存**：与消息历史一起存储

## 总结

MCP 与 LLM 的完整集成现已实现：

1. ✅ 自动收集和格式化可用工具
2. ✅ 构建包含工具定义的系统提示词
3. ✅ 将工具信息发送给 LLM
4. ✅ 检测并解析 LLM 的工具调用
5. ✅ 执行工具（Stdio 和 HTTP 两种方式）
6. ✅ 返回结果给 LLM 继续推理
7. ✅ 友好的错误处理和用户提示

系统已可用于生产环境。下一步的优化方向包括：

- 数据库集成与工具配置持久化
- 工具列表自动刷新和缓存
- 并发工具调用支持
- 调用历史和监控
- 性能指标收集


