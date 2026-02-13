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

## 总结

MCP与LLM的集成现已在前端完全就位。系统可以：

1. ✅ 自动收集和格式化可用工具
2. ✅ 构建包含工具定义的系统提示词
3. ✅ 将工具信息发送给LLM
4. ⏳ 解析LLM的工具调用请求（下一步）
5. ⏳ 执行工具并返回结果（下一步）

下一步的优先顺序应该是实现函数调用响应处理，这将解锁完整的工具执行能力。
