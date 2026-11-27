<#
.SYNOPSIS
    Rust RAM Disk Windows MSI 安裝套件打包腳本 (Build MSI Installer via WiX Toolset)

.DESCRIPTION
    自動使用專案內建的 WiX Toolset (candle.exe + light.exe) 編譯出符合微軟標準的 Windows .msi 安裝套件：
    - 將核心主程式 (rust_ram_disk.exe)
    - 記憶體檔案系統代理 (memfs-x64.exe)
    - 核心動態庫 (winfsp-x64.dll)
    - 官方驅動安裝程式 (winfsp-installer.msi)
    - 一鍵啟動批次檔 (start-ramdisk.bat)
    - 說明文件 (README.md)
    打包至獨立 MSI 安裝套件中，並自動配置「開始功能表捷徑」與「桌面捷徑」。

.PARAMETER Version
    指定安裝套件版本號 (預設讀取 Cargo.toml，例如 0.1.0)

.EXAMPLE
    ./scripts/build-msi.ps1
#>

[CmdletBinding()]
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"

function Write-Info([string]$Message) { Write-Host "[INFO]  $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message)   { Write-Host "[OK]    $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) { Write-Host "[WARN]  $Message" -ForegroundColor Yellow }

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
$OutDir = Join-Path $ProjectRoot "release-artifacts"
$ToolsDir = Join-Path $ProjectRoot "tools"
$WixDir = Join-Path $ToolsDir "wix"

# 1. 檢查版號
if ([string]::IsNullOrWhiteSpace($Version)) {
    $CargoToml = Join-Path $ProjectRoot "Cargo.toml"
    if (Test-Path $CargoToml) {
        $content = Get-Content $CargoToml -Raw
        if ($content -match 'version\s*=\s*"([^"]+)"') {
            $Version = $matches[1]
        }
    }
}
if ([string]::IsNullOrWhiteSpace($Version)) { $Version = "0.1.0" }

Write-Info "=========================================================="
Write-Info "Rust RAM Disk MSI 安裝套件建置工具 (v$Version)"
Write-Info "專案路徑: $ProjectRoot"
Write-Info "=========================================================="

# 2. 檢核必要工具 (WiX Toolset)
$Candle = Join-Path $WixDir "candle.exe"
$Light = Join-Path $WixDir "light.exe"

if ((-not (Test-Path $Candle)) -or (-not (Test-Path $Light))) {
    Write-Info "正在自動配置 WiX Toolset 編譯工具鏈至 tools/wix..."
    if (-not (Test-Path $WixDir)) { New-Item -ItemType Directory -Force -Path $WixDir | Out-Null }
    $wixZip = "$env:TEMP\wix311-binaries.zip"
    curl.exe -L -o $wixZip "https://github.com/wixtoolset/wix3/releases/download/wix3112rtm/wix311-binaries.zip"
    Expand-Archive -Path $wixZip -DestinationPath $WixDir -Force
}

Write-Ok "WiX 編譯器就緒: $Candle"
Write-Ok "WiX 連結器就緒: $Light"

# 3. 確保 Release 產物目錄與檔案完整
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

$StagingDir = Join-Path $env:TEMP "rust_ram_disk_msi_staging"
if (Test-Path $StagingDir) { Remove-Item -Recurse -Force $StagingDir }
New-Item -ItemType Directory -Force -Path $StagingDir | Out-Null

$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path $CargoBin) {
    $env:PATH = "$CargoBin;$env:PATH"
}

$BuiltExe = Join-Path $ProjectRoot "target\release\rust_ram_disk.exe"
Write-Info "正在編譯最新 Release 執行檔..."
Set-Location $ProjectRoot
cargo build --release --quiet

