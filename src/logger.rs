use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const MAX_LOG_FILE_BYTES: u64 = 5 * 1024 * 1024; // 單一檔案上限 5 MB，超過自動輪替 (Rotation)

/// 日誌輸出格式（預設為 JSON Lines `.jsonl`，便於收集、查詢與解析）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Json,
    Text,
}

impl Default for LogFormat {
    fn default() -> Self {
        LogFormat::Json
    }
}

/// 日誌等級
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
    Panic = 4,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl LogLevel {
    pub fn parse_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "DEBUG" => LogLevel::Debug,
            "INFO" => LogLevel::Info,
            "WARN" | "WARNING" => LogLevel::Warn,
            "PANIC" | "FATAL" => LogLevel::Panic,
            _ => LogLevel::Error,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Panic => "PANIC",
        }
    }
}

/// 單筆結構化日誌記錄
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredLogRecord {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub event: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    pub pid: u32,
    pub os: String,
}

pub struct AppLogger {
    format: Mutex<LogFormat>,
    level: Mutex<LogLevel>,
    write_lock: Mutex<()>,
    log_path: PathBuf,
}

static GLOBAL_LOGGER: OnceLock<Arc<AppLogger>> = OnceLock::new();

pub fn global_logger() -> Arc<AppLogger> {
    GLOBAL_LOGGER
        .get_or_init(|| AppLogger::new())
        .clone()
}

impl AppLogger {
    pub fn new() -> Arc<Self> {
        let log_dir = resolve_log_directory();
        let _ = fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("rust_ram_disk.jsonl");

        let logger = Arc::new(Self {
            format: Mutex::new(LogFormat::Json),
            level: Mutex::new(LogLevel::Info),
            write_lock: Mutex::new(()),
            log_path,
        });

        logger
    }

    /// 安裝全域 Panic 攔截器，確保崩潰前寫入日誌檔
    pub fn install_panic_hook(logger: Arc<Self>) {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let location = panic_info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".to_string());

            let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };

