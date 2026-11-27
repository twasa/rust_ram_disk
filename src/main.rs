#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use tracing::{error, warn};

use rust_ram_disk::engine::{CompressionMode, SparseMemoryStorage, StorageEngine};
use rust_ram_disk::persistence::ImagePersistence;
use rust_ram_disk::vfs;

#[derive(Parser, Debug)]
#[command(
    name = "rust_ram_disk",
    version,
    about = "High-performance RAM Disk with Dynamic Sparse Allocation and Windows Systray Integration"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    /// Mount a high-performance RAM disk
    Mount {
        /// Virtual capacity (e.g. 512M, 1G, 4G, 8G)
        #[arg(short, long, default_value = "1G")]
        size: String,

        /// Windows drive letter or mount point (e.g. S: or R:)
        #[arg(short, long, default_value = "R:")]
        mount_point: String,

        /// Memory block compression mode
        #[arg(short, long, value_enum, default_value_t = CliCompression::Lz4)]
        compress: CliCompression,

        /// Path to load snapshot image on start, and save back on unmount
        #[arg(short, long)]
        image: Option<PathBuf>,

        /// Enable Windows system tray icon
        #[arg(long, default_value_t = true)]
        tray: bool,
    },

    /// Run in-memory storage engine speed and IOPS benchmark
    Benchmark {
        /// Size of memory buffer for benchmark
        #[arg(short, long, default_value = "512M")]
        size: String,

        /// Compression mode to test
        #[arg(short, long, value_enum, default_value_t = CliCompression::None)]
        compress: CliCompression,
    },
}

