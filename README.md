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

### 3. MSI 安裝套件打包 (`build-msi.ps1`)
```powershell
# 自動透過 WiX Toolset 編譯出標準 Windows .msi 安裝套件 (包含桌面與開始功能表捷徑)
./scripts/build-msi.ps1
```

### 4. 正式發布與全套件打包 (`build-release.ps1`)
```powershell
# 執行完整測試、Release 編譯、MSI 封裝，並匯出可攜版 .exe、.msi 安裝包與 SHA-256 校驗表至 release-artifacts/
./scripts/build-release.ps1
```

## 🛡️ 權限與生命週期整合 (Security & Process Lifecycle)
- **全新乾淨環境隨裝即用 (Out-of-the-Box on Clean Windows)**：
  - **雙重安裝管道**：同步提供 **WiX Burn 整合引導檔 (`RustRAMDiskSetup.exe`)** 與 **標準微軟 MSI 安裝包 (`RustRAMDisk.msi`)**。
  - **安裝完成即時掛載**：MSI 與引導程式於安裝完成後立即自動調用掛載程序，`R:` 磁碟機代號與系統匣即刻就緒，無需使用者手動尋找捷徑啟動。
  - **智慧驅動自癒與 UAC 引導 (Smart Driver Self-Healing)**：若在未安裝 WinFsp 的乾淨機器上啟動主程式或捷徑，程式會主動跳出友善說明視窗並透過標準 Windows UAC 機制完成核心驅動註冊，隨後無縫完成掛載。
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