# 確保 winfsp_bin 存在 (若無則自 winfsp-installer.msi 自動抽取)
$WinfspBinDir = Join-Path $ProjectRoot "tools\winfsp_bin"
$MemfsBin = Join-Path $WinfspBinDir "memfs-x64.exe"
if (-not (Test-Path $MemfsBin)) {
    $InstallerMsi = Join-Path $ProjectRoot "tools\winfsp-installer.msi"
    if (Test-Path $InstallerMsi) {
        Write-Info "正在自 winfsp-installer.msi 提取配套執行檔..."
        $tempExtract = Join-Path $env:TEMP "winfsp_extract_auto"
        Start-Process msiexec.exe -ArgumentList "/a", "`"$InstallerMsi`"", "/qn", "`"TARGETDIR=$tempExtract`"" -Wait
        $srcDir = Join-Path $tempExtract "DYNAMIC\SxS\DYNAMIC\bin"
        if (Test-Path $srcDir) {
            if (-not (Test-Path $WinfspBinDir)) { New-Item -ItemType Directory -Force -Path $WinfspBinDir | Out-Null }
            Copy-Item (Join-Path $srcDir "memfs-x64.exe") $WinfspBinDir -Force
            Copy-Item (Join-Path $srcDir "winfsp-x64.dll") $WinfspBinDir -Force
            Copy-Item (Join-Path $srcDir "winfsp-x64.sys") $WinfspBinDir -Force
        }
        Remove-Item -Recurse -Force $tempExtract -ErrorAction SilentlyContinue
    }
}

# 複製打包必備檔案至臨時分區 (Staging)
Copy-Item $BuiltExe (Join-Path $StagingDir "rust_ram_disk.exe") -Force
Copy-Item (Join-Path $WinfspBinDir "memfs-x64.exe") (Join-Path $StagingDir "memfs-x64.exe") -Force
Copy-Item (Join-Path $WinfspBinDir "winfsp-x64.dll") (Join-Path $StagingDir "winfsp-x64.dll") -Force
Copy-Item (Join-Path $ProjectRoot "tools\winfsp-installer.msi") (Join-Path $StagingDir "winfsp-installer.msi") -Force
Copy-Item (Join-Path $ProjectRoot "README.md") (Join-Path $StagingDir "README.md") -Force

# 建立 start-ramdisk.bat
$StartBat = @"
@echo off
cd /d "%~dp0"
echo Starting Rust RAM Disk (2GB, Mount R:, LZ4 Compression, Systray)...
start "" "%~dp0rust_ram_disk.exe" mount -s 2G -m R: -c lz4 --tray
"@
$StartBat | Out-File -FilePath (Join-Path $StagingDir "start-ramdisk.bat") -Encoding ascii -Force

Write-Ok "已收集所有安裝套件組件至臨時目錄: $StagingDir"

