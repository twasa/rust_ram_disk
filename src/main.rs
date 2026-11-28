#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use tracing::info;

use rust_ram_disk::engine::{CompressionMode, SparseMemoryStorage, StorageEngine};
use rust_ram_disk::logger::global_logger;
use rust_ram_disk::persistence::ImagePersistence;
use rust_ram_disk::service::{self, ServiceConfig, ServiceState};
use rust_ram_disk::tray::TrayManager;
use rust_ram_disk::ui::settings::{self, SettingsConfig, WM_APP_APPLY_SETTINGS, WM_APP_SERVICE_STATE_CHANGED};
use rust_ram_disk::vfs;

#[derive(Parser, Debug)]
#[command(
    name = "rust_ram_disk",
    version,
    about = "High-performance RAM Disk with Dynamic Sparse Allocation and Windows System Service Integration"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// 啟用 Windows 系統匣常駐圖示 (未指定子指令時之預設模式)
    #[arg(long, default_value_t = true)]
    tray: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum CliCompression {
    None,
    Lz4,
}

impl From<CliCompression> for CompressionMode {
    fn from(val: CliCompression) -> Self {
        match val {
            CliCompression::None => CompressionMode::None,
            CliCompression::Lz4 => CompressionMode::Lz4,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 管理 Windows 系統服務 (註冊、移除、啟動、停止、狀態、常駐)
    Service {
        #[command(subcommand)]
        action: ServiceSubcommands,
    },

    /// 傳統本機獨立掛載 (不透過系統服務，程序結束時磁碟卸載)
    Mount {
        /// 虛擬磁碟容量 (例如 512M, 1G, 2G, 4G, 8G)
        #[arg(short, long, default_value = "2G")]
        size: String,

        /// Windows 磁碟機代號 (例如 R: 或 S:)
        #[arg(short, long, default_value = "R:")]
        mount_point: String,

        /// 區塊透明壓縮模式
        #[arg(short, long, value_enum, default_value_t = CliCompression::Lz4)]
        compress: CliCompression,

        /// 快照映像檔路徑 (啟動時載入，退出時回存)
        #[arg(short, long)]
        image: Option<PathBuf>,

        /// 啟用 Windows 系統匣圖示
        #[arg(long, default_value_t = true)]
        tray: bool,
    },

    /// 執行記憶體儲存引擎讀寫效能與 IOPS 基準測試
    Benchmark {
        /// 測試緩衝區大小
        #[arg(short, long, default_value = "512M")]
        size: String,

        /// 壓縮模式
        #[arg(short, long, value_enum, default_value_t = CliCompression::None)]
        compress: CliCompression,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceSubcommands {
    /// Windows SCM 調用之服務核心進入點 (供系統服務管理器調用)
    Run,

    /// 註冊為 Windows 系統服務 (需系統管理員權限)
    Install {
        /// 磁碟機代號 (例如 R:)
        #[arg(short, long, default_value = "R:")]
        mount_point: String,

        /// 磁碟容量 (例如 2G, 4G, 8G)
        #[arg(short, long, default_value = "2G")]
        size: String,

        /// 壓縮模式 (lz4 或 none)
        #[arg(short, long, default_value = "lz4")]
        compress: String,

        /// 快照映像檔路徑
        #[arg(short, long)]
        image: Option<String>,

        /// 是否隨 Windows 開機自動啟動
        #[arg(long, default_value_t = true)]
        auto_start: bool,

        /// 註冊後是否立即啟動服務
        #[arg(long, default_value_t = true)]
        start_now: bool,
    },

    /// 解除註冊並移除 Windows 系統服務
    Uninstall,

    /// 啟動 Windows 系統服務
    Start,

    /// 停止 Windows 系統服務
    Stop,

    /// 重新啟動 Windows 系統服務
    Restart,

    /// 查詢 Windows 系統服務運行狀態
    Status,

    /// 觸發即時快照存檔
    Snapshot,
}

fn parse_size(input: &str) -> Result<u64, String> {
    service::parse_size_bytes(input)
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::Console::{
            AttachConsole, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        };
        use windows::Win32::Foundation::HANDLE;
        use std::os::windows::io::IntoRawHandle;

        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let raw = file.into_raw_handle();
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, HANDLE(raw as _));
                let _ = SetStdHandle(STD_ERROR_HANDLE, HANDLE(raw as _));
            }
        }
    }

