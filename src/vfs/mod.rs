use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tracing::info;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("WinFsp 核心驅動尚未安裝。請先執行 winfsp-installer.msi 註冊驅動。")]
    DriverNotInstalled,

    #[error("找不到 WinFsp 核心安裝套件 (winfsp-installer.msi)")]
    InstallerNotFound,

    #[error("使用者取消了管理員提權授權 (UAC)")]
    DriverInstallCancelled,

    #[error("WinFsp 核心驅動安裝失敗 (結束代碼: {0})")]
    DriverInstallFailed(u32),

    #[error("找不到 WinFsp 記憶體檔案系統可執行檔 (memfs-x64.exe)")]
    MemFsNotFound,

    #[error("無法啟動 WinFsp 掛載程序: {0}")]
    ProcessLaunch(#[from] std::io::Error),

    #[error("磁碟機 {0} 掛載逾時，檔案總管未能及時辨識該磁碟代號")]
    MountTimeout(String),

    #[error("磁碟機代號格式無效: '{0}'，請使用例如 'S:' 或 'R:'")]
    InvalidMountPoint(String),

    #[error("磁碟機代號 {0} 目前已被其他裝置佔用")]
    MountPointInUse(String),
}

/// 檢查系統是否已安裝並註冊 WinFsp 核心驅動 (winfsp.sys)
pub fn is_driver_installed() -> bool {
    // 1. 檢查 Windows 登錄檔中是否存在 WinFsp 安裝註冊
    let reg_installed = {
        let hklm = windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
        unsafe {
            use windows::core::w;
            use windows::Win32::System::Registry::{RegOpenKeyExW, HKEY, KEY_READ};
            let mut sub_key = HKEY::default();
            let res1 = RegOpenKeyExW(
                hklm,
                w!("SOFTWARE\\WinFsp"),
                0,
                KEY_READ,
                &mut sub_key,
            );
            let res2 = RegOpenKeyExW(
                hklm,
                w!("SOFTWARE\\WOW6432Node\\WinFsp"),
                0,
                KEY_READ,
                &mut sub_key,
            );
            let res3 = RegOpenKeyExW(
                hklm,
                w!("SYSTEM\\CurrentControlSet\\Services\\winfsp"),
                0,
                KEY_READ,
                &mut sub_key,
            );
            res1.is_ok() || res2.is_ok() || res3.is_ok()
        }
    };

    if reg_installed {
        return true;
    }

    // 2. 備用檢查：檢查標準安裝路徑下的 winfsp.sys
    let sys_path = Path::new("C:\\Windows\\System32\\drivers\\winfsp-x64.sys");
    if sys_path.exists() {
        return true;
    }

    false
}

/// 檢查當前執行程序是否具備管理員權限 (Elevated Token)
#[cfg(target_os = "windows")]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() {
            let mut elevation = TOKEN_ELEVATION::default();
            let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
            let ret = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                size,
                &mut size,
            );
            let _ = CloseHandle(token);
            if ret.is_ok() {
                return elevation.TokenIsElevated != 0;
            }
        }
        false
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_elevated() -> bool {
    true
}