            logger.log(
                "PANIC",
                "backend::panic",
                "unhandled_panic",
                &format!("應用程式發生未預期 Panic: {}", payload),
                Some(json!({
                    "location": location,
                    "thread": std::thread::current().name().unwrap_or("unnamed")
                })),
            );
            prev_hook(panic_info);
        }));
    }

    pub fn set_format(&self, fmt: LogFormat) {
        *self.format.lock() = fmt;
        self.log(
            "INFO",
            "backend::logger",
            "log_format_changed",
            &format!("日誌輸出格式已切換為 {:?}", fmt),
            None,
        );
    }

    pub fn get_format(&self) -> LogFormat {
        *self.format.lock()
    }

    pub fn set_level(&self, level: LogLevel) {
        *self.level.lock() = level;
    }

    pub fn get_level(&self) -> LogLevel {
        *self.level.lock()
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn get_log_path_str(&self) -> String {
        self.log_path.to_string_lossy().to_string()
    }

    pub fn log(
        &self,
        level: &str,
        target: &str,
        event: &str,
        message: &str,
        context: Option<Value>,
    ) {
        let rec_level = LogLevel::parse_str(level);
        if rec_level < *self.level.lock() {
            return;
        }

        let record = StructuredLogRecord {
            timestamp: Utc::now().to_rfc3339(),
            level: level.to_ascii_uppercase(),
            target: target.to_string(),
            event: event.to_string(),
            message: message.to_string(),
            context,
            pid: std::process::id(),
            os: std::env::consts::OS.to_string(),
        };

        let _guard = self.write_lock.lock();
        self.rotate_if_needed();

        if let Some(parent) = self.log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let fmt = *self.format.lock();
            match fmt {
                LogFormat::Json => {
                    if let Ok(json_line) = serde_json::to_string(&record) {
                        let _ = writeln!(file, "{}", json_line);
                    }
                }
                LogFormat::Text => {
                    let ctx_str = record
                        .context
                        .as_ref()
                        .map(|c| format!(" | context={}", c))
                        .unwrap_or_default();
                    let _ = writeln!(
                        file,
                        "{} [{:<5}] [{}] ({}) {}{}",
                        record.timestamp,
                        record.level,
                        record.target,
                        record.event,
                        record.message,
                        ctx_str
                    );
                }
            }
        }
    }

    fn rotate_if_needed(&self) {
        if let Ok(meta) = fs::metadata(&self.log_path) {
            if meta.len() >= MAX_LOG_FILE_BYTES {
                let backup = self.log_path.with_extension("jsonl.1");
                let _ = fs::rename(&self.log_path, backup);
            }
        }
    }

    /// 讀取最近 N 筆日誌記錄
    pub fn read_recent_logs(&self, limit: usize) -> Vec<StructuredLogRecord> {
        let _guard = self.write_lock.lock();
        if !self.log_path.exists() {
            return Vec::new();
        }
        let file = match fs::File::open(&self.log_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(rec) = serde_json::from_str::<StructuredLogRecord>(trimmed) {
                records.push(rec);
            } else {
                records.push(StructuredLogRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    level: "INFO".to_string(),
                    target: "raw_text".to_string(),
                    event: "legacy_line".to_string(),
                    message: trimmed.to_string(),
                    context: None,
                    pid: std::process::id(),
                    os: std::env::consts::OS.to_string(),
                });
            }
        }

        let max_items = limit.clamp(1, 1000);
        if records.len() > max_items {
            records.split_off(records.len() - max_items)
        } else {
            records
        }
    }

    pub fn clear_logs(&self) -> Result<(), String> {
        let _guard = self.write_lock.lock();
        if self.log_path.exists() {
            fs::write(&self.log_path, "").map_err(|e| format!("清除日誌失敗: {}", e))?;
        }
        Ok(())
    }

    /// 在 Windows 預設文字編輯器中開啟日誌檔
    pub fn open_in_viewer(&self) {
        #[cfg(target_os = "windows")]
        {
            use windows::core::PCWSTR;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            let path_str = self.log_path.to_string_lossy().to_string();
            let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
            let wide_verb: Vec<u16> = "open\0".encode_utf16().collect();
            unsafe {
                ShellExecuteW(
                    None,
                    PCWSTR(wide_verb.as_ptr()),
                    PCWSTR(wide_path.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
    }
}

/// 檢測目標目錄是否具備寫入權限
pub fn is_directory_writable(dir: &Path) -> bool {
    if !dir.exists() || !dir.is_dir() {
        return false;
    }
    let probe = dir.join(format!(".ramdisk_perm_probe_{}", std::process::id()));
    if fs::write(&probe, b"ok").is_ok() {
        let _ = fs::remove_file(&probe);
        true
    } else {
        false
    }
}

/// 取得應用程式主要基底目錄 (比照 RFM 設計：可攜免安裝模式優先使用 exe 目錄，唯讀安裝模式回退至 AppData)
pub fn resolve_app_directory() -> PathBuf {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            if is_directory_writable(exe_dir) {
                return exe_dir.to_path_buf();
            }
        }
    }
    let base = dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("RustRAMDisk")
}

pub fn resolve_log_directory() -> PathBuf {
    resolve_app_directory().join("logs")
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $event:expr, $msg:expr) => {
        $crate::logger::global_logger().log("INFO", $target, $event, $msg, None)
    };
    ($target:expr, $event:expr, $msg:expr, $ctx:expr) => {
        $crate::logger::global_logger().log("INFO", $target, $event, $msg, Some($ctx))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $event:expr, $msg:expr) => {
        $crate::logger::global_logger().log("WARN", $target, $event, $msg, None)
    };
    ($target:expr, $event:expr, $msg:expr, $ctx:expr) => {
        $crate::logger::global_logger().log("WARN", $target, $event, $msg, Some($ctx))
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $event:expr, $msg:expr) => {
        $crate::logger::global_logger().log("ERROR", $target, $event, $msg, None)
    };
    ($target:expr, $event:expr, $msg:expr, $ctx:expr) => {
        $crate::logger::global_logger().log("ERROR", $target, $event, $msg, Some($ctx))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $event:expr, $msg:expr) => {
        $crate::logger::global_logger().log("DEBUG", $target, $event, $msg, None)
    };
    ($target:expr, $event:expr, $msg:expr, $ctx:expr) => {
        $crate::logger::global_logger().log("DEBUG", $target, $event, $msg, Some($ctx))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logger_lifecycle() {
        let temp_dir = std::env::temp_dir().join(format!("test_ramdisk_logger_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let log_file = temp_dir.join("test.jsonl");

        let logger = AppLogger {
            format: Mutex::new(LogFormat::Json),
            level: Mutex::new(LogLevel::Debug),
            write_lock: Mutex::new(()),
            log_path: log_file.clone(),
        };

        logger.log("INFO", "test_mod", "unit_test_event", "Test message", Some(json!({"key": "val"})));
        assert!(log_file.exists());

        let logs = logger.read_recent_logs(10);
        assert!(!logs.is_empty());
        assert_eq!(logs.last().unwrap().event, "unit_test_event");
        assert_eq!(logs.last().unwrap().message, "Test message");

        logger.set_format(LogFormat::Text);
        logger.log("WARN", "test_mod", "text_event", "Text formatted message", None);

        let _ = fs::remove_dir_all(temp_dir);
    }
}
