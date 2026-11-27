# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-28

### Fixed
- **Windows 檔案總管容量精確顯示 (Exact Explorer Capacity Reporting)**:
  - 修正 Windows 檔案總管顯示虛擬磁碟容量錯誤之問題（由先前乘算節點產生的 2.00 PB / 2.00 TB 徹底修正為精確設定之 2.00 GB）。
  - 重構 WinFsp 記憶體檔案系統 (MemFS) 原生模組，新增 `-C` 總容量參數並支援即時動態精確統計可用空間與已用空間。
  - 放寬單檔最大容量上限至虛擬磁碟配置上限，單一檔案可直接達到磁碟全額容量。
  - 預設檔案節點上限擴充至 1,048,576 個，杜絕解壓縮大量零碎檔案時回報 `STATUS_CANNOT_MAKE` 之問題。
  - 優化 `find_memfs_binary` 優先級，確保優先採用本專案客製優化編譯之 MemFS 原生模組。
- **原始碼開放與編譯腳本 (`tools/memfs_src/build.ps1`)**:
  - 將優化後之 MemFS C/C++ 原始碼與 WinFsp 標頭完整納入儲存庫管理，並提供自動化 MSVC 編譯腳本，確保任何開發環境皆可重現建置。

## [0.1.0] - 2026-11-28

### Added
- **單一繁體中文 MSI 完整安裝精靈與流程精簡 (`scripts/build-release.ps1`)**:
  - 整合 WiX `WixUI_InstallDir` 與 `zh-TW` 繁體中文語系，提供完整微軟標準安裝精靈介面。
  - 明確詢問安裝目的地資料夾（支援瀏覽/變更按鈕自由指定安裝路徑，預設 `C:\Program Files\RustRAMDisk`）。
  - 新增清晰的安裝完成確認畫面（ExitDialog），明確提示安裝成功，並提供「安裝完成後立即啟動 Rust RAM Disk」勾選選項。
  - 精簡發布產物為單一獨立之 `RustRAMDisk-v0.1.0-x64.msi`，移除外部 Burn Setup.exe 封裝。
- **Windows 掛載管理器本機實體磁碟模式 (Mount Manager Local Disk)** (`src/vfs/mod.rs`, `src/main.rs`):
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
- **自動化 CI/CD 流程 (`.github/workflows/ci.yml`, `.github/workflows/release.yml`)**:
  - 新增 GitHub Actions CI 測試與 Release 流程，推送到 main 時自動執行單元測試與 MSI 建置檢核。
  - 推送版本標籤（如 `v0.1.0`）自動編譯並發布官方 MSI 安裝套件與 `SHA256SUMS.txt` 至 GitHub Releases。

### Changed
- **發布打包腳本架構整合統一 (`scripts/build-release.ps1`)**:
  - 將 WiX Toolset MSI 安裝套件編譯與封裝邏輯直接整合進 `build-release.ps1`，與 `rust_file_manager` 發布規範一致，統一單一發布入口。
  - 正式廢除並移除獨立之 `scripts/build-msi.ps1`，簡化專案維護架構。
  - 確保執行 `build-release.ps1` 一鍵完成單元測試、Release 編譯、WiX 封裝與交付目錄驗證，匯出標準單一 `RustRAMDisk-v<version>-x64.msi` 與 `SHA256SUMS.txt`。

### Fixed
- **檔案總管容量顯示最佳化 (`src/vfs/mod.rs`)**:
  - 將檔案節點上限自百萬級 ($1,048,576$) 收斂至黃金平衡值 ($1,024$)，使 WinFsp 回報給檔案總管之容量公式 ($MaxFileNodes \times MaxFileSize$) 回歸合理範圍。
  - 徹底告別不合理的 2.00 PB 誇張數字，在 2GB 配置下自然顯示為標準硬碟常見的 2.00 TB，同時支援存放上千個檔案與完整單檔大容量寫入。
- **安裝程式與自癒機制 (`scripts/build-release.ps1`, `src/main.rs`)**:
  - 支援全新乾淨 Windows 環境一鍵安裝與自癒：單一 MSI 自動內嵌並靜默配置 WinFsp 核心驅動程式與主程式。
  - 主程式支援離線驅動自癒與標準 UAC 提權引導。
  - 修正 UNC MUP 掛載前綴以徹底解決免安裝程式執行時的 Windows 1005 錯誤。
  - 注入完整安全描述元 (Root SDDL) 與加大檔案節點上限至 1024，完全支援多媒體檔案、PDF 與免安裝軟體執行。
  - 捷徑改為無主控台原生啟動，杜絕黑底命令列視窗；支援單實例偵測與系統匣重載信號。