    tracing_subscriber::fmt::init();
    let app_logger = global_logger();
    rust_ram_disk::logger::AppLogger::install_panic_hook(app_logger.clone());

    app_logger.log(
        "INFO",
        "main",
        "app_started",
        &format!("Rust RAM Disk 應用程式啟動 (PID: {})", std::process::id()),
        Some(serde_json::json!({
            "args": std::env::args().collect::<Vec<_>>(),
            "log_path": app_logger.get_log_path_str(),
        })),
    );

    let cli = Cli::parse();

    match cli.command {
        // ===================================================================
        // 1. Windows 系統服務管理子命令
        // ===================================================================
        Some(Commands::Service { action }) => match action {
            ServiceSubcommands::Run => {
                info!("啟動 Windows 系統服務分發常駐程序...");
                if let Err(e) = service::run_service() {
                    app_logger.log("ERROR", "service", "run_failed", &format!("服務執行失敗: {}", e), None);
                    eprintln!("Windows 系統服務分發失敗: {}", e);
                    std::process::exit(1);
                }
            }
            ServiceSubcommands::Install {
                mount_point,
                size,
                compress,
                image,
                auto_start,
                start_now,
            } => {
                #[cfg(target_os = "windows")]
                if !vfs::is_elevated() {
                    println!("🔐 需要系統管理員權限以註冊 Windows 系統服務，正在發起 UAC 提權請求...");
                    vfs::relaunch_elevated()?;
                    return Ok(());
                }

                let cfg = ServiceConfig {
                    mount_point,
                    size,
                    compress,
                    image_path: image,
                    auto_start,
                };
                println!("🚀 正在註冊 Rust RAM Disk Windows 系統服務...");
                println!("   磁碟代號: {}", cfg.mount_point);
                println!("   虛擬容量: {}", cfg.size);
                println!("   壓縮模式: {}", cfg.compress);
                println!("   開機自啟: {}", cfg.auto_start);

                service::install(&cfg, start_now)?;
                println!("✅ 系統服務註冊成功！");
                if start_now {
                    println!("🎉 服務已啟動，磁碟機 {} 已在背景常駐掛載。", cfg.mount_point);
                }
            }
            ServiceSubcommands::Uninstall => {
                #[cfg(target_os = "windows")]
                if !vfs::is_elevated() {
                    println!("🔐 需要系統管理員權限以移除 Windows 系統服務，正在發起 UAC 提權請求...");
                    vfs::relaunch_elevated()?;
                    return Ok(());
                }
                println!("🗑️ 正在移除 Rust RAM Disk 系統服務...");
                service::uninstall()?;
                println!("✅ 系統服務已完全移除。");
            }
            ServiceSubcommands::Start => {
                #[cfg(target_os = "windows")]
                if !vfs::is_elevated() {
                    println!("🔐 需要系統管理員權限以啟動服務，正在發起 UAC 提權請求...");
                    vfs::relaunch_elevated()?;
                    return Ok(());
                }
                println!("▶️ 正在啟動 Rust RAM Disk 系統服務...");
                service::start()?;
                println!("✅ 服務已成功啟動。");
            }
            ServiceSubcommands::Stop => {
                #[cfg(target_os = "windows")]
                if !vfs::is_elevated() {
                    println!("🔐 需要系統管理員權限以停止服務，正在發起 UAC 提權請求...");
                    vfs::relaunch_elevated()?;
                    return Ok(());
                }
                println!("⏹️ 正在停止 Rust RAM Disk 系統服務...");
                service::stop()?;
                println!("✅ 服務已停止。");
            }
            ServiceSubcommands::Restart => {
                #[cfg(target_os = "windows")]
                if !vfs::is_elevated() {
                    println!("🔐 需要系統管理員權限以重啟服務，正在發起 UAC 提權請求...");
                    vfs::relaunch_elevated()?;
                    return Ok(());
                }
                println!("🔄 正在重新啟動 Rust RAM Disk 系統服務...");
                service::restart()?;
                println!("✅ 服務已重新啟動。");
            }
            ServiceSubcommands::Status => {
                let state = service::query_status()?;
                let cfg = ServiceConfig::load();
                println!("============================================================");
                println!("📋 Rust RAM Disk 系統服務狀態");
                println!("   服務名稱   : RustRamDiskService");
                println!("   運行狀態   : {}", state);
                println!("   目標磁碟機 : {}", cfg.mount_point);
                println!("   虛擬容量   : {}", cfg.size);
                println!("   壓縮模式   : {}", cfg.compress);
                println!("   快照路徑   : {:?}", cfg.image_path);
                println!("   開機自啟   : {}", cfg.auto_start);
                println!("============================================================");
            }
            ServiceSubcommands::Snapshot => {
                println!("💾 正在觸發即時快照儲存...");
                service::trigger_snapshot()?;
                println!("✅ 快照儲存完成！");
            }
        },

        // ===================================================================
        // 2. 儲存引擎 In-Memory 基準測試子命令
        // ===================================================================
        Some(Commands::Benchmark { size, compress }) => {
            let cap = parse_size(&size).map_err(|e| anyhow::anyhow!(e))?;
            println!("============================================================");
            println!("🚀 Rust RAM Disk Storage Engine In-Memory Benchmark");
            println!(
                "   Virtual Capacity : {} bytes ({:.2} MB)",
                cap,
                cap as f64 / (1024.0 * 1024.0)
            );
            println!("   Compression      : {:?}", compress);
            println!("============================================================");

            let storage = SparseMemoryStorage::new(cap, 4096, compress.into());

            // 1. Sequential Write Benchmark (64 KB chunks)
            let chunk_size = 64 * 1024;
            let total_test_bytes = cap.min(256 * 1024 * 1024); // up to 256MB
            let iterations = (total_test_bytes / chunk_size as u64) as usize;
            let write_buf: Vec<u8> = (0..chunk_size).map(|i| (i % 251) as u8).collect();

            print!(
                "-> Testing Sequential Write ({} MB)... ",
                total_test_bytes / (1024 * 1024)
            );
            let start = Instant::now();
            for i in 0..iterations {
                let offset = (i * chunk_size) as u64;
                storage.write_at(offset, &write_buf)?;
            }
            let duration = start.elapsed();
            let mb_sec = (total_test_bytes as f64 / (1024.0 * 1024.0)) / duration.as_secs_f64();
            println!("{:.2} MB/s ({:.2?})", mb_sec, duration);

            // 2. Sequential Read Benchmark (64 KB chunks)
            let mut read_buf = vec![0u8; chunk_size];
            print!(
                "-> Testing Sequential Read ({} MB)... ",
                total_test_bytes / (1024 * 1024)
            );
            let start = Instant::now();
            for i in 0..iterations {
                let offset = (i * chunk_size) as u64;
                storage.read_at(offset, &mut read_buf)?;
            }
            let duration = start.elapsed();
            let mb_sec = (total_test_bytes as f64 / (1024.0 * 1024.0)) / duration.as_secs_f64();
            println!("{:.2} MB/s ({:.2?})", mb_sec, duration);

            // 3. 4K Random I/O Benchmark
            let random_ops = 50_000;
            let block_4k = 4096;
            let block_count = total_test_bytes / block_4k as u64;
            let buf_4k = vec![42u8; block_4k];

            print!("-> Testing 4K Random Write ({} ops)... ", random_ops);
            let start = Instant::now();
            for op in 0..random_ops {
                let offset = ((op * 179) % block_count) * block_4k as u64;
                storage.write_at(offset, &buf_4k)?;
            }
            let duration = start.elapsed();
            let iops = random_ops as f64 / duration.as_secs_f64();
            println!("{:.0} IOPS ({:.2?})", iops, duration);

            println!("------------------------------------------------------------");
            println!(
                "Physical Memory Allocated : {} KB",
                storage.physical_memory_used() / 1024
            );
            println!("Active Allocated Blocks   : {}", storage.allocated_blocks());
            println!("============================================================");
        }

        // ===================================================================
        // 3. 傳統本機獨立掛載 (非系統服務模式)
        // ===================================================================
        Some(Commands::Mount {
            size,
            mount_point,
            compress,
            image,
            tray,
        }) => {
            run_standalone_mount(&size, &mount_point, compress, image, tray)?;
        }

        // ===================================================================
        // 4. 預設執行行為：啟動 UI 並常駐至右下角 Systray 監控面板
        //    (RAM Disk 跟隨 Windows 系統服務，UI 關閉磁碟不消失)
        // ===================================================================
        None => {
            run_systray_service_monitor()?;
        }
    }

