# Rust RAM Disk (`rust_ram_disk`)

基於 Rust 打造的高效能 Windows 虛擬記憶體磁碟（RAM Disk），整合動態稀疏分頁（Sparse Paging）、LZ4 即時透明壓縮、映像檔持久化（Snapshot）與 Windows 系統匣（Systray）常駐管理。

## 🌟 核心特性
- **極致記憶體吞吐**：純記憶體隨機寫入突破 **1,000,000+ IOPS**，循序讀寫高達 **3~6 GB/s**。
- **動態稀疏分頁（Sparse Memory Paging）**：未寫入區塊不佔用實體 RAM，寫入時按需（On-Demand）配置記憶體分頁。
- **透明壓縮（LZ4 Compression）**：可選區塊級即時壓縮，大幅節省實體 RAM 佔用（可達 5x~45x 記憶體節省）並提升整體快取容量。
- **持久化快照（Snapshot & Image Persistence）**：支援一鍵或排程將記憶體狀態同步寫入 SSD 映像檔（`.img`），下次啟動時無縫載入。
- **系統匣整合與原生設定介面（Windows Systray & Settings UI）**：
  - 右下角常駐圖示，即時顯示磁碟機代號與記憶體容量。
  - **純 Win32 原生設定視窗**：左鍵點擊圖示或右鍵點選「⚙️ 磁碟設定」即可叫出設定介面，零額外 GUI 依賴、瞬間啟動。
  - **動態重新配置（Apply & Remount）**：可於運作中即時切換磁碟機代號（`R:`, `S:`, `T:` 等）、變更容量（`512M` 至 `64G` 或自訂）、切換壓縮演算法（LZ4 / None）、選擇快照映像檔路徑，以及勾選「開機自動背景掛載（Run 登錄檔）」。
  - **安全熱切換**：套用時自動儲存現有快照至映像檔、卸載舊磁碟並以新參數無縫重掛，隨後即時更新系統匣狀態與 Windows 檔案總管。
  - **快速選單**：右鍵選單提供即時快照存檔、安全卸載與退出功能。

## 📊 實測基準效能 (In-Memory Benchmark)
在 512 MB 虛擬記憶體磁碟上的實測表現（AMD/Intel x64）：

| 測試項目 | 無壓縮模式 (None) | LZ4 透明壓縮模式 (Lz4) |
| :--- | :--- | :--- |
| **循序寫入 (Seq Write)** | 2,223 MB/s | **3,388 MB/s** |
| **循序讀取 (Seq Read)** | 6,020 MB/s | **6,530 MB/s** |
| **4K 隨機寫入 (4K Random Write)** | 641,341 IOPS | **1,031,183 IOPS (破百萬)** |
| **256 MB 資料實體記憶體佔用** | 262,144 KB (100%) | **5,776 KB (2.2%)** |

## 🛠️ 自動化工程腳本 (PowerShell 7+ Scripts)

比照專案工程規範，於 `scripts/` 提供一鍵環境配置、本地部署與發布打包腳本：

### 1. 環境檢核與安裝導引 (`setup-dev-env.ps1`)
具備自動引導與無套件管理器備援機制（當系統無 `winget` 時自動下載官方 `rustup-init.exe` 靜默安裝、自動檢測 Visual Studio C++ Build Tools，並自動就緒 WiX Toolset 3.11）：
```powershell
# 檢核環境工具鏈狀態（Rust、MSVC、WinFsp 驅動與登錄檔、WiX 工具）
./scripts/setup-dev-env.ps1 -CheckOnly

# 全自動引導安裝缺失之 Rust 工具鏈與 WiX
./scripts/setup-dev-env.ps1

# 自動啟動專案內建之 WinFsp 安裝精靈註冊驅動
./scripts/setup-dev-env.ps1 -InstallWinFsp
```

### 2. 本機建置與自動部署 (`build-local.ps1`)
```powershell
# 一鍵編譯並部署至 $HOME\RAMDisk（並產生 start-ramdisk.bat 快捷啟動檔）
./scripts/build-local.ps1

# 部署至指定目錄並在完成後立即執行記憶體儲存引擎驗證壓測
./scripts/build-local.ps1 -Destination "D:\Tools\RAMDisk" -RunBenchmark
```

### 3. 正式發布與單一 MSI 安裝套件打包 (`build-release.ps1`)
```powershell
# 自動執行完整單元測試、Release 編譯最佳化與 WiX MSI 封裝，匯出系統原生單一 MSI 安裝包 (RustRAMDisk-v<version>-x64.msi) 與 SHA-256 校驗表至 release-artifacts/
./scripts/build-release.ps1

# 若需略過單元測試快速打包
./scripts/build-release.ps1 -SkipTests
```

