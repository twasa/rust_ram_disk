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
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
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

            // 1. 驅動環境檢核
            if !vfs::is_driver_installed() {
                eprintln!("\n❌ 錯誤：未偵測到已安裝之 WinFsp 核心檔案系統代理驅動！");
                eprintln!("   Windows 必須註冊 WinFsp 核心驅動 (winfsp.sys) 才能建立虛擬磁碟機代號。");
                eprintln!("------------------------------------------------------------");
                eprintln!("👉 解決方式：");
                if let Some(installer) = vfs::find_winfsp_installer() {
                    eprintln!("   1. 請先執行專案安裝套件註冊驅動：");
                    eprintln!("      {}", installer.display());
                } else {
                    eprintln!("   1. 請安裝本專案提供的安裝套件 (RustRAMDisk.msi)");
                }
                eprintln!("   2. 安裝完成後，重新執行本指令即可正常看見 {} 磁碟機。\n", norm_mount);
                return Err(anyhow::anyhow!("WinFsp 核心驅動未安裝，掛載中止"));
            }

            // 2. 執行實體掛載
            let mut mount_handle = vfs::mount_ram_disk(&norm_mount, capacity)
                .map_err(|e| anyhow::anyhow!("掛載失敗: {}", e))?;

            println!("🎉 虛擬磁碟機 {} 已成功掛載！", norm_mount);
            println!("   您現在可在 Windows 檔案總管與命令列直接讀寫磁碟機 {}。", norm_mount);

            // 3. 還原快照 (若指定且檔案存在)
            if let Some(ref img_path) = image {
                if img_path.exists() {
                    println!("💾 正在還原快照映像檔: {:?}", img_path);
                    match ImagePersistence::restore_drive_snapshot(img_path, &norm_mount) {
                        Ok(count) => {
                            println!("✅ 快照還原完成！共還原 {} 個檔案至 {}", count, norm_mount)
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
                    use windows::core::{w, PCWSTR};
                    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
                    use windows::Win32::UI::WindowsAndMessaging::{
                        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                        MsgWaitForMultipleObjects, PeekMessageW, RegisterClassExW, RegisterWindowMessageW,
                        TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT, WNDCLASSEXW, WM_APP,
                        WINDOW_EX_STYLE, WINDOW_STYLE,
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

                    let host_title = format!("RustRAMDisk_Host_{}\0", norm_mount.trim_end_matches(':'));
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
                        TrayManager::new("Rust RAM Disk", &norm_mount, &size)?;
                    let mut menu_channel = muda::MenuEvent::receiver();
                    println!("\n📌 系統匣圖示 (Systray) 已在右下角常駐。");
                    println!("   右鍵點擊系統匣圖示可隨時儲存快照、卸載磁碟或退出程式。");

                    let mut running = true;
                    while running {
                        let mut msg = MSG::default();
                        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                            // 檢查是否收到「再次執行重載」或「Explorer 重新啟動 (TaskbarCreated)」通知
                            if msg.message == WM_APP_RELOAD_TRAY
                                || (taskbar_created_msg != 0 && msg.message == taskbar_created_msg)
                            {
                                tracing::info!("收到系統匣重載或工作列重建通知，正在重新整理圖示...");
                                match TrayManager::new("Rust RAM Disk", &norm_mount, &size) {
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

                            unsafe {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }

                        while let Ok(event) = menu_channel.try_recv() {
                            if event.id == tray_manager.exit_item.id()
                                || event.id == tray_manager.unmount_item.id()
                            {
                                println!("🛑 收到系統匣卸載與退出指令。");
                                running = false;
                                break;
                            } else if event.id == tray_manager.snapshot_item.id() {
                                let target_img = image
                                    .as_deref()
                                    .unwrap_or_else(|| Path::new("ramdisk_snapshot.img"));
                                match ImagePersistence::save_drive_snapshot(
                                    &norm_mount,
                                    target_img,
                                    compress.into(),
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
            if let Some(ref img_path) = image {
                println!("💾 退出前自動儲存最新快照至: {:?}", img_path);
                if let Err(e) =
                    ImagePersistence::save_drive_snapshot(&norm_mount, img_path, compress.into())
                {
                    error!("自動儲存快照失敗: {}", e);
                } else {
                    println!("✅ 快照已成功更新。");
                }
            }

            println!("⏏️ 正在安全卸載磁碟機 {}...", norm_mount);
            let _ = mount_handle.unmount();
            println!("✅ 磁碟機 {} 已完全卸載，程式結束。", norm_mount);
        }
    }

    Ok(())
}
