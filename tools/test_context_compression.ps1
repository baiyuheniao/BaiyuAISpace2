# 离线验证上下文压缩后的真实请求边界；不启动桌面应用，也不调用任何模型 API。
# 用法：powershell -ExecutionPolicy Bypass -File .\tools\test_context_compression.ps1

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

Push-Location (Join-Path $repoRoot "src-tauri")
try {
    cargo test context_compaction_replaces_old_history_with_summary -- --nocapture
} finally {
    Pop-Location
}
