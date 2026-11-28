use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows::Win32::System::Services::*;

use crate::engine::CompressionMode;
use crate::logger::global_logger;
use crate::persistence::ImagePersistence;
use crate::vfs;

pub const SERVICE_NAME: PCWSTR = w!("RustRamDiskService");
pub const SERVICE_DISPLAY_NAME: PCWSTR = w!("Rust RAM Disk Service");
pub const SERVICE_DESCRIPTION_STR: PCWSTR =
    w!("High-Performance In-Memory RAM Disk Managed Service with Dynamic Sparse Allocation");

/// 自訂服務控制碼：觸發即時快照儲存
pub const SERVICE_CONTROL_CUSTOM_SNAPSHOT: u32 = 128;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("Windows API 錯誤 (Win32 Error: {0:#X})")]
    Win32(u32),

    #[error("無法開啟服務控制管理員 (SCM)，可能需要系統管理員權限: {0:#X}")]
    OpenScmFailed(u32),

    #[error("服務尚未安裝")]
    NotInstalled,

    #[error("服務已在運行中")]
    AlreadyRunning,

    #[error("服務未能及時啟動 (逾時)")]
    StartTimeout,

    #[error("服務未能及時停止 (逾時)")]
    StopTimeout,

    #[error("掛載虛擬磁碟失敗: {0}")]
    MountFailed(String),

    #[error("IO 或設定錯誤: {0}")]
    Io(#[from] std::io::Error),

    #[error("設定序列化錯誤: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    ContinuePending,
    PausePending,
    Paused,
    Unknown(u32),
}

impl std::fmt::Display for ServiceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceState::NotInstalled => write!(f, "未安裝 (Not Installed)"),
            ServiceState::Stopped => write!(f, "已停止 (Stopped)"),
            ServiceState::StartPending => write!(f, "啟動中 (Starting...)"),
            ServiceState::StopPending => write!(f, "停止中 (Stopping...)"),
            ServiceState::Running => write!(f, "運行中 (Running)"),
            ServiceState::ContinuePending => write!(f, "繼續中 (Continuing...)"),
            ServiceState::PausePending => write!(f, "暫停中 (Pausing...)"),
            ServiceState::Paused => write!(f, "已暫停 (Paused)"),
            ServiceState::Unknown(code) => write!(f, "未知狀態 ({})", code),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceConfig {
    pub mount_point: String,
    pub size: String,
    pub compress: String,
    pub image_path: Option<String>,
    pub auto_start: bool,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            mount_point: "R:".to_string(),
            size: "2G".to_string(),
            compress: "lz4".to_string(),
            image_path: None,
            auto_start: true,
        }
    }
}

impl ServiceConfig {
    pub fn config_path() -> PathBuf {
        let base = std::env::var("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\ProgramData"));
        base.join("RustRamDisk").join("config.json")
    }

    pub fn load() -> Self {
        let p = Self::config_path();
        if p.exists() {
            if let Ok(content) = fs::read_to_string(&p) {
                if let Ok(cfg) = serde_json::from_str::<ServiceConfig>(&content) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), ServiceError> {
        let p = Self::config_path();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&p, json)?;
        Ok(())
    }
}

pub fn parse_size_bytes(input: &str) -> Result<u64, String> {
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

// ---------------------------------------------------------------------------
// SCM 管理函式 (查詢、安裝、反註冊、啟動、停止、重新啟動)
// ---------------------------------------------------------------------------

/// 查詢 Rust RAM Disk Windows 系統服務當前狀態
pub fn query_status() -> Result<ServiceState, ServiceError> {
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;

        let service = match OpenServiceW(scm, SERVICE_NAME, SERVICE_QUERY_STATUS) {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.into() {
                    return Ok(ServiceState::NotInstalled);
                }
                return Err(ServiceError::Win32(e.code().0 as u32));
            }
        };

        let mut status_process = SERVICE_STATUS_PROCESS::default();
        let mut bytes_needed = 0u32;
        let slice = std::slice::from_raw_parts_mut(
            &mut status_process as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
        );
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            Some(slice),
            &mut bytes_needed,
        );

        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);

        if let Err(e) = ok {
            return Err(ServiceError::Win32(e.code().0 as u32));
        }

        let state = match status_process.dwCurrentState {
            SERVICE_STOPPED => ServiceState::Stopped,
            SERVICE_START_PENDING => ServiceState::StartPending,
            SERVICE_STOP_PENDING => ServiceState::StopPending,
            SERVICE_RUNNING => ServiceState::Running,
            SERVICE_CONTINUE_PENDING => ServiceState::ContinuePending,
            SERVICE_PAUSE_PENDING => ServiceState::PausePending,
            SERVICE_PAUSED => ServiceState::Paused,
            other => ServiceState::Unknown(other.0),
        };

        Ok(state)
    }
}