# 4. 生成 WiX 原始碼 (RustRAMDisk.wxs)
$WxsPath = Join-Path $StagingDir "RustRAMDisk.wxs"
$WixXml = @"
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product Id="*" 
           Name="Rust RAM Disk" 
           Language="1028" 
           Codepage="950"
           Version="$Version.0" 
           Manufacturer="William Wang" 
           UpgradeCode="{E7D8A90B-4F12-4B6C-8D3E-9A5C7B1E3F5A}">
           
    <Package InstallerVersion="300" 
             Compressed="yes" 
             InstallScope="perMachine" 
             Platform="x64" 
             Description="High-Performance Rust RAM Disk with LZ4 Compression and Systray Integration" />

    <MajorUpgrade DowngradeErrorMessage="A newer version of [ProductName] is already installed." />
    <MediaTemplate EmbedCab="yes" />

    <Directory Id="TARGETDIR" Name="SourceDir">
      <!-- 64-bit Program Files -->
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLFOLDER" Name="RustRAMDisk" />
      </Directory>

      <!-- Start Menu Programs -->
      <Directory Id="ProgramMenuFolder">
        <Directory Id="ApplicationProgramsFolder" Name="Rust RAM Disk" />
      </Directory>

      <!-- Desktop -->
      <Directory Id="DesktopFolder" Name="Desktop" />
    </Directory>

    <!-- Installation Components -->
    <DirectoryRef Id="INSTALLFOLDER">
      <Component Id="MainExecutable" Guid="{B1A2C3D4-1111-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="RustRamDiskExe" Source="$StagingDir\rust_ram_disk.exe" KeyPath="yes" />
      </Component>

      <Component Id="MemFsExe" Guid="{B1A2C3D4-2222-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="MemFsExeFile" Source="$StagingDir\memfs-x64.exe" KeyPath="yes" />
      </Component>

      <Component Id="WinFspDll" Guid="{B1A2C3D4-3333-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="WinFspDllFile" Source="$StagingDir\winfsp-x64.dll" KeyPath="yes" />
      </Component>

      <Component Id="WinFspInstaller" Guid="{B1A2C3D4-4444-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="WinFspMsi" Source="$StagingDir\winfsp-installer.msi" KeyPath="yes" />
      </Component>

      <Component Id="StartBatch" Guid="{B1A2C3D4-5555-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="StartBatchFile" Source="$StagingDir\start-ramdisk.bat" KeyPath="yes" />
      </Component>

      <Component Id="ReadmeFile" Guid="{B1A2C3D4-6666-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <File Id="ReadmeDoc" Source="$StagingDir\README.md" KeyPath="yes" />
      </Component>

      <!-- WinFsp Mount Manager Kernel-mode Registration -->
      <Component Id="MountManagerConfig" Guid="{F3E2D1C0-9999-4A5B-8C7D-0E1F2A3B4C5D}" Win64="yes">
        <RegistryValue Root="HKLM" Key="SOFTWARE\WinFsp" Name="MountUseMountmgrFromFSD" Type="integer" Value="1" KeyPath="yes" />
        <RegistryValue Root="HKLM" Key="SOFTWARE\WOW6432Node\WinFsp" Name="MountUseMountmgrFromFSD" Type="integer" Value="1" />
      </Component>
    </DirectoryRef>

    <!-- Shortcuts -->
    <DirectoryRef Id="ApplicationProgramsFolder">
      <Component Id="ApplicationShortcuts" Guid="{C1D2E3F4-7777-4A5B-8C7D-0E1F2A3B4C5D}">
        <Shortcut Id="AppStartMenuShortcut"
                  Name="Rust RAM Disk"
                  Description="啟動 Rust RAM Disk (預設 2GB R: 槽，啟用 LZ4 透明壓縮與系統匣)"
                  Target="[INSTALLFOLDER]rust_ram_disk.exe"
                  Arguments="mount -s 2G -m R: -c lz4 --tray"
                  WorkingDirectory="INSTALLFOLDER" />
                  
        <Shortcut Id="InstallDriverShortcut"
                  Name="安裝或修復 WinFsp 核心驅動"
                  Description="安裝或更新 WinFsp 核心驅動程式 (winfsp-installer.msi)"
                  Target="[SystemFolder]msiexec.exe"
                  Arguments="/i &quot;[INSTALLFOLDER]winfsp-installer.msi&quot;"
                  WorkingDirectory="INSTALLFOLDER" />

        <RemoveFolder Id="CleanUpShortcuts" On="uninstall" />
        <RegistryValue Root="HKCU" Key="Software\RustRAMDisk" Name="installed" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>

    <DirectoryRef Id="DesktopFolder">
      <Component Id="DesktopShortcutComponent" Guid="{C1D2E3F4-8888-4A5B-8C7D-0E1F2A3B4C5D}">
        <Shortcut Id="AppDesktopShortcut"
                  Name="Rust RAM Disk"
                  Description="啟動 Rust RAM Disk"
                  Target="[INSTALLFOLDER]rust_ram_disk.exe"
                  Arguments="mount -s 2G -m R: -c lz4 --tray"
                  WorkingDirectory="INSTALLFOLDER" />
        <RegistryValue Root="HKCU" Key="Software\RustRAMDisk" Name="desktop_shortcut" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>

    <!-- Features -->
    <Feature Id="MainFeature" Title="Rust RAM Disk" Level="1">
      <ComponentRef Id="MainExecutable" />
      <ComponentRef Id="MemFsExe" />
      <ComponentRef Id="WinFspDll" />
      <ComponentRef Id="WinFspInstaller" />
      <ComponentRef Id="StartBatch" />
      <ComponentRef Id="ReadmeFile" />
      <ComponentRef Id="ApplicationShortcuts" />
      <ComponentRef Id="DesktopShortcutComponent" />
      <ComponentRef Id="MountManagerConfig" />
    </Feature>

    <!-- User Interface Configuration: WixUI_InstallDir (詢問安裝路徑與顯示安裝完成畫面) -->
    <Property Id="WIXUI_INSTALLDIR" Value="INSTALLFOLDER" />
    <Property Id="WIXUI_EXITDIALOGOPTIONALCHECKBOXTEXT" Value="安裝完成後立即啟動 Rust RAM Disk (掛載 R: 槽並常駐系統匣)" />
    <Property Id="WIXUI_EXITDIALOGOPTIONALCHECKBOX" Value="1" />

    <UI>
      <UIRef Id="WixUI_InstallDir" />
      <!-- 跳過多餘授權協議頁面，直接從歡迎頁進入安裝路徑選擇頁 (InstallDirDlg) -->
      <Publish Dialog="WelcomeDlg" Control="Next" Event="NewDialog" Value="InstallDirDlg">NOT Installed</Publish>
      <Publish Dialog="InstallDirDlg" Control="Back" Event="NewDialog" Value="WelcomeDlg">1</Publish>

      <!-- 於安裝完成頁 (ExitDialog) 點選完成時，依勾選狀態啟動應用程式 -->
      <Publish Dialog="ExitDialog"
               Control="Finish" 
               Event="DoAction" 
               Value="LaunchApp" 
               Order="1">WIXUI_EXITDIALOGOPTIONALCHECKBOX = 1 and NOT Installed</Publish>
    </UI>

    <!-- Post-installation Launch Action -->
    <CustomAction Id="LaunchApp" 
                  Directory="INSTALLFOLDER" 
                  ExeCommand="&quot;[INSTALLFOLDER]rust_ram_disk.exe&quot; mount -s 2G -m R: -c lz4 --tray" 
                  Execute="immediate" 
                  Return="asyncNoWait" />

    <!-- 針對命令列靜默安裝 (UILevel < 5) 自動執行啟動 -->
    <InstallExecuteSequence>
      <Custom Action="LaunchApp" After="InstallFinalize">NOT Installed AND UILevel &lt; 5</Custom>
    </InstallExecuteSequence>
  </Product>
