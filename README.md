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
| **循序寫入 (Seq Write)** | 2,223 MB/s | **3,099 MB/s** |
| **循序讀取 (Seq Read)** | 6,020 MB/s | **6,245 MB/s** |
| **4K 隨機寫入 (4K Random Write)** | 641,341 IOPS | **1,044,380 IOPS** |
| **256 MB 資料實體記憶體佔用** | 262,144 KB (100%) | **5,776 KB (2.2%)** |

## 🚀 指令與操作指南

### 1. 執行效能基準測試
```powershell
# 無壓縮測試
cargo run --release -- benchmark -s 512M -c none

# LZ4 壓縮測試
cargo run --release -- benchmark -s 512M -c lz4
```

### 2. 啟動 RAM Disk 並常駐系統匣 (Systray)
```powershell
# 建立 2GB RAM Disk，預設掛載 R:，啟用 LZ4 壓縮與系統匣圖示
cargo run --release -- mount -s 2G -m R: -c lz4 --tray

# 搭配快照檔案（啟動時自動載入，結束時自動回存）
cargo run --release -- mount -s 4G -m R: -i D:\backup\ramdisk.img --tray
```

## 🛠️ 開發環境與 WinFsp 整合

專案已將核心引擎與驅動介面完成解耦設計：
1. **核心引擎（預設啟用）**：純 Rust 開發，具備完整的 Sparse 儲存、LZ4 壓縮、快照與 Systray 圖示功能。
2. **WinFsp 檔案系統驅動**：
   - 官方安裝包已下載備妥於 `tools/winfsp-installer.msi`。
   - 若要開啟直接掛載至 Windows 檔案總管的磁碟機代號，雙擊執行 `tools/winfsp-installer.msi` 安裝一次即可。