/// 註冊並安裝為 Windows 系統服務
pub fn install(config: &ServiceConfig, start_now: bool) -> Result<(), ServiceError> {
    // 1. 儲存全域設定檔至 ProgramData
    config.save()?;

    // 2. 取得目前執行檔路徑，組裝 binary path
    let current_exe = std::env::current_exe()?;
    let exe_str = current_exe.to_string_lossy();
    let bin_path_cmd = format!("\"{}\" service run", exe_str);
    let wide_bin_path: Vec<u16> = bin_path_cmd.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;

        // 檢查是否已存在，若存在先刪除重建
        if let Ok(old_svc) = OpenServiceW(scm, SERVICE_NAME, SERVICE_ALL_ACCESS) {
            info!("偵測到舊有服務已存在，先進行停止與移除...");
            let mut status = SERVICE_STATUS::default();
            let _ = ControlService(old_svc, SERVICE_CONTROL_STOP, &mut status);
            let _ = DeleteService(old_svc);
            let _ = CloseServiceHandle(old_svc);
            std::thread::sleep(Duration::from_millis(500));
        }

        let start_type = if config.auto_start {
            SERVICE_AUTO_START
        } else {
            SERVICE_DEMAND_START
        };

        let service = CreateServiceW(
            scm,
            SERVICE_NAME,
            SERVICE_DISPLAY_NAME,
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            start_type,
            SERVICE_ERROR_NORMAL,
            PCWSTR(wide_bin_path.as_ptr()),
            None,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| {
            let _ = CloseServiceHandle(scm);
            ServiceError::Win32(e.code().0 as u32)
        })?;

        // 設定服務說明 (Description)
        let mut desc = SERVICE_DESCRIPTIONW {
            lpDescription: PWSTR(SERVICE_DESCRIPTION_STR.as_ptr() as *mut _),
        };
        let _ = ChangeServiceConfig2W(
            service,
            SERVICE_CONFIG_DESCRIPTION,
            Some(&mut desc as *mut _ as *const c_void),
        );

        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);
        info!("Rust RAM Disk 系統服務註冊成功！");
    }

    if start_now {
        info!("立即啟動服務...");
        start()?;
    }

    Ok(())
}

/// 解除註冊並移除 Windows 系統服務
pub fn uninstall() -> Result<(), ServiceError> {
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;

        let service = match OpenServiceW(scm, SERVICE_NAME, SERVICE_ALL_ACCESS) {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.into() {
                    return Ok(());
                }
                return Err(ServiceError::Win32(e.code().0 as u32));
            }
        };

        // 若仍在運行，先發送停止指令
        let mut status = SERVICE_STATUS::default();
        let _ = ControlService(service, SERVICE_CONTROL_STOP, &mut status);
        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);

        // 等待停止完成
        let start_wait = Instant::now();
        while start_wait.elapsed() < Duration::from_secs(10) {
            match query_status()? {
                ServiceState::Stopped | ServiceState::NotInstalled => break,
                _ => std::thread::sleep(Duration::from_millis(200)),
            }
        }

        let scm2 = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;
        if let Ok(svc2) = OpenServiceW(scm2, SERVICE_NAME, SERVICE_ALL_ACCESS) {
            let _ = DeleteService(svc2);
            let _ = CloseServiceHandle(svc2);
        }
        let _ = CloseServiceHandle(scm2);
        info!("Rust RAM Disk 系統服務已成功移除。");
    }

    Ok(())
}

/// 啟動 Windows 系統服務
pub fn start() -> Result<(), ServiceError> {
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;

        let service = OpenServiceW(scm, SERVICE_NAME, SERVICE_START | SERVICE_QUERY_STATUS)
            .map_err(|e| {
                let _ = CloseServiceHandle(scm);
                ServiceError::Win32(e.code().0 as u32)
            })?;

        let start_res = StartServiceW(service, None);
        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);

        if let Err(e) = start_res {
            let code = e.code().0 as u32;
            // 0x420 is ERROR_SERVICE_ALREADY_RUNNING
            if code != 0x420 {
                return Err(ServiceError::Win32(code));
            }
        }
    }

    // 等待服務轉為 Running 狀態 (最多 15 秒)
    let start_wait = Instant::now();
    while start_wait.elapsed() < Duration::from_secs(15) {
        match query_status()? {
            ServiceState::Running => {
                info!("Rust RAM Disk 服務已成功啟動！");
                return Ok(());
            }
            ServiceState::Stopped => {
                return Err(ServiceError::MountFailed(
                    "服務啟動後立即終止，請檢閱服務日誌以獲取詳細崩潰原因".to_string(),
                ));
            }
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }

    Err(ServiceError::StartTimeout)
}

