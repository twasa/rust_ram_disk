<#
.SYNOPSIS
    Rust RAM Disk 自動化發布打包腳本 (Build Release & Export Artifacts)

.DESCRIPTION
    自動執行完整測試、Cargo Release 編譯最佳化，並調用 WiX 封裝 Windows 系統原生單一 MSI 安裝套件：
    1. 具完整安裝精靈與版號之單一 MSI 安裝套件 (RustRAMDisk-v<version>-x64.msi)
    2. SHA-256 數位指紋校驗表 (SHA256SUMS.txt)
    統一輸出至專案根目錄下的 release-artifacts/ 目錄。

.PARAMETER SkipTests
    略過單元測試步驟，直接進行 Release 編譯

.PARAMETER WithWinFsp
    啟用 winfsp-mount 特性進行編譯 (需要本機已安裝 WinFsp 與可用的 bindgen/libclang)

.EXAMPLE
    ./scripts/build-release.ps1
    ./scripts/build-release.ps1 -SkipTests
#>

[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$WithWinFsp
)

$ErrorActionPreference = "Stop"

function Write-Info([string]$Message) { Write-Host "[INFO]  $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message)   { Write-Host "[OK]    $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) { Write-Host "[WARN]  $Message" -ForegroundColor Yellow }

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$OutDir = Join-Path $ProjectRoot "release-artifacts"

# 確保 PATH 包含 Cargo bin
$CargoBinDir = Join-Path $HOME ".cargo/bin"
if (Test-Path $CargoBinDir) {
    $PathSep = [System.IO.Path]::PathSeparator
    if (($env:Path -split [regex]::Escape($PathSep)) -notcontains $CargoBinDir) {
        $env:Path = "$CargoBinDir$PathSep$env:Path"
    }
}

# 讀取 Cargo.toml 版號
$CargoTomlPath = Join-Path $ProjectRoot "Cargo.toml"
$Version = "0.1.0"
if (Test-Path $CargoTomlPath) {
    $content = Get-Content $CargoTomlPath -Raw
    if ($content -match 'version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
    }
}

Write-Info "=========================================================="
Write-Info "Rust RAM Disk 發布打包工具 (v$Version)"
Write-Info "專案路徑: $ProjectRoot"
Write-Info "輸出目錄: $OutDir"
Write-Info "=========================================================="

if (Test-Path $OutDir) {
    Get-ChildItem -Path $OutDir -Recurse | Remove-Item -Force -Recurse -ErrorAction SilentlyContinue
} else {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

Set-Location $ProjectRoot

# 1. 執行單元測試
if (-not $SkipTests) {
    Write-Info ">>> [1/3] 執行儲存引擎與快照單元測試 (cargo test -q)..."
    & cargo test -q
    if ($LASTEXITCODE -ne 0) {
        throw "單元測試未通過，打包終止！"
    }
    Write-Ok "所有單元測試完全通過！"
} else {
    Write-Warn ">>> [1/3] 已略過單元測試 (-SkipTests)"
}

# 2. 執行 Cargo Release 編譯
Write-Info "`n>>> [2/3] 編譯 Release 最佳化二進位檔..."
$cargoArgs = @("build", "--release", "-q")
if ($WithWinFsp) {
    $cargoArgs += @("--features", "winfsp-mount")
    Write-Info "已啟用特性: winfsp-mount"
}

& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) {
    throw "Cargo Release 編譯失敗！"
}

$BuiltExe = Join-Path $ProjectRoot "target\release\rust_ram_disk.exe"
if (-not (Test-Path $BuiltExe)) {
    throw "未找到編譯產物: $BuiltExe"
}

# 3. 呼叫 build-msi.ps1 封裝 Windows .msi 安裝套件
Write-Info "`n>>> [3/4] 呼叫 build-msi.ps1 封裝官方 MSI 安裝套件..."
$BuildMsiScript = Join-Path $ScriptDir "build-msi.ps1"
if (Test-Path $BuildMsiScript) {
    & $BuildMsiScript -Version $Version
}

# 4. 驗證交付產物並計算 SHA-256
Write-Info "`n>>> [4/4] 驗證交付產物與計算校驗碼至 release-artifacts/..."

# 確保 release-artifacts 僅保留單一 MSI 安裝套件，清理非必要產物 (如原始二進位檔或除錯符號)
Get-ChildItem $OutDir -File | Where-Object { $_.Extension -ne ".msi" -and $_.Name -ne "SHA256SUMS.txt" } | Remove-Item -Force -ErrorAction SilentlyContinue
Get-Item (Join-Path $OutDir "RustRAMDisk.msi") -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

# 計算校驗碼
Write-Info "計算 SHA-256 校驗碼清單..."
$ChecksumLines = @()
Get-ChildItem $OutDir -File | Where-Object { $_.Name -ne "SHA256SUMS.txt" } | ForEach-Object {
    $hash = (Get-FileHash -Path $_.FullName -Algorithm SHA256).Hash.ToLower()
    $line = "$hash  $($_.Name)"
    $ChecksumLines += $line
}
$ChecksumFile = Join-Path $OutDir "SHA256SUMS.txt"
$ChecksumLines | Out-File -FilePath $ChecksumFile -Encoding utf8 -Force
Write-Ok "已產生 SHA256SUMS.txt"

Write-Info "=========================================================="
Write-Ok "打包交付圓滿完成！產物清單 ($OutDir):"
Get-ChildItem $OutDir | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String | Write-Host -ForegroundColor Green
Write-Info "=========================================================="