fn parse_size(input: &str) -> Result<u64, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Size cannot be empty".into());
    }

    let (num_part, unit) = match s.chars().last() {
        Some('g' | 'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024u64),
        Some('m' | 'M') => (&s[..s.len() - 1], 1024 * 1024u64),
        Some('k' | 'K') => (&s[..s.len() - 1], 1024u64),
        _ => (s, 1u64),
    };

    let number: u64 = num_part
        .trim()
        .parse()
        .map_err(|e| format!("Invalid number '{}': {}", num_part, e))?;

    Ok(number * unit)
}

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::Console::{AttachConsole, SetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE};
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Benchmark { size, compress } => {
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

        Commands::Mount {
            size,
            mount_point,
            compress,
            image,
            tray,
        } => {
            let capacity = parse_size(&size).map_err(|e| anyhow::anyhow!(e))?;
            let norm_mount = vfs::normalize_mount_point(&mount_point)
                .map_err(|e| anyhow::anyhow!(e))?;

            println!("============================================================");
            println!("🚀 Rust RAM Disk 掛載程序啟動");
            println!("   目標磁碟機代號 : {}", norm_mount);
            println!(
                "   虛擬配置容量   : {} bytes ({:.2} MB)",
                capacity,
                capacity as f64 / (1024.0 * 1024.0)
            );
            println!("   記憶體壓縮模式 : {:?}", compress);
            println!("============================================================");

            // 0. 單實例檢測 (Single-Instance Check) 與系統匣重載信號
            #[cfg(target_os = "windows")]
            {
                use windows::core::PCWSTR;
                use windows::Win32::Foundation::{LPARAM, WPARAM};
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};

                const WM_APP_RELOAD_TRAY: u32 = WM_APP + 101;
                let host_title = format!("RustRAMDisk_Host_{}\0", norm_mount.trim_end_matches(':'));
                let wide_title: Vec<u16> = host_title.encode_utf16().collect();
                if let Ok(existing_hwnd) = unsafe { FindWindowW(None, PCWSTR(wide_title.as_ptr())) } {
                    if !existing_hwnd.0.is_null() {
                        unsafe {
                            let _ = PostMessageW(existing_hwnd, WM_APP_RELOAD_TRAY, WPARAM(0), LPARAM(0));
                        }
                        println!("🔔 虛擬磁碟機 {} 已在背景執行中！", norm_mount);
                        println!("   已向執行中實例發送重載信號，重新載入並顯示右下角系統匣圖示。");
                        return Ok(());
                    }
                }
            }

            // 1. 驅動環境檢核與自動註冊修復
            if !vfs::is_driver_installed() {
                println!("🔍 系統尚未註冊 WinFsp 核心檔案系統代理驅動 (winfsp.sys)...");
                if let Some(installer) = vfs::find_winfsp_installer() {
                    println!("📦 偵測到驅動安裝套件: {}", installer.display());
                    println!("🚀 正在為您自動完成 WinFsp 核心驅動註冊 (請在 UAC 提示中確認)...");

                    #[cfg(target_os = "windows")]
                    if tray {
                        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OKCANCEL, IDOK};
                        use windows::core::w;
                        let msg = format!(
                            "未偵測到 WinFsp 核心檔案系統代理驅動程式 (winfsp.sys)。\n\n\
                            Rust RAM Disk 需要 WinFsp 核心驅動以建立虛擬磁碟機代號 ({})。\n\n\
                            點選【確定】將自動為您啟動驅動程式安裝程序 (請在接下來的 Windows 提示中點選【是】)。",
                            norm_mount
                        );
                        let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                        let ret = unsafe {
                            MessageBoxW(
                                None,
                                windows::core::PCWSTR(wide_msg.as_ptr()),
                                w!("Rust RAM Disk - 核心驅動程式設定"),
                                MB_ICONINFORMATION | MB_OKCANCEL,
                            )
                        };
                        if ret != IDOK {
                            return Err(anyhow::anyhow!("使用者取消驅動程式安裝，掛載中止"));
                        }
                    }

                    match vfs::install_winfsp_driver_silent() {
                        Ok(_) => {
                            println!("🎉 WinFsp 核心驅動已成功註冊完成！");
                        }
                        Err(e) => {
                            eprintln!("❌ 自動安裝驅動失敗: {}", e);
                            #[cfg(target_os = "windows")]
                            if tray {
                                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
                                use windows::core::w;
                                let err_msg = format!("WinFsp 核心驅動安裝未完成: {}\n\n無法建立虛擬磁碟機代號 {}", e, norm_mount);
                                let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                                unsafe {
                                    MessageBoxW(None, windows::core::PCWSTR(wide_err.as_ptr()), w!("Rust RAM Disk 錯誤"), MB_ICONERROR | MB_OK);
                                }
                            }
                            return Err(anyhow::anyhow!("WinFsp 核心驅動未安裝，掛載中止: {}", e));
                        }
                    }
                } else {
                    eprintln!("\n❌ 錯誤：未偵測到已安裝之 WinFsp 核心檔案系統代理驅動，且找不到安裝套件！");
                    eprintln!("   請先執行專案安裝套件 (RustRAMDisk.msi) 完整安裝驅動程式。\n");
                    #[cfg(target_os = "windows")]
                    if tray {
                        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
                        use windows::core::w;
                        let err_msg = format!("未偵測到 WinFsp 核心驅動，且未在安裝目錄找到 winfsp-installer.msi。\n\n無法建立虛擬磁碟機代號 {}", norm_mount);
                        let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                        unsafe {
                            MessageBoxW(None, windows::core::PCWSTR(wide_err.as_ptr()), w!("Rust RAM Disk 錯誤"), MB_ICONERROR | MB_OK);
                        }
                    }
                    return Err(anyhow::anyhow!("找不到 WinFsp 驅動與安裝套件，掛載中止"));
                }
            }

            // 2. 執行實體掛載
            let mut current_norm_mount = norm_mount.clone();
            let mut current_size = size.clone();
            let mut current_compress = compress;
            let mut current_image = image.clone();

            let mut mount_handle = vfs::mount_ram_disk(&current_norm_mount, capacity)
                .map_err(|e| anyhow::anyhow!("掛載失敗: {}", e))?;

            println!("🎉 虛擬磁碟機 {} 已成功掛載！", current_norm_mount);
            println!("   您現在可在 Windows 檔案總管與命令列直接讀寫磁碟機 {}。", current_norm_mount);

            // 3. 還原快照 (若指定且檔案存在)
            if let Some(ref img_path) = current_image {
                if img_path.exists() {
                    println!("💾 正在還原快照映像檔: {:?}", img_path);
                    match ImagePersistence::restore_drive_snapshot(img_path, &current_norm_mount) {
                        Ok(count) => {
                            println!("✅ 快照還原完成！共還原 {} 個檔案至 {}", count, current_norm_mount)
                        }
                        Err(e) => warn!("還原快照時發生錯誤: {}", e),
                    }
                }
            }

            // 4. 常駐管理 (系統匣 Systray 或 Console 模式)
            if tray {
                #[cfg(target_os = "windows")]
                {
                    use rust_ram_disk::tray::TrayManager;
                    use rust_ram_disk::ui::settings::{self, SettingsConfig, WM_APP_APPLY_SETTINGS};
                    use windows::core::{w, PCWSTR};
                    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
                    use windows::Win32::UI::WindowsAndMessaging::{
                        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                        MessageBoxW, MsgWaitForMultipleObjects, PeekMessageW, RegisterClassExW,
                        RegisterWindowMessageW, TranslateMessage, MB_ICONERROR, MB_ICONINFORMATION,
                        MB_OK, MSG, PM_REMOVE, QS_ALLINPUT, WNDCLASSEXW, WM_APP, WINDOW_EX_STYLE,
                        WINDOW_STYLE,
                    };

                    const WM_APP_RELOAD_TRAY: u32 = WM_APP + 101;

                    unsafe extern "system" fn host_wndproc(
                        hwnd: HWND,
                        msg: u32,
                        wparam: WPARAM,
                        lparam: LPARAM,
                    ) -> LRESULT {
                        DefWindowProcW(hwnd, msg, wparam, lparam)
                    }

                    // 註冊隱藏 Host 視窗類別，以接收工作列與二次執行訊息
                    let class_name = w!("RustRAMDisk_HostClass");
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

                    let host_title = format!("RustRAMDisk_Host_{}\0", current_norm_mount.trim_end_matches(':'));
                    let wide_title: Vec<u16> = host_title.encode_utf16().collect();
                    let host_hwnd = unsafe {
                        CreateWindowExW(
                            WINDOW_EX_STYLE::default(),
                            class_name,
                            PCWSTR(wide_title.as_ptr()),
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
                        TrayManager::new("Rust RAM Disk", &current_norm_mount, &current_size)?;
                    let mut menu_channel = muda::MenuEvent::receiver();
                    let tray_channel = tray_icon::TrayIconEvent::receiver();
                    println!("\n📌 系統匣圖示 (Systray) 已在右下角常駐。");
                    println!("   左鍵點擊圖示可開啟「設定畫面」，右鍵可快速儲存快照、卸載或結束。");

                    let open_settings = |h_opt: Option<HWND>, mount: &str, sz: &str, cp: CliCompression, img: &Option<PathBuf>| {
                        if let Some(h) = h_opt {
                            let cfg = SettingsConfig {
                                mount_point: mount.to_string(),
                                size: sz.to_string(),
                                compress: match cp {
                                    CliCompression::Lz4 => "lz4".to_string(),
                                    CliCompression::None => "none".to_string(),
                                },
                                image_path: img.as_ref().map(|p| p.to_string_lossy().to_string()),
                                auto_start: false,
                            };
                            settings::show_settings_window(h, &cfg);
                        }
                    };

                    let mut running = true;
                    while running {
                        let mut msg = MSG::default();
                        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                            // 1. 檢查是否收到「再次執行重載」或「Explorer 重新啟動 (TaskbarCreated)」通知
                            if msg.message == WM_APP_RELOAD_TRAY
                                || (taskbar_created_msg != 0 && msg.message == taskbar_created_msg)
                            {
                                tracing::info!("收到系統匣重載或工作列重建通知，正在重新整理圖示...");
                                match TrayManager::new("Rust RAM Disk", &current_norm_mount, &current_size) {
                                    Ok(new_tm) => {
                                        tray_manager = new_tm;
                                        menu_channel = muda::MenuEvent::receiver();
                                        tracing::info!("系統匣圖示已成功刷新重建。");
                                    }
                                    Err(e) => {
                                        error!("重新整理系統匣圖示失敗: {}", e);
                                    }
                                }
                            }
                            // 2. 檢查是否收到「套用設定並重新掛載」訊息
                            else if msg.message == WM_APP_APPLY_SETTINGS {
                                if let Some(new_cfg) = settings::take_pending_config() {
                                    println!("\n⚙️ 收到重新配置指令，正在進行動態套用與重新掛載...");
                                    println!("   新磁碟代號: {}", new_cfg.mount_point);
                                    println!("   新容量:     {}", new_cfg.size);
                                    println!("   新壓縮模式: {}", new_cfg.compress);

                                    let new_cap_res = parse_size(&new_cfg.size);
                                    let new_norm_res = vfs::normalize_mount_point(&new_cfg.mount_point);

                                    match (new_cap_res, new_norm_res) {
                                        (Ok(new_cap), Ok(new_norm)) => {
                                            if let Some(ref old_img) = current_image {
                                                println!("💾 卸載前先儲存目前快照至: {:?}", old_img);
                                                let _ = ImagePersistence::save_drive_snapshot(
                                                    &current_norm_mount,
                                                    old_img,
                                                    current_compress.into(),
                                                );
                                            }

                                            println!("⏏️ 正在安全卸載目前磁碟機 {}...", current_norm_mount);
                                            let _ = mount_handle.unmount();

                                            match vfs::mount_ram_disk(&new_norm, new_cap) {
                                                Ok(new_handle) => {
                                                    mount_handle = new_handle;
                                                    current_norm_mount = new_norm.clone();
                                                    current_size = new_cfg.size.clone();
                                                    current_compress = if new_cfg.compress == "none" {
                                                        CliCompression::None
                                                    } else {
                                                        CliCompression::Lz4
                                                    };
                                                    current_image = new_cfg.image_path.as_ref().map(PathBuf::from);

                                                    if let Some(ref img_path) = current_image {
                                                        if img_path.exists() {
                                                            println!("💾 正在還原快照映像檔: {:?}", img_path);
                                                            let _ = ImagePersistence::restore_drive_snapshot(
                                                                img_path,
                                                                &current_norm_mount,
                                                            );
                                                        }
                                                    }

                                                    let new_status_text = format!(
                                                        "Rust RAM Disk: {} ({})",
                                                        current_norm_mount, current_size
                                                    );
                                                    tray_manager.status_item.set_text(&new_status_text);
                                                    let _ = tray_manager.tray_icon().set_tooltip(Some(format!(
                                                        "Rust RAM Disk - {}",
                                                        current_norm_mount
                                                    )));

                                                    println!(
                                                        "🎉 虛擬磁碟機已成功重新掛載為 {} (容量: {})！",
                                                        current_norm_mount, current_size
                                                    );

                                                    let tip = format!(
                                                        "虛擬磁碟機已成功重新掛載！\n\n磁碟代號：{}\n容量大小：{}\n壓縮模式：{}",
                                                        current_norm_mount, current_size, new_cfg.compress
                                                    );
                                                    let wide_tip: Vec<u16> = tip
                                                        .encode_utf16()
                                                        .chain(std::iter::once(0))
                                                        .collect();
                                                    unsafe {
                                                        MessageBoxW(
                                                            None,
                                                            PCWSTR(wide_tip.as_ptr()),
                                                            w!("Rust RAM Disk - 設定已套用"),
                                                            MB_ICONINFORMATION | MB_OK,
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("重新掛載失敗: {}", e);
                                                    let err_tip = format!("重新掛載磁碟機 {} 失敗: {}\n\n請確認磁碟代號未被佔用。", new_norm, e);
                                                    let wide_err: Vec<u16> = err_tip
                                                        .encode_utf16()
                                                        .chain(std::iter::once(0))
                                                        .collect();
                                                    unsafe {
                                                        MessageBoxW(
                                                            None,
                                                            PCWSTR(wide_err.as_ptr()),
                                                            w!("Rust RAM Disk 錯誤"),
                                                            MB_ICONERROR | MB_OK,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        (Err(e), _) => {
                                            error!("容量格式錯誤: {}", e);
                                            let err_tip = format!("容量格式無效: {}\n請使用例如 1G, 2G, 512M", e);
                                            let wide_err: Vec<u16> = err_tip
                                                .encode_utf16()
                                                .chain(std::iter::once(0))
                                                .collect();
                                            unsafe {
                                                MessageBoxW(
                                                    None,
                                                    PCWSTR(wide_err.as_ptr()),
                                                    w!("Rust RAM Disk 錯誤"),
                                                    MB_ICONERROR | MB_OK,
                                                );
                                            }
                                        }
                                        (_, Err(e)) => {
                                            error!("磁碟代號格式錯誤: {}", e);
                                        }
                                    }
                                }
                            }

                            unsafe {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }

                        // 3. 處理系統匣滑鼠點擊事件 (左鍵單擊/雙擊開啟設定畫面)
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
                                    open_settings(
                                        host_hwnd,
                                        &current_norm_mount,
                                        &current_size,
                                        current_compress,
                                        &current_image,
                                    );
                                }
                                _ => {}
                            }
                        }

                        // 4. 處理系統匣右鍵選單項目點擊
                        while let Ok(event) = menu_channel.try_recv() {
                            if event.id == tray_manager.exit_item.id()
                                || event.id == tray_manager.unmount_item.id()
                            {
                                println!("🛑 收到系統匣卸載與退出指令。");
                                running = false;
                                break;
                            } else if event.id == tray_manager.settings_item.id() {
                                open_settings(
                                    host_hwnd,
                                    &current_norm_mount,
                                    &current_size,
                                    current_compress,
                                    &current_image,
                                );
                            } else if event.id == tray_manager.snapshot_item.id() {
                                let target_img = current_image
                                    .as_deref()
                                    .unwrap_or_else(|| Path::new("ramdisk_snapshot.img"));
                                match ImagePersistence::save_drive_snapshot(
                                    &current_norm_mount,
                                    target_img,
                                    current_compress.into(),
                                ) {
                                    Ok(count) => println!(
                                        "💾 快照儲存完成！已打包 {} 個檔案至 {:?}",
                                        count, target_img
                                    ),
                                    Err(e) => error!("儲存快照失敗: {}", e),
                                }
                            }
                        }

                        // 檢查底層掛載程序是否意外中斷
                        if !mount_handle.is_alive() {
                            eprintln!("⚠️ 偵測到掛載服務中斷，退出系統匣。");
                            break;
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
            } else {
                println!("🚀 RAM Disk 已以 Console 模式常駐。按下 Ctrl+C 即可終止並安全卸載。");
                let running = Arc::new(AtomicBool::new(true));
                let r = running.clone();
                ctrlc::set_handler(move || {
                    r.store(false, Ordering::SeqCst);
                })?;
                while running.load(Ordering::SeqCst) && mount_handle.is_alive() {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }

            // 5. 結束前自動回存快照與安全卸載
            if let Some(ref img_path) = current_image {
                println!("💾 退出前自動儲存最新快照至: {:?}", img_path);
                if let Err(e) =
                    ImagePersistence::save_drive_snapshot(&current_norm_mount, img_path, current_compress.into())
                {
                    error!("自動儲存快照失敗: {}", e);
                } else {
                    println!("✅ 快照已成功更新。");
                }
            }

            println!("⏏️ 正在安全卸載磁碟機 {}...", current_norm_mount);
            let _ = mount_handle.unmount();
            println!("✅ 磁碟機 {} 已完全卸載，程式結束。", current_norm_mount);
        }
    }

    Ok(())
}