/// 停止 Windows 系統服務
pub fn stop() -> Result<(), ServiceError> {
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
            .map_err(|e| ServiceError::OpenScmFailed(e.code().0 as u32))?;

        let service = match OpenServiceW(scm, SERVICE_NAME, SERVICE_STOP | SERVICE_QUERY_STATUS) {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.into() {
                    return Ok(());
                }
                return Err(ServiceError::Win32(e.code().0 as u32));
            }
        };

        let mut status = SERVICE_STATUS::default();
        let _ = ControlService(service, SERVICE_CONTROL_STOP, &mut status);
        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);
    }

    // 等待停止完成 (最多 15 秒)
    let start_wait = Instant::now();
    while start_wait.elapsed() < Duration::from_secs(15) {
        match query_status()? {
            ServiceState::Stopped | ServiceState::NotInstalled => {
                info!("Rust RAM Disk 服務已停止。");
                return Ok(());
            }
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }

    Err(ServiceError::StopTimeout)
}

/// 重新啟動 Windows 系統服務
pub fn restart() -> Result<(), ServiceError> {
    let _ = stop();
    start()
}

/// 觸發即時快照儲存
pub fn trigger_snapshot() -> Result<(), ServiceError> {
    let cfg = ServiceConfig::load();
    if let Some(ref img_path_str) = cfg.image_path {
        let comp = match cfg.compress.to_lowercase().as_str() {
            "none" => CompressionMode::None,
            _ => CompressionMode::Lz4,
        };
        ImagePersistence::save_drive_snapshot(&cfg.mount_point, Path::new(img_path_str), comp)
            .map_err(|e| ServiceError::MountFailed(format!("儲存快照失敗: {}", e)))?;
        Ok(())
    } else {
        Err(ServiceError::MountFailed("未設定快照存檔路徑".to_string()))
    }
}

// ---------------------------------------------------------------------------
// 服務主體與分發常駐邏輯 (執行於 Session 0 系統服務環境)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct SendServiceStatusHandle(SERVICE_STATUS_HANDLE);
unsafe impl Send for SendServiceStatusHandle {}
unsafe impl Sync for SendServiceStatusHandle {}

static SERVICE_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static SERVICE_SNAPSHOT_REQUESTED: AtomicBool = AtomicBool::new(false);
static SERVICE_STATUS_HANDLE_GLOBAL: Mutex<Option<SendServiceStatusHandle>> = Mutex::new(None);

fn set_service_status_state(current_state: SERVICE_STATUS_CURRENT_STATE, controls_accepted: u32) {
    if let Some(wrapped) = *SERVICE_STATUS_HANDLE_GLOBAL.lock() {
        let mut status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: current_state,
            dwControlsAccepted: controls_accepted,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 0,
        };
        unsafe {
            let _ = SetServiceStatus(wrapped.0, &mut status);
        }
    }
}

unsafe extern "system" fn service_ctrl_handler(
    dwcontrol: u32,
    _dweventtype: u32,
    _lpeventdata: *mut c_void,
    _lpcontext: *mut c_void,
) -> u32 {
    match dwcontrol {
        c if c == SERVICE_CONTROL_STOP || c == SERVICE_CONTROL_SHUTDOWN => {
            set_service_status_state(SERVICE_STOP_PENDING, 0);
            SERVICE_STOP_REQUESTED.store(true, Ordering::SeqCst);
            0
        }
        SERVICE_CONTROL_CUSTOM_SNAPSHOT => {
            SERVICE_SNAPSHOT_REQUESTED.store(true, Ordering::SeqCst);
            0
        }
        c if c == SERVICE_CONTROL_INTERROGATE => 0,
        _ => 0,
    }
}

