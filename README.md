# Rust RAM Disk (`rust_ram_disk`)

基於 Rust 打造的高效能 Windows 虛擬記憶體磁碟（RAM Disk），整合 WinFsp 檔案系統代理、動態稀疏分頁（Sparse Paging）、LZ4 即時透明壓縮與系統匣（Systray）常駐管理。

## 🌟 核心特性
- **極致記憶體吞吐**：透過 WinFsp 原生檔案系統介面，提供數 GB/s 讀寫效能。
- **動態稀疏分頁（Sparse Memory Paging）**：未寫入區塊不佔用實體 RAM，寫入時按需（On-Demand）配置記憶體分頁。
- **透明壓縮（LZ4 Compression）**：可選區塊級即時壓縮，大幅節省實體 RAM 佔用並提升有效可用容量。
- **持久化快照（Snapshot & Image Persistence）**：支援一鍵或排程將記憶體狀態同步寫入 SSD 映像檔（`.img`），下次啟動時無縫載入。
- **系統匣整合（Windows Systray）**：右下角常駐圖示，即時顯示記憶體使用率、快速掛載/卸載與快照操作。

## 🚀 系統環境要求
1. **Windows 10 / 11 / Server 2016+ (64-bit)**
2. **WinFsp 執行環境**：
   - 本專案已在專案根目錄備妥 `tools/winfsp-installer.msi`（或至 [WinFsp 官方 Releases](https://github.com/winfsp/winfsp/releases) 下載）。
   - 請以管理員身分點擊執行安裝一次即可。

## 🛠️ 開發與建置

```powershell
# 執行測試
cargo test -q

# 建置 Release 版本
cargo build --release
```

## 📖 架構設計
```text
rust_ram_disk/
├── src/
│   ├── engine/          # 核心儲存引擎：分頁管理、Sparse配置、LZ4壓縮
│   ├── vfs/             # WinFsp 檔案系統橋接介面實作
│   ├── persistence/     # 映像檔儲存與還原 (Snapshot / WAL)
│   ├── tray/            # Windows 系統匣圖示與選單控制
│   └── main.rs          # 應用程式入口與 CLI / 常駐管理
```
