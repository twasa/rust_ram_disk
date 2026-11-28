# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Windows 系統服務常駐生命週期架構 (Windows System Service Architecture)** (`src/service/mod.rs`):
  - 核心 RAM Disk 由獨立背景 Windows 服務 (`RustRamDiskService`) 代管，隨系統開機自動於背景掛載為本機全域實體硬碟 (`DRIVE_FIXED`)。
  - 支援 SCM 服務控制管理員完整指令：`service install`（註冊並啟動）、`service uninstall`（停止並移除）、`service start`、`service stop`、`service restart`、`service status`、`service snapshot` 與 `service run`。
  - 全域配置保存於 `%ProgramData%\RustRamDisk\config.json`，支援 Session 0 與使用者會話無障礙跨程序存取。
- **UI 與服務徹底解耦之系統匣常駐監控 (Systray Service Monitor & Decoupled Lifecycle)** (`src/main.rs`, `src/tray/mod.rs`):
  - 執行主程式預設開啟右下角 Systray 監控面板；當使用者點擊「❌ 關閉監控面板 (Exit Tray)」時，僅關閉 UI 面板，**RAM Disk 虛擬磁碟持續跟隨服務在背景常駐，絕對不會因為 UI 關閉而消失**！
  - 系統匣圖示狀態即時感知：服務運行中呈現青藍色圖示，服務停止/離線時呈現橘黃色圖示。
  - 右鍵選單支援「▶️ 啟動系統服務」、「⏹️ 停止系統服務」、「🔄 重新啟動服務」與「💾 立即儲存快照」。
- **原生 Win32 設定視窗升級服務整合管理 (Integrated Service Management in Settings UI)** (`src/ui/settings.rs`):
  - 設定畫面新增「Windows 系統服務管理」控制區塊，即時顯示服務狀態燈號與說明。
  - 內建「🚀 註冊並啟動服務」、「▶️ 啟動服務」、「⏹️ 停止服務」與「🗑️ 卸載服務」一鍵快捷控制。
  - 動態套用設定後自動重啟服務並平滑重新掛載磁碟。

## [0.1.4] - 2026-11-28

### Fixed
- **修復 WinFsp 記憶體檔案系統原生模組崩潰 (Fix MemFS Crash in USER32.dll `wvsprintfW`)**:
  - 排查根本原因：MemFS 原生模組在調用 WinFsp `FspServiceLog` 輸出日誌時，底層調用了 Windows `USER32.dll` 的 `wvsprintfW`；然而 `wvsprintfW` 不支援 64 位元之 `%llu` 格式化符號（僅支援 `%I64u`），導致 64 位元整數被當作 32 位元讀取，致使堆疊參數偏移 4 個位元組，後續 `%s` 字串參數讀取到錯誤指標而觸發 Access Violation `0xC0000005` 崩潰。
  - 將 `%llu` 全面修正為相容的 `%I64u`，並修復 `OtherFlags` 位元遮罩覆蓋問題，重新編譯並交付新版 `memfs-x64.exe`，徹底根絕 MemFS 剛啟動即閃退的重大缺陷。

### Added
- **參考 RFM 打造完整結構化日誌系統 (RFM-Grade Structured Logging & Panic Hook)** (`src/logger.rs`, `src/main.rs`):
  - 引入與 Rust File Manager (RFM) 相同標準之結構化日誌架構，支援 JSON Lines (`.jsonl`) 與文字格式。
  - 自動於 `%LOCALAPPDATA%\RustRAMDisk\logs\rust_ram_disk.jsonl` 保存執行歷程，並內建 5 MB 自動輪替機制。
  - 安裝全域 Panic 攔截器 (`install_panic_hook`)，未預期錯誤皆能完整留存堆疊與執行脈絡。
  - 追蹤記錄應用程式完整生命週期：PID、命令列參數、UAC 提權請求、WinFsp 核心驅動偵測結果、掛載參數、進程狀態與正常/異常退出原因。
- **系統匣整合「查看運行日誌」選單與異常退出告警 (Tray Log Viewer & Crash Alert)** (`src/tray/mod.rs`, `src/main.rs`):
  - 在系統匣右鍵選單新增「📋 查看運行日誌 (View Logs)...」，一鍵直接於 Windows 預設文字編輯器開啟日誌檔。
  - 在底層掛載程序無預警中斷時，主動記錄退出代碼並於桌面彈出 Win32 錯誤提示視窗與日誌檔案路徑，杜絕無聲無息默默結束程式。
- **升級 WinFsp 官方安裝套件為 v2.2 正式版 (`scripts/build-release.ps1`)**:
  - 同步將下載與打包之 WinFsp 安裝套件升級為最新 2026 正式發布版 `v2.2.26323`，與編譯鏈之頭文件與 DLL 完全相容。

## [0.1.3] - 2026-11-28

### Fixed
- **精準 WinFsp.Launcher 服務與 64 位元安裝路徑檢測 (Accurate WinFsp Service & WOW64 Detection)**:
  - 修正核心服務名稱判定：WinFsp 官方安裝套件在 Windows 註冊之核心常駐與驅動代理服務名稱為 `WinFsp.Launcher` 與 `WinFsp.Np`（並非單純字串 `winfsp`），且在 64 位元系統之標準安裝路徑記錄於 `HKLM\SOFTWARE\WOW6432Node\WinFsp\InstallDir`。
  - 將檢測邏輯升級為覆蓋 `WinFsp.Launcher` 核心服務、`WOW6432Node\WinFsp\InstallDir`、原生 `SOFTWARE\WinFsp\InstallDir` 與 `fsptool-x64.exe`，徹底解決自動安裝完成後因服務名稱不符而誤報「WinFsp 核心驅動尚未安裝」的異常。
  - 新增核心驅動檢測單元測試 (`test_is_driver_installed_status`)。

## [0.1.2] - 2026-11-28

### Fixed
- **精準核心驅動檢測 (Accurate Kernel Driver Detection)**:
  - 修正 `is_driver_installed()` 檢測邏輯，僅檢驗核心服務 (`Services\winfsp`) 與驅動檔 (`winfsp-x64.sys`)，徹底排除本專案 MSI 預寫 `HKLM\SOFTWARE\WinFsp\MountUseMountmgrFromFSD` 所造成的「尚未安裝驅動卻誤判為已安裝」之重大漏洞。
- **精簡提權流程與消除重複對話框 (Streamlined Single UAC Prompt)**:
  - 移除多餘的中繼確認對話框（消除先跳確認視窗再跳 Windows UAC 的繁瑣體驗），桌面捷徑啟動時直接由 Windows 原生 UAC 授權（單一畫面一次點選【是】即可）。
  - 將核心驅動檢測延後至提權完成後執行，由提權程序直接於背景靜默完成 `winfsp-installer.msi` 註冊，徹底告別「連續兩次對話視窗點【是】」的不良體驗。
  - 在 `relaunch_elevated()` 中顯式指定 `lpDirectory` 指向程式安裝目錄，防止 Windows 預設提權切換至 `System32` 導致相對路徑與相依工具失聯。
- **圖形化掛載失敗診斷提示 (GUI Error Alert on Mount Failure)**:
  - 在 `--tray` 系統匣模式下，若掛載遭遇磁碟機代號衝突、被佔用或核心驅動未載入，即時彈出 Win32 錯誤對話框提示具體錯誤原因與建議處置，不再因無終端機視窗而靜默退出。

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
