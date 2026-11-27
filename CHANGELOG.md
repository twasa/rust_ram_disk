# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Windows 掛載管理器本機實體磁碟模式 (Mount Manager Local Disk)** (`src/vfs/mod.rs`, `src/main.rs`, `scripts/build-msi.ps1`):
  - 徹底移除 UNC 網路重定向標籤，改採 `\\.\<代號>` 語法向 Windows 核心掛載管理器 (`\Device\MountPointManager`) 註冊全域磁碟區。
  - 檔案總管顯示為標準本機硬碟圖示 (`DRIVE_FIXED`)，不再呈現為網路磁碟機。
  - 解決 Windows 1005 (`ERROR_UNRECOGNIZED_VOLUME`) 錯誤，各類免安裝可攜式程式 (Portable `.exe`) 直接雙擊執行零報錯。
  - 內建 Windows 管理員權限檢測與友善 UAC 自動提權重啟對話框。
  - MSI 安裝套件自動於 `HKLM\SOFTWARE\WinFsp` 註冊 `MountUseMountmgrFromFSD` 旗標以強化全域磁碟協同。
- **原生設定視窗 (Settings UI)** (`src/ui/settings.rs`):
  - 純 Win32 原生對話框介面（零第三方 GUI 依賴、微秒級啟動）。
  - 支援從系統匣圖示左鍵點擊、雙擊或右鍵選單「⚙️ 磁碟設定」直接叫出。
  - 支援動態熱重載與線上重新配置：磁碟機代號 (`R:`, `S:`, `T:` 等)、記憶體容量 (`512M` 至 `64G` 或自訂)、壓縮模式 (`LZ4` / `None`)、快照映像檔路徑瀏覽 (`GetSaveFileNameW`) 與「隨 Windows 開機自動背景掛載 (`HKCU\...\Run`)」。
  - 點擊「套用並重新掛載」自動回存現有快照、卸載舊磁碟並以新設定重新掛載，同時更新系統匣狀態與資訊提示。
- **自動化環境建置升級 (`scripts/setup-dev-env.ps1`)**:
  - 新增在無 `winget` 環境下自動下載並靜默安裝官方 `rustup-init.exe` 之備援機制。
  - 新增 Visual Studio C++ Build Tools (`vswhere.exe`) 檢核。
  - 新增 WiX Toolset 3.11 專案自動下載與配置支援。

### Fixed
- **安裝程式與自癒機制 (`scripts/build-msi.ps1`, `src/main.rs`)**:
  - 支援全新乾淨 Windows 環境一鍵安裝與自癒：WiX Burn 引導安裝包 (`RustRAMDiskSetup.exe`) 自動鏈結安裝 WinFsp 核心驅動程式與主程式。
  - 主程式支援離線驅動自癒與標準 UAC 提權引導。
  - 修正 UNC MUP 掛載前綴以徹底解決免安裝程式執行時的 Windows 1005 錯誤。
  - 注入完整安全描述元 (Root SDDL) 與加大檔案節點上限至 1048576，完全支援多媒體檔案、PDF 與程式碼預覽。
  - 捷徑改為無主控台原生啟動，杜絕黑底命令列視窗；支援單實例偵測與系統匣重載信號。