</Wix>
"@

$WixXml | Out-File -FilePath $WxsPath -Encoding utf8 -Force
Write-Ok "已產生 WiX 設定規格書: $WxsPath"

# 5. 編譯與連結 MSI (啟用 WixUIExtension)
Write-Info ">>> [1/2] 執行 WiX 編譯 (candle.exe)..."
$WixObj = Join-Path $StagingDir "RustRAMDisk.wixobj"
& $Candle -arch x64 -nologo -ext WixUIExtension "$WxsPath" -out "$WixObj"
if ($LASTEXITCODE -ne 0) { throw "WiX candle 編譯失敗！" }

Write-Info ">>> [2/2] 執行 WiX 連結 (light.exe)..."
$OutputMsi = Join-Path $OutDir "RustRAMDisk-v$Version-x64.msi"
$OutputGenericMsi = Join-Path $OutDir "RustRAMDisk.msi"

& $Light -nologo -ext WixUIExtension -cultures:zh-TW -sice:ICE91 -sice:ICE60 "$WixObj" -out "$OutputMsi"
if ($LASTEXITCODE -ne 0) { throw "WiX light 連結失敗！" }

Copy-Item $OutputMsi $OutputGenericMsi -Force

# 6. 計算 SHA-256
$Hash = (Get-FileHash -Path $OutputMsi -Algorithm SHA256).Hash.ToLower()
Write-Info "MSI SHA-256: $Hash"

Write-Info "=========================================================="
Write-Ok "🎉 單一 MSI 安裝套件封裝圓滿成功！"
Write-Ok "MSI 安裝檔: $OutputMsi"
Write-Ok "MSI 通用檔: $OutputGenericMsi"
Write-Ok "檔案大小: $((Get-Item $OutputMsi).Length / 1MB | ForEach-Object { '{0:N2} MB' -f $_ })"
Write-Info "=========================================================="
