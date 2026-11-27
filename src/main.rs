use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use tracing::info;

use rust_ram_disk::engine::{CompressionMode, SparseMemoryStorage, StorageEngine};
use rust_ram_disk::persistence::ImagePersistence;

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

        /// Windows drive letter or mount point
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
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Benchmark { size, compress } => {
            let cap = parse_size(&size).map_err(|e| anyhow::anyhow!(e))?;
            println!("============================================================");
            println!("🚀 Rust RAM Disk Storage Engine In-Memory Benchmark");
            println!("   Virtual Capacity : {} bytes ({:.2} MB)", cap, cap as f64 / (1024.0 * 1024.0));
            println!("   Compression      : {:?}", compress);
            println!("============================================================");

            let storage = SparseMemoryStorage::new(cap, 4096, compress.into());

            // 1. Sequential Write Benchmark (64 KB chunks)
            let chunk_size = 64 * 1024;
            let total_test_bytes = cap.min(256 * 1024 * 1024); // up to 256MB
            let iterations = (total_test_bytes / chunk_size as u64) as usize;
            let write_buf: Vec<u8> = (0..chunk_size).map(|i| (i % 251) as u8).collect();

            print!("-> Testing Sequential Write ({} MB)... ", total_test_bytes / (1024 * 1024));
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
            print!("-> Testing Sequential Read ({} MB)... ", total_test_bytes / (1024 * 1024));
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
            println!("Physical Memory Allocated : {} KB", storage.physical_memory_used() / 1024);
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
            info!(
                "Initializing RAM Disk: Capacity={:.2} MB, MountPoint={}, Compression={:?}",
                capacity as f64 / (1024.0 * 1024.0),
                mount_point,
                compress
            );

            let storage = if let Some(ref img_path) = image {
                if img_path.exists() {
                    info!("Loading snapshot image from: {:?}", img_path);
                    ImagePersistence::load_sparse(img_path, compress.into())?
                } else {
                    info!("Snapshot file not found; creating fresh disk: {:?}", img_path);
                    SparseMemoryStorage::new(capacity, 4096, compress.into())
                }
            } else {
                SparseMemoryStorage::new(capacity, 4096, compress.into())
            };

            let storage = Arc::new(storage);

            if tray {
                #[cfg(target_os = "windows")]
                {
                    use rust_ram_disk::tray::TrayManager;
                    let tray_manager = TrayManager::new("Rust RAM Disk", &mount_point, &size)?;
                    let menu_channel = muda::MenuEvent::receiver();
                    println!("🚀 RAM Disk running. System Tray icon active in taskbar.");
                    println!("   Right click tray icon to inspect status, save snapshot or exit.");

                    unsafe {
                        use windows::Win32::UI::WindowsAndMessaging::{
                            DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW,
                            TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT,
                        };

                        let mut running = true;
                        while running {
                            let mut msg = MSG::default();
                            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }

                            while let Ok(event) = menu_channel.try_recv() {
                                if event.id == tray_manager.exit_item.id()
                                    || event.id == tray_manager.unmount_item.id()
                                {
                                    println!("🛑 Exit/Unmount requested from tray menu.");
                                    running = false;
                                    break;
                                } else if event.id == tray_manager.snapshot_item.id() {
                                    if let Some(ref img_path) = image {
                                        let _ = ImagePersistence::save_sparse(&storage, img_path);
                                        println!("💾 Snapshot saved to {:?}", img_path);
                                    } else {
                                        let fallback = PathBuf::from("ramdisk_snapshot.img");
                                        let _ = ImagePersistence::save_sparse(&storage, &fallback);
                                        println!("💾 Snapshot saved to {:?}", fallback);
                                    }
                                }
                            }

                            let _ = MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
                        }
                    }
                }
            } else {
                println!("🚀 RAM Disk started without Tray GUI. Press Ctrl+C to terminate.");
                let running = Arc::new(AtomicBool::new(true));
                let r = running.clone();
                ctrlc::set_handler(move || {
                    r.store(false, Ordering::SeqCst);
                })?;
                while running.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }

            if let Some(ref img_path) = image {
                info!("Saving snapshot before unmounting to: {:?}", img_path);
                ImagePersistence::save_sparse(&storage, img_path)?;
                println!("💾 Snapshot successfully saved to {:?}", img_path);
            }
        }
    }

    Ok(())
}
