# SimpleTranslation 启动脚本
# 自动补全 Rust 工具链环境变量，避免因终端环境陈旧导致 cargo 找不到。
#
# 用法：
#   .\run.ps1            -> npx tauri dev   (开发模式)
#   .\run.ps1 build      -> npx tauri build (打包安装程序)
#   .\run.ps1 <子命令>   -> npx tauri <子命令>

param(
    [string]$Command = "dev"
)

$ErrorActionPreference = "Stop"

$CargoBin = "D:\Rust\cargo\bin"
$CargoHome = "D:\Rust\cargo"
$RustupHome = "D:\Rust\rustup"

if (-not (Test-Path (Join-Path $CargoBin "cargo.exe"))) {
    Write-Host "找不到 cargo：$CargoBin\cargo.exe" -ForegroundColor Red
    Write-Host "请检查 Rust 是否安装在该位置。" -ForegroundColor Red
    exit 1
}

# 只在缺失时补齐，不覆盖已有正确设置
if ($env:PATH -notlike "*$CargoBin*") {
    $env:PATH = "$CargoBin;$env:PATH"
}
if (-not $env:CARGO_HOME) { $env:CARGO_HOME = $CargoHome }
if (-not $env:RUSTUP_HOME) { $env:RUSTUP_HOME = $RustupHome }

Write-Host "cargo: $((Get-Command cargo).Source)" -ForegroundColor DarkGray
Write-Host "执行: npx tauri $Command" -ForegroundColor DarkGray

Push-Location $PSScriptRoot
try {
    & npx tauri $Command
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
