# BaiyuAISpace 依赖安装脚本（国内镜像加速）
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  BaiyuAISpace 依赖安装" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 检查 pnpm
Write-Host "[1/3] 检查 pnpm..." -ForegroundColor Yellow
$pnpm = Get-Command pnpm -ErrorAction SilentlyContinue
if (-not $pnpm) {
    Write-Host "正在安装 pnpm..." -ForegroundColor Yellow
    npm install -g pnpm --registry=https://registry.npmmirror.com
}
Write-Host "✓ pnpm 已就绪" -ForegroundColor Green
Write-Host ""

# 安装前端依赖
Write-Host "[2/3] 安装前端依赖 (使用淘宝镜像)..." -ForegroundColor Yellow
Write-Host "这可能需要几分钟..." -ForegroundColor Gray
pnpm install
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 前端依赖安装失败" -ForegroundColor Red
    exit 1
}
Write-Host "✓ 前端依赖安装完成" -ForegroundColor Green
Write-Host ""

# 安装 Rust 依赖
Write-Host "[3/3] 安装 Rust 依赖 (使用 rsproxy 镜像)..." -ForegroundColor Yellow
Write-Host "首次编译需要 3-5 分钟，请耐心等待..." -ForegroundColor Gray
Push-Location src-tauri
cargo build
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ Rust 依赖安装失败" -ForegroundColor Red
    Pop-Location
    exit 1
}
Pop-Location
Write-Host "✓ Rust 依赖安装完成" -ForegroundColor Green
Write-Host ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  所有依赖安装完成！" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "现在可以运行开发服务器：" -ForegroundColor Yellow
Write-Host "  pnpm tauri dev" -ForegroundColor White
Write-Host ""
Read-Host "按回车键退出"