/// 自動以 Windows 管理員權限重啟當前程序 (UAC 提權)
#[cfg(target_os = "windows")]
pub fn relaunch_elevated() -> Result<(), VfsError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT;

    let exe = std::env::current_exe().map_err(VfsError::ProcessLaunch)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args_str = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{}\"", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let wide_exe: Vec<u16> = exe
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let wide_args: Vec<u16> = args_str.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_verb: Vec<u16> = "runas\0".encode_utf16().collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: Default::default(),
        lpVerb: PCWSTR(wide_verb.as_ptr()),
        lpFile: PCWSTR(wide_exe.as_ptr()),
        lpParameters: PCWSTR(wide_args.as_ptr()),
        lpDirectory: PCWSTR::null(),
        nShow: SW_SHOWDEFAULT.0 as i32,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut sei).map_err(|e| {
            VfsError::ProcessLaunch(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
    }

    std::process::exit(0);
}

#[cfg(not(target_os = "windows"))]
pub fn relaunch_elevated() -> Result<(), VfsError> {
    Ok(())
}

/// 尋找 WinFsp 核心安裝套件 (winfsp-installer.msi)
pub fn find_winfsp_installer() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    // 1. 檢查目前執行檔同層目錄
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("winfsp-installer.msi"));
            candidates.push(exe_dir.join("tools").join("winfsp-installer.msi"));
        }
    }

    // 2. 檢查專案原始碼目錄相對路徑
    candidates.push(PathBuf::from("tools/winfsp-installer.msi"));
    candidates.push(PathBuf::from("winfsp-installer.msi"));

    for p in candidates {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 自動以管理員權限靜默執行 winfsp-installer.msi 進行驅動註冊
pub fn install_winfsp_driver_silent() -> Result<(), VfsError> {
    #[cfg(target_os = "windows")]
    {
        use std::time::{Duration, Instant};
        use windows::core::{HRESULT, PCWSTR};
        use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED};
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT;

        let installer_path = find_winfsp_installer().ok_or(VfsError::InstallerNotFound)?;
        let canonical = installer_path.canonicalize().unwrap_or(installer_path);
        let path_str = canonical.to_string_lossy();
        let clean_path = path_str.trim_start_matches(r"\\?\");

        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let file: Vec<u16> = "msiexec.exe\0".encode_utf16().collect();
        let params: Vec<u16> = format!("/i \"{}\" /qb /norestart\0", clean_path)
            .encode_utf16()
            .collect();

        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: Default::default(),
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(params.as_ptr()),
            lpDirectory: PCWSTR::null(),
            nShow: SW_SHOWDEFAULT.0 as i32,
            ..Default::default()
        };

        if let Err(err) = unsafe { ShellExecuteExW(&mut sei) } {
            if err.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Err(VfsError::DriverInstallCancelled);
            }
            return Err(VfsError::ProcessLaunch(std::io::Error::new(
                std::io::ErrorKind::Other,
                err.to_string(),
            )));
        }

        if !sei.hProcess.is_invalid() {
            unsafe {
                let _ = WaitForSingleObject(sei.hProcess, 60_000);
                let mut exit_code = 0u32;
                let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
                let _ = CloseHandle(sei.hProcess);

                if exit_code != 0 && exit_code != 3010 {
                    return Err(VfsError::DriverInstallFailed(exit_code));
                }
            }
        }

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if is_driver_installed() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }

        if is_driver_installed() {
            Ok(())
        } else {
            Err(VfsError::DriverNotInstalled)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(VfsError::DriverNotInstalled)
    }
}

/// 尋找 WinFsp memfs-x64.exe 二進位檔
pub fn find_memfs_binary() -> Result<PathBuf, VfsError> {
    let mut candidates = Vec::new();

    // 1. 執行檔同層目錄
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("memfs-x64.exe"));
            candidates.push(exe_dir.join("tools").join("winfsp_bin").join("memfs-x64.exe"));
            candidates.push(exe_dir.join("winfsp").join("memfs-x64.exe"));
        }
    }

    // 2. 專案內部工具目錄 (開發與建置優先使用本專案客製優化之 MemFS)
    candidates.push(PathBuf::from("tools\\winfsp_bin\\memfs-x64.exe"));
    candidates.push(PathBuf::from("tools/winfsp_bin/memfs-x64.exe"));

    // 3. WinFsp 系統標準安裝目錄
    candidates.push(PathBuf::from("C:\\Program Files (x86)\\WinFsp\\bin\\memfs-x64.exe"));
    candidates.push(PathBuf::from("C:\\Program Files\\WinFsp\\bin\\memfs-x64.exe"));

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(VfsError::MemFsNotFound)
}

/// 格式化磁碟代號 (例如 "s", "S", "S:", "s:\" -> "S:")
pub fn normalize_mount_point(input: &str) -> Result<String, VfsError> {
    let trimmed = input.trim().trim_end_matches('\\').trim_end_matches('/');
    let letter = if trimmed.len() == 1 && trimmed.chars().next().unwrap().is_ascii_alphabetic() {
        trimmed.to_ascii_uppercase()
    } else if trimmed.len() == 2
        && trimmed.ends_with(':')
        && trimmed.chars().next().unwrap().is_ascii_alphabetic()
    {
        trimmed[..1].to_ascii_uppercase()
    } else {
        return Err(VfsError::InvalidMountPoint(input.to_string()));
    };

    Ok(format!("{}:", letter))
}

/// 已掛載之虛擬磁碟控管控制代碼
pub struct VfsMountHandle {
    mount_point: String,
    process: Option<Child>,
    unmounted: bool,
}

impl VfsMountHandle {
    pub fn mount_point(&self) -> &str {
        &self.mount_point
    }