    Ok(())
}

/// 啟動 Windows 系統匣 (Systray) 服務監控面板
/// 此為執行程式之預設行為：RAM Disk 跟隨 Windows Service，關閉 UI 磁碟依然常駐運作！
fn run_systray_service_monitor() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::core::w;
        use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
            MessageBoxW, MsgWaitForMultipleObjects, PeekMessageW, PostMessageW, RegisterClassExW,
            RegisterWindowMessageW, TranslateMessage, MB_ICONINFORMATION, MB_OK, MSG, PM_REMOVE,
            QS_ALLINPUT, WNDCLASSEXW, WM_APP, WINDOW_EX_STYLE, WINDOW_STYLE,
        };

        const WM_APP_RELOAD_TRAY: u32 = WM_APP + 101;
        let host_title = w!("RustRAMDisk_SystrayHostWindow");

        // 1. 單實例檢測 (Single-Instance Check)
        if let Ok(existing_hwnd) = unsafe { FindWindowW(None, host_title) } {
            if !existing_hwnd.0.is_null() {
                unsafe {
                    let _ = PostMessageW(existing_hwnd, WM_APP_RELOAD_TRAY, WPARAM(0), LPARAM(0));
                }
                println!("🔔 Rust RAM Disk 監控面板已在背景執行中！已刷新圖示。");
                return Ok(());
            }
        }

        // 2. 驅動環境預先檢查 (若未安裝，引導安裝)
        if !vfs::is_driver_installed() {
            println!("⚠️ 系統尚未安裝 WinFsp 核心驅動，嘗試自動註冊...");
            if vfs::is_elevated() {
                let _ = vfs::install_winfsp_driver_silent();
            } else {
                let _ = vfs::relaunch_elevated();
                return Ok(());
            }
        }

        // 3. 讀取全域服務配置
        let mut cfg = ServiceConfig::load();
        let svc_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
        let mut is_running = svc_state == ServiceState::Running;

        // 若服務尚未安裝或停止，自動嘗試啟動（若開機自啟開啟）
        if svc_state == ServiceState::Stopped && cfg.auto_start {
            if let Ok(()) = service::start() {
                is_running = true;
            }
        }

        unsafe extern "system" fn host_wndproc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        // 註冊隱藏 Host 視窗
        let class_name = w!("RustRAMDisk_ServiceHostClass");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_wndproc),
            hInstance: HINSTANCE(std::ptr::null_mut()),
            lpszClassName: class_name,
            ..Default::default()
        };
        unsafe {
            let _ = RegisterClassExW(&wc);
        }

        let host_hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                host_title,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND(std::ptr::null_mut()),
                None,
                HINSTANCE(std::ptr::null_mut()),
                None,
            )
            .ok()
        };

        let taskbar_created_msg = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };

        let mut tray_manager =
            TrayManager::new_service_mode("Rust RAM Disk", &cfg.mount_point, &cfg.size, is_running)?;
        let mut menu_channel = muda::MenuEvent::receiver();
        let tray_channel = tray_icon::TrayIconEvent::receiver();

        println!("\n📌 Rust RAM Disk 系統匣監控面板已就緒。");
        println!("   磁碟代號: {} | 容量: {} | 服務運行狀態: {}", cfg.mount_point, cfg.size, is_running);
        println!("   💡 提示：磁碟由系統服務全時維護，即使關閉本面板或退出，磁碟機依然不受影響！");

        let open_settings = |h_opt: Option<HWND>, current_cfg: &ServiceConfig| {
            if let Some(h) = h_opt {
                let st_cfg: SettingsConfig = current_cfg.clone().into();
                settings::show_settings_window(h, &st_cfg);
            }
        };

        let mut last_status_check = Instant::now();
        let mut running = true;

        while running {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                // 1. 處理工作列重建或二次啟動重載
                if msg.message == WM_APP_RELOAD_TRAY
                    || (taskbar_created_msg != 0 && msg.message == taskbar_created_msg)
                {
                    tracing::info!("收到系統匣重載通知，正在重建圖示...");
                    let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                    is_running = cur_state == ServiceState::Running;
                    if let Ok(new_tm) =
                        TrayManager::new_service_mode("Rust RAM Disk", &cfg.mount_point, &cfg.size, is_running)
                    {
                        tray_manager = new_tm;
                        menu_channel = muda::MenuEvent::receiver();
                    }
                }
                // 2. 處理設定變更套用
                else if msg.message == WM_APP_APPLY_SETTINGS {
                    if let Some(new_cfg) = settings::take_pending_config() {
                        cfg = new_cfg.into();
                        let _ = cfg.save();
                        println!("⚙️ 收到全域設定更新: 磁碟代號: {}, 容量: {}", cfg.mount_point, cfg.size);

                        // 若服務正在運行，重啟服務以套用新磁碟規格
                        if service::query_status().unwrap_or(ServiceState::NotInstalled) == ServiceState::Running {
                            println!("🔄 正在重啟系統服務以套用新設定...");
                            let _ = service::restart();
                        }

                        let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                        is_running = cur_state == ServiceState::Running;
                        tray_manager.update_service_status(&cfg.mount_point, &cfg.size, is_running);
                    }
                }
                // 3. 處理服務狀態外部變更
                else if msg.message == WM_APP_SERVICE_STATE_CHANGED {
                    let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                    is_running = cur_state == ServiceState::Running;
                    tray_manager.update_service_status(&cfg.mount_point, &cfg.size, is_running);
                }

                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // 4. 定期狀態校驗 (每 2 秒檢查一次系統服務狀態，同步更新圖示)
            if last_status_check.elapsed() >= Duration::from_secs(2) {
                last_status_check = Instant::now();
                let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                let new_running = cur_state == ServiceState::Running;
                if new_running != is_running {
                    is_running = new_running;
                    cfg = ServiceConfig::load();
                    tray_manager.update_service_status(&cfg.mount_point, &cfg.size, is_running);
                }
            }

            // 5. 處理滑鼠點擊系統匣圖示 (左鍵點擊開啟設定)
            while let Ok(tray_event) = tray_channel.try_recv() {
                match tray_event {
                    tray_icon::TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    }
                    | tray_icon::TrayIconEvent::DoubleClick {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } => {
                        open_settings(host_hwnd, &cfg);
                    }
                    _ => {}
                }
            }

            // 6. 處理右鍵選單項目點擊
            while let Ok(event) = menu_channel.try_recv() {
                if event.id == tray_manager.exit_item.id() {
                    // 重要：退出 Systray 監控面板，磁碟機依然跟隨服務常駐，不予卸載！
                    println!("👋 關閉系統匣監控面板。Rust RAM Disk 系統服務依然在背景全時常駐。");
                    running = false;
                    break;
                } else if event.id == tray_manager.settings_item.id() {
                    open_settings(host_hwnd, &cfg);
                } else if event.id == tray_manager.view_logs_item.id() {
                    global_logger().open_in_viewer();
                } else if event.id == tray_manager.snapshot_item.id() {
                    println!("💾 正在觸發即時快照存檔...");
                    match service::trigger_snapshot() {
                        Ok(()) => {
                            println!("✅ 快照儲存完成！");
                            unsafe {
                                MessageBoxW(
                                    None,
                                    w!("虛擬磁碟快照已成功儲存！"),
                                    w!("Rust RAM Disk"),
                                    MB_ICONINFORMATION | MB_OK,
                                );
                            }
                        }
                        Err(e) => {
                            println!("❌ 儲存快照失敗: {}", e);
                        }
                    }
                } else if let Some(ref toggle) = tray_manager.service_toggle_item {
                    if event.id == toggle.id() {
                        if is_running {
                            println!("⏹️ 使用者自系統匣要求停止服務...");
                            let _ = service::stop();
                        } else {
                            println!("▶️ 使用者自系統匣要求啟動服務...");
                            let _ = service::start();
                        }
                        let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                        is_running = cur_state == ServiceState::Running;
                        tray_manager.update_service_status(&cfg.mount_point, &cfg.size, is_running);
                    }
                } else if let Some(ref restart_item) = tray_manager.service_restart_item {
                    if event.id == restart_item.id() {
                        println!("🔄 使用者自系統匣要求重啟服務...");
                        let _ = service::restart();
                        let cur_state = service::query_status().unwrap_or(ServiceState::NotInstalled);
                        is_running = cur_state == ServiceState::Running;
                        tray_manager.update_service_status(&cfg.mount_point, &cfg.size, is_running);
                    }
                }
            }

            unsafe {
                let _ = MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
            }
        }

        if let Some(hwnd) = host_hwnd {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }

    Ok(())
}

