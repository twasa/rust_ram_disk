<#
.SYNOPSIS
    Rust RAM Disk 本地建置與部署腳本 (Build & Deploy to Local RAMDISK_HOME)

.DESCRIPTION
    編譯最新 Release 二進位檔，並部署至本機工具目錄 ($env:RAMDISK_HOME 或預設 $HOME\RAMDisk)。
    同時產生一鍵啟動腳本 (start-ramdisk.bat) 與效能測試捷徑，方便日常一鍵啟動常駐。

.PARAMETER Destination
    手動指定目標部署目錄。若未指定，則優先讀取 $env:RAMDISK_HOME 環境變數；預設為 $HOME\RAMDisk。

.PARAMETER Clean
    在部署前清空目標目錄中的舊版產物。

.PARAMETER RunBenchmark
    部署完成後立即執行記憶體儲存引擎之基準效能測試。

.EXAMPLE
    ./scripts/build-local.ps1
    ./scripts/build-local.ps1 -Clean -RunBenchmark
    ./scripts/build-local.ps1 -Destination "D:\Tools\RAMDisk"
#>

[CmdletBinding()]
param(
    [string]$Destination = "",
    [switch]$Clean,
    [switch]$RunBenchmark
)

$ErrorActionPreference = "Stop"

function Write-Info([string]$Message) { Write-Host "[INFO]  $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message)   { Write-Host "[OK]    $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) { Write-Host "[WARN]  $Message" -ForegroundColor Yellow }

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# 決定目標部署目錄
$TargetDir = if (-not [string]::IsNullOrWhiteSpace($Destination)) {
    $Destination
} elseif (-not [string]::IsNullOrWhiteSpace($env:RAMDISK_HOME)) {
    $env:RAMDISK_HOME
} else {
    Join-Path $HOME "RAMDisk"
}

Write-Info "=========================================================="
Write-Info "Rust RAM Disk 本地建置與一鍵部署工具 (build-local)"
Write-Info "專案路徑: $ProjectRoot"
Write-Info "目標部署目錄: $TargetDir"
Write-Info "=========================================================="

if (-not (Test-Path $TargetDir)) {
    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
    Write-Info "已建立部署目錄: $TargetDir"
}

if ($Clean) {
    Write-Info "正在清理部署目錄中的舊版檔案..."
    Get-ChildItem -Path $TargetDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
}

# 1. 執行 Release 編譯
Write-Info ">>> [1/2] 呼叫 build-release.ps1 編譯主程式..."
$BuildReleaseScript = Join-Path $ScriptDir "build-release.ps1"
& $BuildReleaseScript -SkipTests

# 2. 部署主體與便利執行腳本至目標目錄
Write-Info "`n>>> [2/2] 部署檔案至 $TargetDir..."

$SourceExe = Join-Path $ProjectRoot "release-artifacts\rust_ram_disk.exe"
$DestExe = Join-Path $TargetDir "rust_ram_disk.exe"
Copy-Item $SourceExe $DestExe -Force
Write-Ok "已部署核心執行檔: $DestExe"

# 複製驅動安裝檔以備不時之需
$SourceMsi = Join-Path $ProjectRoot "tools\winfsp-installer.msi"
if (Test-Path $SourceMsi) {
    Copy-Item $SourceMsi (Join-Path $TargetDir "winfsp-installer.msi") -Force
    Write-Ok "已同步複製驅動安裝套件"
}

# 產生一鍵啟動系統匣的 Windows Batch 捷徑
$StartBatchContent = @"
@echo off
cd /d "%~dp0"
echo Starting Rust RAM Disk (Systray Monitor & Windows Service)...
start "" "%~dp0rust_ram_disk.exe"
"@
$StartBatchPath = Join-Path $TargetDir "start-ramdisk.bat"
$StartBatchContent | Out-File -FilePath $StartBatchPath -Encoding ascii -Force
Write-Ok "已產生一鍵啟動捷徑: $StartBatchPath"

# 產生一鍵效能測試的 Windows Batch 捷徑
$BenchBatchContent = @"
@echo off
cd /d "%~dp0"
echo Running Rust RAM Disk In-Memory Benchmark...
"%~dp0rust_ram_disk.exe" benchmark -s 512M -c lz4
pause
"@
$BenchBatchPath = Join-Path $TargetDir "run-benchmark.bat"
$BenchBatchContent | Out-File -FilePath $BenchBatchPath -Encoding ascii -Force
Write-Ok "已產生一鍵效能測試捷徑: $BenchBatchPath"

Write-Info "=========================================================="
Write-Ok "本地部署圓滿完成！您可隨時執行 $StartBatchPath 啟動 RAM Disk。"
Write-Info "=========================================================="

if ($RunBenchmark) {
    Write-Info "`n>>> 執行部署後驗證效能測試..."
    & $DestExe benchmark -s 512M -c lz4
}
