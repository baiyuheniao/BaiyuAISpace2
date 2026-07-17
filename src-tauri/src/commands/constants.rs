use std::time::Duration;

// 非流式请求在生成完成前服务器不发任何字节，无法用读间隔超时兜底，
// 只能给足总时长：慢模型的长续答（工具调用后续答、Workspace Agent 轮次）
// 超过 3 分钟很常见。
pub const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
pub const LLM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// 流式请求不能设总超时（长回复会被中途掐断，reqwest 报 "error decoding
// response body"），只限制两次收到数据之间的最大间隔。
pub const LLM_STREAM_READ_TIMEOUT: Duration = Duration::from_secs(180);

// 流式下载（Ollama 模型拉取、安装包下载）同理不能设总超时——下载耗时
// 由文件大小和网速决定，没有安全的上限；只限读间隔，断流才算失败。
pub const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

pub const MCP_STDIO_TIMEOUT: Duration = Duration::from_secs(30);
pub const MCP_HTTP_TIMEOUT: Duration = Duration::from_secs(60);
// 工具真正执行（tools/call）可能是搜索、抓网页、长推理，30 秒不够用；
// tools/list 仍用上面的短超时。
pub const MCP_TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(300);

pub const EMBEDDING_BATCH_DELAY_MS: u64 = 100;

// 服务商返回限流/过载类错误（429/529/"overloaded" 等）时的默认自动重试
// 次数和间隔；用户可在设置页覆盖，未配置时用这两个值兜底。
pub const DEFAULT_LLM_RETRY_COUNT: u32 = 3;
pub const DEFAULT_LLM_RETRY_INTERVAL_SECS: u32 = 2;
