<#
.SYNOPSIS
    Rust RAM Disk Windows 開發環境自動化檢核與安裝腳本 (PowerShell 7+ / pwsh)

.DESCRIPTION
    自動檢查並安裝開發與建置 Rust RAM Disk 所需的系統工具：
    - Rust 穩定版工具鏈 (rustup + cargo + rustc)
    - Microsoft Visual Studio C++ 建置工具鏈 (MSVC link.exe)
    - WinFsp 核心檔案系統代理驅動 (檢查登錄檔，並提供本機離線安裝包引導)
    - LLVM / Clang 工具鏈狀態檢驗 (可選，供進階 bindgen 特性使用)

.PARAMETER RustVersion
    指定期望的 Rust 工具鏈版本 (預設: 1.100.0)

.PARAMETER InstallWinFsp
    若未偵測到 WinFsp，自動叫用 tools/winfsp-installer.msi 啟動安裝精靈

.PARAMETER CheckOnly
    僅執行環境檢核，不啟動任何安裝程式或異動系統設定

.EXAMPLE
    ./scripts/setup-dev-env.ps1
    ./scripts/setup-dev-env.ps1 -CheckOnly
    ./scripts/setup-dev-env.ps1 -InstallWinFsp
#>

[CmdletBinding()]
param(
    [string]$RustVersion = $(if ($env:RUST_VERSION) { $env:RUST_VERSION } else { "1.100.0" }),
    [switch]$InstallWinFsp,
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"

function Write-Info([string]$Message) { Write-Host "[INFO]  $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message)   { Write-Host "[OK]    $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) { Write-Host "[WARN]  $Message" -ForegroundColor Yellow }
function Write-Err([string]$Message)  { Write-Host "[ERR]   $Message" -ForegroundColor Red }

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# 確保目前 Session PATH 包含 Cargo bin
$CargoBinDir = Join-Path $HOME ".cargo/bin"
if (-not (Test-Path $CargoBinDir)) {
    New-Item -ItemType Directory -Force -Path $CargoBinDir | Out-Null
}
$PathSep = [System.IO.Path]::PathSeparator
if (($env:Path -split [regex]::Escape($PathSep)) -notcontains $CargoBinDir) {
    $env:Path = "$CargoBinDir$PathSep$env:Path"
}

Write-Info "=========================================================="
Write-Info "Rust RAM Disk 開發環境檢核與安裝工具"
Write-Info "專案路徑: $ProjectRoot"
Write-Info "目標 Rust 版本基準: $RustVersion"
Write-Info "=========================================================="

# 1. 檢核 Rust 工具鏈 (rustc / cargo / rustup)
Write-Info ">>> [1/5] 檢查 Rust 語言工具鏈..."
$rustc = Get-Command "rustc" -ErrorAction SilentlyContinue
$cargo = Get-Command "cargo" -ErrorAction SilentlyContinue

if ($rustc -and $cargo) {
    $verOutput = (& $rustc.Source --version)
    Write-Ok "Rust 編譯器就緒: $verOutput ($($rustc.Source))"
    $cargoVer = (& $cargo.Source --version)
    Write-Ok "Cargo 套件管理員就緒: $cargoVer"
} else {
    Write-Warn "未於目前 PATH 偵測到完整 Rust 工具鏈！"
    if (-not $CheckOnly) {
        $rustupExe = Join-Path $CargoBinDir "rustup.exe"
        if (Test-Path $rustupExe) {
            Write-Info "偵測到 rustup，正在確認並切換至定版工具鏈 $RustVersion..."
            & $rustupExe toolchain install $RustVersion --profile minimal
            & $rustupExe default $RustVersion
        } else {
            Write-Info "正在自動下載並安裝 rustup 官方工具鏈 (定版: $RustVersion)..."
            $rustupInit = Join-Path $env:TEMP "rustup-init.exe"
            if (-not (Test-Path $rustupInit)) {
                try {
                    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit
                } catch {
                    curl.exe -L -o $rustupInit "https://win.rustup.rs/x86_64"
                }
            }
            if (Test-Path $rustupInit) {
                & $rustupInit -y --default-toolchain $RustVersion --profile minimal
            } else {
                Write-Err "無法下載 rustup-init.exe，請至 https://rustup.rs 手動安裝。"
            }
        }

        # 刷新 Session PATH 並再次檢查
        if (($env:Path -split [regex]::Escape($PathSep)) -notcontains $CargoBinDir) {
            $env:Path = "$CargoBinDir$PathSep$env:Path"
        }
        $rustc = Get-Command "rustc" -ErrorAction SilentlyContinue
        $cargo = Get-Command "cargo" -ErrorAction SilentlyContinue
        if ($rustc -and $cargo) {
            Write-Ok "Rust 工具鏈配置完成: $(& $rustc.Source --version) / $(& $cargo.Source --version)"
        }
    }
}

# 2. 檢核 MSVC C++ 建置工具 (link.exe / VS Build Tools)
Write-Info "`n>>> [2/5] 檢查 Visual Studio C++ 工具鏈 (MSVC Linker)..."
$linker = Get-Command "link.exe" -ErrorAction SilentlyContinue
$vsWhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$hasVcTools = $false

if (Test-Path $vsWhere) {
    $vcInstall = & $vsWhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($vcInstall) {
        $hasVcTools = $true
        Write-Ok "已偵測到 Visual Studio C++ Build Tools: $vcInstall"
    }
}

if ($linker) {
    Write-Ok "MSVC Linker 就緒: $($linker.Source)"
} elseif ($hasVcTools) {
    Write-Ok "Visual Studio C++ 建置環境已就緒 (可透過 MSVC 構建環境或 VS Developer Prompt 使用)。"
} else {
    Write-Warn "未偵測到 Visual Studio C++ Build Tools (link.exe)。"
    Write-Info "若編譯遇到 C++ 連結錯誤，可至 https://visualstudio.microsoft.com/visual-cpp-build-tools/ 安裝 C++ 建置工具。"
}

# 3. 檢核 WiX Toolset 打包工具 (candle.exe + light.exe)
Write-Info "`n>>> [3/5] 檢查 WiX Toolset 安裝套件編譯器 (tools/wix)..."
$wixDir = Join-Path $ProjectRoot "tools\wix"
$candle = Join-Path $wixDir "candle.exe"
$light = Join-Path $wixDir "light.exe"

if ((Test-Path $candle) -and (Test-Path $light)) {
    Write-Ok "WiX Toolset 就緒: $candle / $light"
} else {
    Write-Warn "專案內部未配置 WiX Toolset 二進位檔！"
    if (-not $CheckOnly) {
        Write-Info "正在自動下載並配置 WiX Toolset 3.11 綠色工具包至 tools/wix..."
        if (-not (Test-Path $wixDir)) { New-Item -ItemType Directory -Force -Path $wixDir | Out-Null }
        $wixZip = "$env:TEMP\wix311-binaries.zip"
        if (-not (Test-Path $wixZip)) {
            curl.exe -L -o $wixZip "https://github.com/wixtoolset/wix3/releases/download/wix3112rtm/wix311-binaries.zip"
        }
        Expand-Archive -Path $wixZip -DestinationPath $wixDir -Force
        Write-Ok "WiX Toolset 下載並解壓完成！"
    }
}

# 4. 檢核 WinFsp 核心檔案系統代理驅動
Write-Info "`n>>> [4/5] 檢查 WinFsp (Windows File System Proxy) 驅動與執行環境..."
$winFspInstalled = $false
$regKeys = @(
    "HKLM:\SOFTWARE\WinFsp",
    "HKLM:\SOFTWARE\WOW6432Node\WinFsp"
)
foreach ($key in $regKeys) {
    if (Test-Path $key) {
        $installDir = (Get-ItemProperty $key -ErrorAction SilentlyContinue).InstallDir
        $ver = (Get-ItemProperty $key -ErrorAction SilentlyContinue).Version
        Write-Ok "WinFsp 已安裝! 路徑: $installDir (版本: $ver)"
        $winFspInstalled = $true
        break
    }
}

$installerMsi = Join-Path $ProjectRoot "tools\winfsp-installer.msi"

if (-not $winFspInstalled) {
    Write-Warn "WinFsp 核心驅動尚未安裝於本機！"
    Write-Info "專案內建離線安裝檔位置: $installerMsi"
    
    if ($InstallWinFsp -or (-not $CheckOnly)) {
        if (Test-Path $installerMsi) {
            Write-Info "即將啟動 WinFsp 安裝精靈，請在彈出的視窗中點擊確認以完成驅動註冊..."
            Start-Process msiexec.exe -ArgumentList "/i", "`"$installerMsi`"" -Wait
            
            # 重新檢查
            $recheck = (Test-Path "HKLM:\SOFTWARE\WinFsp") -or (Test-Path "HKLM:\SOFTWARE\WOW6432Node\WinFsp")
            if ($recheck) {
                Write-Ok "WinFsp 安裝完成且已成功註冊至 Windows 系統！"
            } else {
                Write-Warn "WinFsp 尚未完成安裝。若要掛載至檔案總管磁碟機代號，請手動執行 $installerMsi"
            }
        } else {
            Write-Warn "未找到離線安裝包: $installerMsi。可至 https://github.com/winfsp/winfsp/releases 下載。"
        }
    } else {
        Write-Info "如需自動執行安裝，可加上 -InstallWinFsp 旗標執行本腳本。"
    }
}

# 5. 檢核 LLVM / Clang (可選進階項目)
Write-Info "`n>>> [5/5] 檢查 LLVM / Clang 工具鏈狀態 (可選)..."
$clang = Get-Command "clang" -ErrorAction SilentlyContinue
if ($clang) {
    $clangVer = (& $clang.Source --version | Select-Object -First 1)
    Write-Ok "Clang 就緒: $clangVer"
} else {
    Write-Info "本機未安裝獨立 LLVM/Clang (核心 Sparse 引擎與 Systray 已可獨立原生編譯；若欲重新生成 winfsp FFI 綁定才需配置)。"
}

Write-Info "=========================================================="
Write-Ok "環境檢核完成！您可立即執行 ./scripts/build-local.ps1 進行本機建置與測試。"
Write-Info "=========================================================="