/// 傳統獨立掛載程序 (相容非服務模式之本機臨時掛載)
fn run_standalone_mount(
    size: &str,
    mount_point: &str,
    compress: CliCompression,
    image: Option<PathBuf>,
    tray: bool,
) -> anyhow::Result<()> {
    let capacity = parse_size(size).map_err(|e| anyhow::anyhow!(e))?;
    let norm_mount = vfs::normalize_mount_point(mount_point).map_err(|e| anyhow::anyhow!(e))?;

    println!("============================================================");
    println!("🚀 Rust RAM Disk 本機獨立掛載程序啟動");
    println!("   目標磁碟機代號 : {}", norm_mount);
    println!(
        "   虛擬配置容量   : {} bytes ({:.2} MB)",
        capacity,
        capacity as f64 / (1024.0 * 1024.0)
    );
    println!("   記憶體壓縮模式 : {:?}", compress);
    println!("============================================================");

    #[cfg(target_os = "windows")]
    if !vfs::is_elevated() {
        vfs::relaunch_elevated()?;
        return Ok(());
    }

    if !vfs::is_driver_installed() {
        let _ = vfs::install_winfsp_driver_silent();
    }

    let current_norm_mount = norm_mount.clone();
    let current_size = size.to_string();
    let current_compress = compress;
    let current_image = image;

    let mut mount_handle = vfs::mount_ram_disk(&current_norm_mount, capacity)?;
    println!("🎉 虛擬磁碟機 {} 已成功掛載！", current_norm_mount);

    if let Some(ref img_path) = current_image {
        if img_path.exists() {
            let _ = ImagePersistence::restore_drive_snapshot(img_path, &current_norm_mount);
        }
    }

    if tray {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW,
                TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT,
            };

            let tray_manager =
                TrayManager::new_standalone_mode("Rust RAM Disk", &current_norm_mount, &current_size)?;
            let menu_channel = muda::MenuEvent::receiver();
            let mut running = true;

            while running && mount_handle.is_alive() {
                let mut msg = MSG::default();
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                while let Ok(event) = menu_channel.try_recv() {
                    if event.id == tray_manager.exit_item.id()
                        || tray_manager
                            .unmount_item
                            .as_ref()
                            .map_or(false, |it| event.id == it.id())
                    {
                        running = false;
                        break;
                    }
                }

                unsafe {
                    let _ = MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
                }
            }
        }
    } else {
        println!("🚀 RAM Disk 已以 Console 模式常駐。按下 Ctrl+C 即可終止並安全卸載。");
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })?;
        while running.load(Ordering::SeqCst) && mount_handle.is_alive() {
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    if let Some(ref img_path) = current_image {
        let _ = ImagePersistence::save_drive_snapshot(
            &current_norm_mount,
            img_path,
            current_compress.into(),
        );
    }

    println!("⏏️ 正在安全卸載磁碟機 {}...", current_norm_mount);
    let _ = mount_handle.unmount();
    println!("✅ 磁碟機 {} 已完全卸載，程式結束。", current_norm_mount);

    Ok(())
}