## 🛡️ 權限與生命週期整合 (Security & Process Lifecycle)
- **Windows 掛載管理器本機磁碟模式 (True Local Disk via Mount Manager)**：
  - **原生硬碟圖示**：透過 `\\.\<代號>` 向 Windows 掛載管理器 (`\Device\MountPointManager`) 註冊全域磁碟區，Windows 檔案總管顯示為標準**本機硬碟 (`DRIVE_FIXED`)**，徹底告別網路磁碟圖示。
  - **免安裝軟體相容支援**：消除 Windows 1005 (`ERROR_UNRECOGNIZED_VOLUME`) 錯誤，Windows PE Loader 與 AppCompat 能正確解析磁碟區代號，所有免安裝可攜式程式（Portable `.exe`）皆可直接雙擊順暢執行。
  - **自動 UAC 權限導引**：若未以系統管理員身分啟動，程式會主動跳出友善說明視窗並透過標準 Windows UAC 機制完成提權重啟。
  - **MSI 安裝環境優化**：MSI 安裝套件自動於 `HKLM\SOFTWARE\WinFsp` 註冊 `MountUseMountmgrFromFSD` 旗標，確保核心驅動與全域掛載管理器無縫協同。
- **單一繁體中文 MSI 完整安裝精靈 (Standard Windows MSI with Full Wizard UI)**：
  - **純單一 MSI 發布**：精簡打包流程，統一發布獨立、高內聚的微軟標準 `RustRAMDisk-v0.1.0-x64.msi`。
  - **完整安裝引導與自訂目錄**：
    1. **歡迎精靈畫面**：繁體中文歡迎步驟引導。
    2. **自訂安裝目的地**：明確詢問使用者軟體要安裝到哪裡，支援「變更/瀏覽」按鈕自選安裝磁碟與路徑（預設 `C:\Program Files\RustRAMDisk`）。
    3. **準備安裝與進度顯示**：顯示檔案解壓與系統元件配置進度條。
    4. **安裝完成確認畫面**：清楚呈現安裝成功通知，提供「安裝完成後立即啟動 Rust RAM Disk」勾選方塊，點擊「完成」時才啟動服務與常駐系統匣。
  - **內嵌核心驅動離線自癒**：MSI 內部直接打包 `winfsp-installer.msi`，乾淨環境隨裝即用，無需手動補載依賴。
- **免安裝軟體執行支援**：WinFsp 根安全描述元（Root SDDL）注入 `O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;WD)`，具備物件繼承（`OI`）與容器繼承（`CI`），賦予 System、Administrators 與所有使用者完全控制權（`FA`），全面支援 portable `.exe` 執行與檔案讀取。
- **無縫預覽相容性**：檔案節點上限自預設 1024 擴充至 1,048,576，並啟用快取立即 flush/purge 機制（`-f`），檔案管理器（如 `rust_file_manager`）與 WebView2 即時預覽文字、圖片與程式碼零衝突。
- **系統匣單實例與智慧重載（Single-Instance & Reload）**：
  - 重複執行桌面或開始功能表捷徑時，不會跳出錯誤或重複掛載，而是自動向背景常駐實例發送重載通知，即時重新整理並顯示系統匣圖示。
  - 監聽 Windows `TaskbarCreated` 廣播，當 `explorer.exe` 重啟或工作列重建時，圖示自動復原不遺失。
  - 捷徑直接調用主程式並啟用無主控台視窗模式（GUI Subsystem），徹底消除黑底 `cmd.exe` 視窗。

## 🚀 CLI 指令與手動操作

### 執行效能基準測試
```powershell
# 無壓縮測試
cargo run --release -- benchmark -s 512M -c none

# LZ4 壓縮測試
cargo run --release -- benchmark -s 512M -c lz4
```

### 啟動 RAM Disk 並常駐系統匣 (Systray)
```powershell
# 建立 2GB RAM Disk，預設掛載 R:，啟用 LZ4 壓縮與系統匣圖示
cargo run --release -- mount -s 10G -m S: -c lz4 --tray

# 搭配快照檔案（啟動時自動載入，結束時自動回存）
cargo run --release -- mount -s 4G -m R: -i D:\backup\ramdisk.img --tray
```

## 🤖 自動化 CI/CD 與 GitHub Releases

專案內建完整的 GitHub Actions 自動化工作流程：
- [`.github/workflows/ci.yml`](.github/workflows/ci.yml)：每次推送至 `main` 或開立 PR 時，自動在 `windows-latest` 執行單元測試、編譯與 MSI 打包驗證。
- [`.github/workflows/release.yml`](.github/workflows/release.yml)：推送語意化版本標籤（例如 `git tag v0.1.0 && git push origin v0.1.0`）或於 GitHub Actions 手動觸發時，自動編譯官方正式發行版、封裝繁體中文單一 MSI 安裝套件、計算 `SHA256SUMS.txt` 並自動發布至 GitHub Releases。
