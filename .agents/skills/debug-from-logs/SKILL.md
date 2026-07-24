---
name: debug-from-logs
description: Diagnose a user-reported BaiyuAISpace2 bug from symptoms and app logs. Use when the user describes runtime misbehavior (报错、消息消失、下载中断、模型答非所问、界面没反应) or drops a log file/error string like "Stream error: ..." — before touching any code.
---

用户报障的固定模式：描述现象（有时附日志或截图），期望先定位根因再谈修复。

## 第一原则：先分清是模型的问题还是代码的问题

用户明确要求过这个顺序。涉及 LLM 输出异常（答非所问、格式错乱、工具不调用）时，
先用软件真实跑一遍、换一个模型对照，确认是代码 bug 再动手改代码。

## 数据都在哪（两个目录名不一样，别找错）

- **日志**：`%APPDATA%\BaiyuAISpace2\logs\app_<date>.log`。
  用户有时会把导出的日志放到 `docs/` 或直接给路径。
- **SQLite 数据库和 WebView2 配置**：`%APPDATA%\com.baiyu.aispace\`
  （来自 tauri.conf.json 的 identifier）。查会话历史、API 配置串台类
  问题直接开 db 看表。
- 复现和界面取证用 `run-baiyuaispace2` skill 的 driver（截图、eval、点按钮）。

## 已知病灶门类（先对号入座再全面排查）

1. **超时误伤**：故障总是卡在某个固定整数时长（30s/60s/300s）→
   先搜自家代码里的 timeout 常量，而不是怀疑网络。流式响应和大文件下载
   **禁用总超时，只用读间隔超时**。"Stream error: error decoding response body"
   曾经就是总超时把流拦腰掐断所致（commit 840b1f0 修过五处）。
2. **配置串台**：切换 API 服务商后仍调用旧配置、新会话挂到旧 API 上。
   查配置读取链路里的缓存/状态残留。
3. **流式解析**：SSE 增量拼接丢工具名、消息渲染完瞬间消失（前端状态被
   覆盖或历史没落库）。前后端各查一半：db 里有没有、渲染层丢没丢。
4. **前端假死**：后台在干活但界面无 loading/无反馈（启动 Ollama、下载
   模型都出过）。这类属于缺状态提示，不是逻辑 bug。

## 排查产出

- 定位到根因后：技术描述 + **一段人话解释**（用户硬性要求），
  说清"什么情况下会踩、为什么会这样"。
- 用户只是在报问题/提问时，先给诊断结论，**等用户发话再修**。
- 修复后：`pnpm tauri build` 验证编译，能复现的场景用 driver 回归一遍。
- 同类问题顺手全局搜一遍（用户习惯说"检查下还有没有同类问题"），
  一次修干净。