    /// 檢查掛載服務是否正常運作中
    pub fn is_alive(&mut self) -> bool {
        if self.unmounted {
            return false;
        }
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(None) => true, // 仍正常執行中
                _ => false,       // 已退出
            }
        } else {
            false
        }
    }

    /// 安全卸載虛擬磁碟機
    pub fn unmount(&mut self) -> Result<(), VfsError> {
        if self.unmounted {
            return Ok(());
        }

        info!("Unmounting RAM Disk from: {}", self.mount_point);
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // 短暫等待 Windows 釋放磁碟機代號
        let check_root = format!("{}\\", self.mount_point);
        for _ in 0..20 {
            if !Path::new(&check_root).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        self.unmounted = true;
        info!("Drive {} successfully unmounted", self.mount_point);
        Ok(())
    }
}

impl Drop for VfsMountHandle {
    fn drop(&mut self) {
        let _ = self.unmount();
    }
}

/// 啟動 WinFsp 記憶體虛擬磁碟掛載程序
pub fn mount_ram_disk(mount_point_raw: &str, capacity_bytes: u64) -> Result<VfsMountHandle, VfsError> {
    let mount_point = normalize_mount_point(mount_point_raw)?;
    let check_path = format!("{}\\", mount_point);

    if Path::new(&check_path).exists() {
        return Err(VfsError::MountPointInUse(mount_point));
    }

    if !is_driver_installed() {
        return Err(VfsError::DriverNotInstalled);
    }

    let memfs_bin = find_memfs_binary()?;
    info!("Using WinFsp MemFS binary: {:?}", memfs_bin);

    // 啟動 memfs:
    // -i: 忽略檔名大小寫 (Windows 應用程式相容必要)
    // -F NTFS: 模擬為 NTFS 檔案系統格式 (Windows 執行檔、提權執行與總管辨識必備)
    // -C <capacity_bytes>: 精確設定虛擬磁碟總容量，檔案總管顯示精確容量 (例如 2.00 GB)
    // -s <capacity_bytes>: 支援的單檔最大容量 (允許單檔達磁碟上限)
    // -f: 關閉檔案控制代碼時立即 flush and purge 快取，確保跨程序讀取與預覽即時一致
    // -S: 指定 Root SDDL，包含 OICI 繼承與 FA 完全控制權，確保免安裝軟體可執行、檔案預覽與所有使用者讀寫權限
    // -m \\.\<mount_point>: 透過 Windows 掛載管理器 (Mount Manager, \Device\MountPointManager) 註冊為本機實體磁碟 (DRIVE_FIXED)，
    //     不僅檔案總管呈現標準本機硬碟圖示（非網路磁碟），更讓 GetFinalPathNameByHandleW 正確反查 Volume，
    //     免安裝軟體 (.exe)、PE Loader、WebView2 及檔案預覽皆能完美運作。
    let mount_target = format!("\\\\.\\{}", mount_point);
    let sddl = "O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;WD)";
    let mut cmd = Command::new(&memfs_bin);
    cmd.arg("-i")
        .arg("-F")
        .arg("NTFS")
        .arg("-C")
        .arg(capacity_bytes.to_string())
        .arg("-s")
        .arg(capacity_bytes.to_string())
        .arg("-f")
        .arg("-S")
        .arg(sddl)
        .arg("-m")
        .arg(&mount_target);

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let child = cmd.spawn()?;
    let mut handle = VfsMountHandle {
        mount_point: mount_point.clone(),
        process: Some(child),
        unmounted: false,
    };

    // 等待 Windows Explorer 辨識該磁碟機代號 (最多等待 5 秒)
    let start_wait = Instant::now();
    let mut mounted = false;

    while start_wait.elapsed() < Duration::from_secs(5) {
        if Path::new(&check_path).exists() {
            mounted = true;
            break;
        }

        // 檢查 child 是否提早崩潰退出
        if !handle.is_alive() {
            return Err(VfsError::ProcessLaunch(std::io::Error::new(
                std::io::ErrorKind::Other,
                "WinFsp memfs 行程無預警退出，可能核心驅動尚未完全啟動",
            )));
        }

        std::thread::sleep(Duration::from_millis(150));
    }

    if !mounted {
        handle.unmount().ok();
        return Err(VfsError::MountTimeout(mount_point));
    }

    info!("Successfully mounted RAM Disk to {}", mount_point);
    Ok(handle)
}
