# Rust RAM Disk (`rust_ram_disk`)

基於 Rust 打造的高效能 Windows 虛擬記憶體磁碟（RAM Disk），整合動態稀疏分頁（Sparse Paging）、LZ4 即時透明壓縮、映像檔持久化（Snapshot）與 Windows 系統匣（Systray）常駐管理。

## 🌟 核心特性
- **極致記憶體吞吐**：純記憶體隨機寫入突破 **1,000,000+ IOPS**，循序讀寫高達 **3~6 GB/s**。
- **動態稀疏分頁（Sparse Memory Paging）**：未寫入區塊不佔用實體 RAM，寫入時按需（On-Demand）配置記憶體分頁。
- **透明壓縮（LZ4 Compression）**：可選區塊級即時壓縮，大幅節省實體 RAM 佔用（可達 5x~45x 記憶體節省）並提升整體快取容量。
- **持久化快照（Snapshot & Image Persistence）**：支援一鍵或排程將記憶體狀態同步寫入 SSD 映像檔（`.img`），下次啟動時無縫載入。
- **系統匣整合（Windows Systray）**：右下角常駐圖示，即時顯示記憶體使用率、快速一鍵儲存快照、卸載與結束操作。

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
```powershell
# 檢核環境工具鏈狀態（Rust、MSVC、WinFsp 驅動與登錄檔）
./scripts/setup-dev-env.ps1 -CheckOnly

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
