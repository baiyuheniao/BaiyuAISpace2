use std::time::Duration;

pub const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
pub const LLM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub const MCP_STDIO_TIMEOUT: Duration = Duration::from_secs(30);
pub const MCP_HTTP_TIMEOUT: Duration = Duration::from_secs(60);

pub const EMBEDDING_BATCH_DELAY_MS: u64 = 100;