unsafe extern "system" fn service_main_entry(
    _dwnumserviceargs: u32,
    _lpserviceargvectors: *mut PWSTR,
) {
    let app_logger = global_logger();
    app_logger.log("INFO", "service", "entry", "Rust RAM Disk 服務 main 進入點執行", None);

    let status_handle = match RegisterServiceCtrlHandlerExW(
        SERVICE_NAME,
        Some(service_ctrl_handler),
        None,
    ) {
        Ok(h) => h,
        Err(e) => {
            app_logger.log(
                "ERROR",
                "service",
                "register_handler_failed",
                &format!("RegisterServiceCtrlHandlerExW 失敗: {:?}", e),
                None,
            );
            return;
        }
    };

    *SERVICE_STATUS_HANDLE_GLOBAL.lock() = Some(SendServiceStatusHandle(status_handle));
    set_service_status_state(SERVICE_START_PENDING, 0);

    let config = ServiceConfig::load();
    app_logger.log(
        "INFO",
        "service",
        "config_loaded",
        &format!("服務設定載入完成: {:?}", config),
        None,
    );

    let capacity = match parse_size_bytes(&config.size) {
        Ok(c) => c,
        Err(e) => {
            app_logger.log("ERROR", "service", "parse_size_error", &e, None);
            set_service_status_state(SERVICE_STOPPED, 0);
            return;
        }
    };

    // 1. 若有快照設定，先嘗試載入快照檔案
    let comp_mode = match config.compress.to_lowercase().as_str() {
        "none" => CompressionMode::None,
        _ => CompressionMode::Lz4,
    };

    // 2. 啟動 WinFsp MemFS 掛載
    let mut mount_handle = match vfs::mount_ram_disk(&config.mount_point, capacity) {
        Ok(h) => h,
        Err(e) => {
            app_logger.log(
                "ERROR",
                "service",
                "mount_failed",
                &format!("掛載失敗: {}", e),
                None,
            );
            set_service_status_state(SERVICE_STOPPED, 0);
            return;
        }
    };

    // 3. 掛載成功後若有快照，自動回補還原快照內容
    if let Some(ref img_path_str) = config.image_path {
        let img_path = PathBuf::from(img_path_str);
        if img_path.exists() {
            app_logger.log(
                "INFO",
                "service",
                "restoring_snapshot",
                &format!("從快照還原至 {}", config.mount_point),
                None,
            );
            let _ = ImagePersistence::restore_drive_snapshot(&img_path, &config.mount_point);
        }
    }

    // 4. 回報服務正在運行 (SERVICE_RUNNING)
    set_service_status_state(
        SERVICE_RUNNING,
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
    );
    app_logger.log(
        "INFO",
        "service",
        "running",
        &format!("Rust RAM Disk 服務已就緒並維持掛載: {}", config.mount_point),
        None,
    );

    // 5. 常駐監控迴圈
    while !SERVICE_STOP_REQUESTED.load(Ordering::SeqCst) {
        // 檢查是否有即時快照請求
        if SERVICE_SNAPSHOT_REQUESTED.swap(false, Ordering::SeqCst) {
            if let Some(ref img_path_str) = config.image_path {
                app_logger.log("INFO", "service", "snapshot_triggered", "執行即時快照存檔...", None);
                let _ = ImagePersistence::save_drive_snapshot(
                    &config.mount_point,
                    Path::new(img_path_str),
                    comp_mode,
                );
            }
        }

        // 檢查掛載行程是否仍存活
        if !mount_handle.is_alive() {
            app_logger.log("ERROR", "service", "memfs_died", "WinFsp MemFS 程序意外結束！", None);
            break;
        }

        std::thread::sleep(Duration::from_millis(500));
    }

    // 6. 服務即將停止：回存快照與卸載磁碟
    set_service_status_state(SERVICE_STOP_PENDING, 0);
    app_logger.log("INFO", "service", "stopping", "服務收到停止通知，開始安全處置...", None);

    if let Some(ref img_path_str) = config.image_path {
        app_logger.log("INFO", "service", "save_snapshot_on_stop", "停止前儲存快照...", None);
        let _ = ImagePersistence::save_drive_snapshot(
            &config.mount_point,
            Path::new(img_path_str),
            comp_mode,
        );
    }

    let _ = mount_handle.unmount();
    app_logger.log("INFO", "service", "stopped", "RAM Disk 卸載完成，服務完全停止", None);
    set_service_status_state(SERVICE_STOPPED, 0);
}

/// SCM 呼叫之服務分發器入口 (由 `rust_ram_disk service run` 調用)
pub fn run_service() -> Result<(), ServiceError> {
    let mut service_name_wide: Vec<u16> = "RustRamDiskService"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(service_name_wide.as_mut_ptr()),
            lpServiceProc: Some(service_main_entry),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(std::ptr::null_mut()),
            lpServiceProc: None,
        },
    ];

    unsafe {
        StartServiceCtrlDispatcherW(table.as_ptr())
            .map_err(|e| ServiceError::Win32(e.code().0 as u32))?;
    }

    Ok(())
}
