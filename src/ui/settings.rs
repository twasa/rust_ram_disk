use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, GetSysColorBrush, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, FF_DONTCARE, FW_BOLD, FW_NORMAL, HFONT, OUT_DEFAULT_PRECIS, SYS_COLOR_INDEX,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_EXPLORER, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::service::{self, ServiceConfig, ServiceState};

pub const WM_APP_APPLY_SETTINGS: u32 = WM_APP + 201;
pub const WM_APP_SERVICE_STATE_CHANGED: u32 = WM_APP + 202;

// 控制項識別碼
const ID_DRIVE_COMBO: u16 = 101;
const ID_SIZE_COMBO: u16 = 102;
const ID_COMPRESS_COMBO: u16 = 103;
const ID_IMAGE_EDIT: u16 = 104;
const ID_BROWSE_BTN: u16 = 105;
const ID_AUTOSTART_CHECK: u16 = 106;
const ID_APPLY_BTN: u16 = 107;
const ID_CANCEL_BTN: u16 = 108;
const ID_STATUS_LABEL: u16 = 109;

const ID_SVC_INSTALL_START_BTN: u16 = 120;
const ID_SVC_START_BTN: u16 = 121;
const ID_SVC_STOP_BTN: u16 = 122;
const ID_SVC_UNINSTALL_BTN: u16 = 123;
const ID_SVC_STATUS_LABEL: u16 = 124;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    pub mount_point: String,
    pub size: String,
    pub compress: String,
    pub image_path: Option<String>,
    pub auto_start: bool,
}

impl From<ServiceConfig> for SettingsConfig {
    fn from(s: ServiceConfig) -> Self {
        Self {
            mount_point: s.mount_point,
            size: s.size,
            compress: s.compress,
            image_path: s.image_path,
            auto_start: s.auto_start,
        }
    }
}

impl From<SettingsConfig> for ServiceConfig {
    fn from(s: SettingsConfig) -> Self {
        Self {
            mount_point: s.mount_point,
            size: s.size,
            compress: s.compress,
            image_path: s.image_path,
            auto_start: s.auto_start,
        }
    }
}

#[derive(Clone, Copy)]
struct SendHwnd(HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

static PENDING_CONFIG: Mutex<Option<SettingsConfig>> = Mutex::new(None);
static CURRENT_SETTINGS_WINDOW: Mutex<Option<SendHwnd>> = Mutex::new(None);
static HOST_WINDOW: Mutex<Option<SendHwnd>> = Mutex::new(None);
static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// 取出待套用之新設定 (由主訊息迴圈調用)
pub fn take_pending_config() -> Option<SettingsConfig> {
    PENDING_CONFIG.lock().unwrap().take()
}

/// 自登錄檔與全域設定檔讀取配置
pub fn load_config_from_registry(
    default_mount: &str,
    default_size: &str,
    default_compress: &str,
    default_image: Option<&str>,
) -> SettingsConfig {
    // 優先從 ServiceConfig (ProgramData/RustRamDisk/config.json) 讀取
    let svc_cfg = ServiceConfig::load();
    if ServiceConfig::config_path().exists() {
        return svc_cfg.into();
    }

    let mut config = SettingsConfig {
        mount_point: default_mount.to_string(),
        size: default_size.to_string(),
        compress: default_compress.to_string(),
        image_path: default_image.map(|s| s.to_string()),
        auto_start: false,
    };

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\RustRAMDisk"),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_ok()
        {
            let read_string = |val_name: PCWSTR| -> Option<String> {
                let mut buf = [0u16; 512];
                let mut len = (buf.len() * 2) as u32;
                if RegQueryValueExW(
                    hkey,
                    val_name,
                    None,
                    None,
                    Some(buf.as_mut_ptr() as *mut u8),
                    Some(&mut len),
                )
                .is_ok()
                {
                    let char_len = (len / 2).saturating_sub(1) as usize;
                    Some(String::from_utf16_lossy(&buf[..char_len]))
                } else {
                    None
                }
            };

            if let Some(m) = read_string(w!("DriveLetter")) {
                if !m.is_empty() {
                    config.mount_point = m;
                }
            }
            if let Some(s) = read_string(w!("Size")) {
                if !s.is_empty() {
                    config.size = s;
                }
            }
            if let Some(c) = read_string(w!("Compress")) {
                if !c.is_empty() {
                    config.compress = c;
                }
            }
            if let Some(img) = read_string(w!("ImagePath")) {
                if !img.is_empty() {
                    config.image_path = Some(img);
                }
            }

            let mut auto_dw = 0u32;
            let mut len = 4u32;
            if RegQueryValueExW(
                hkey,
                w!("AutoStart"),
                None,
                None,
                Some(&mut auto_dw as *mut u32 as *mut u8),
                Some(&mut len),
            )
            .is_ok()
            {
                config.auto_start = auto_dw != 0;
            }

            let _ = RegCloseKey(hkey);
        }
    }

    config
}

/// 儲存設定至全域設定檔與登錄檔
pub fn save_config_to_registry(config: &SettingsConfig) {
    // 1. 同步儲存至 ServiceConfig (C:\ProgramData\RustRamDisk\config.json)
    let svc_cfg: ServiceConfig = config.clone().into();
    let _ = svc_cfg.save();

    unsafe {
        let mut hkey = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\RustRAMDisk"),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
        .is_ok()
        {
            let write_string = |val_name: PCWSTR, s: &str| {
                let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = RegSetValueExW(
                    hkey,
                    val_name,
                    0,
                    REG_SZ,
                    Some(std::slice::from_raw_parts(
                        wide.as_ptr() as *const u8,
                        wide.len() * 2,
                    )),
                );
            };

            write_string(w!("DriveLetter"), &config.mount_point);
            write_string(w!("Size"), &config.size);
            write_string(w!("Compress"), &config.compress);
            if let Some(ref img) = config.image_path {
                write_string(w!("ImagePath"), img);
            } else {
                write_string(w!("ImagePath"), "");
            }

            let auto_dw: u32 = if config.auto_start { 1 } else { 0 };
            let _ = RegSetValueExW(
                hkey,
                w!("AutoStart"),
                0,
                REG_DWORD,
                Some(std::slice::from_raw_parts(
                    &auto_dw as *const u32 as *const u8,
                    4,
                )),
            );

            let _ = RegCloseKey(hkey);
        }

        // 開機自動啟動註冊 (登錄檔 Run：讓 Systray 在使用者登入時常駐顯示)
        let mut run_key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_WRITE,
            &mut run_key,
        )
        .is_ok()
        {
            if config.auto_start {
                if let Ok(exe) = std::env::current_exe() {
                    let cmd = format!("\"{}\" --tray", exe.display());
                    let wide_cmd: Vec<u16> =
                        cmd.encode_utf16().chain(std::iter::once(0)).collect();
                    let _ = RegSetValueExW(
                        run_key,
                        w!("RustRAMDisk"),
                        0,
                        REG_SZ,
                        Some(std::slice::from_raw_parts(
                            wide_cmd.as_ptr() as *const u8,
                            wide_cmd.len() * 2,
                        )),
                    );
                }
            } else {
                let _ = RegDeleteValueW(run_key, w!("RustRAMDisk"));
            }
            let _ = RegCloseKey(run_key);
        }
    }
}

/// 註冊設定視窗類別
fn ensure_class_registered() {
    if CLASS_REGISTERED.load(Ordering::SeqCst) {
        return;
    }

    unsafe {
        let class_name = w!("RustRAMDisk_SettingsWindowClass");
        let brush = GetSysColorBrush(SYS_COLOR_INDEX(15)); // COLOR_BTNFACE = 15
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(settings_wndproc),
            hInstance: HINSTANCE(std::ptr::null_mut()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: brush,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        CLASS_REGISTERED.store(true, Ordering::SeqCst);
    }
}

/// 開啟或帶起設定畫面
pub fn show_settings_window(host_hwnd: HWND, current_config: &SettingsConfig) {
    ensure_class_registered();

    {
        let mut host_guard = HOST_WINDOW.lock().unwrap();
        *host_guard = Some(SendHwnd(host_hwnd));
    }

    let mut win_guard = CURRENT_SETTINGS_WINDOW.lock().unwrap();
    if let Some(send_h) = *win_guard {
        if unsafe { IsWindow(send_h.0) }.as_bool() {
            unsafe {
                let _ = ShowWindow(send_h.0, SW_SHOW);
                let _ = SetForegroundWindow(send_h.0);
                let _ = BringWindowToTop(send_h.0);
            }
            return;
        }
    }

    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let width = 530;
        let height = 620;
        let x = (screen_w - width) / 2;
        let y = (screen_h - height) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("RustRAMDisk_SettingsWindowClass"),
            w!("Rust RAM Disk - 虛擬磁碟與系統服務設定"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            width,
            height,
            HWND(std::ptr::null_mut()),
            None,
            HINSTANCE(std::ptr::null_mut()),
            None,
        )
        .ok();

        if let Some(h) = hwnd {
            *win_guard = Some(SendHwnd(h));
            init_controls(h, current_config);
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }
}

unsafe fn create_font(size: i32, bold: bool) -> HFONT {
    let font_name: Vec<u16> = "Microsoft JhengHei UI\0".encode_utf16().collect();
    CreateFontW(
        size,
        0,
        0,
        0,
        if bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(font_name.as_ptr()),
    )
}

fn get_service_status_text() -> String {
    match service::query_status() {
        Ok(ServiceState::Running) => {
            "🟢 系統服務狀態：運行中 (Running) — 磁碟由服務全時維護，關閉 UI 磁碟不受影響".to_string()
        }
        Ok(ServiceState::Stopped) => {
            "🟡 系統服務狀態：已停止 (Stopped) — 點擊下方按鈕啟動服務以掛載磁碟".to_string()
        }
        Ok(ServiceState::NotInstalled) => {
            "⚪ 系統服務狀態：尚未安裝 (Not Installed) — 點擊「註冊並啟動」可常駐於系統".to_string()
        }
        Ok(other) => format!("🔵 系統服務狀態：{}", other),
        Err(e) => format!("⚠️ 查詢服務失敗: {}", e),
    }
}

unsafe fn update_service_labels(hwnd: HWND) {
    if let Ok(st) = GetDlgItem(hwnd, ID_SVC_STATUS_LABEL as _) {
        let text = get_service_status_text();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = SetWindowTextW(st, PCWSTR(wide.as_ptr()));
    }
}

unsafe fn init_controls(hwnd: HWND, config: &SettingsConfig) {
    let font = create_font(16, false);
    let bold_font = create_font(16, true);

    let set_ctrl_font = |hctrl: HWND, is_bold: bool| {
        let f = if is_bold { bold_font } else { font };
        SendMessageW(hctrl, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
    };

    // 1. GroupBox: 目前磁碟與服務狀態
    let gb1 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("目前磁碟狀態"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        15,
        10,
        485,
        75,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(gb1, true);

    let status_text = format!(
        "掛載代號：{}　｜　目前容量：{}　｜　壓縮：{}\n運行架構：Windows System Service (隨系統啟動，關閉 UI 磁碟不受影響)",
        config.mount_point, config.size, config.compress
    );
    let wide_status: Vec<u16> = status_text.encode_utf16().chain(std::iter::once(0)).collect();
    let st1 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(wide_status.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        30,
        32,
        455,
        42,
        hwnd,
        HMENU(ID_STATUS_LABEL as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(st1, false);

    // 2. GroupBox: 虛擬磁碟參數配置
    let gb2 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("虛擬磁碟參數配置"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        15,
        95,
        485,
        250,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(gb2, true);

    // 磁碟機代號
    let lbl_drv = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("磁碟代號 (Drive):"),
        WS_CHILD | WS_VISIBLE,
        30,
        125,
        135,
        22,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_drv, false);

    let cb_drv = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
        175,
        122,
        295,
        200,
        hwnd,
        HMENU(ID_DRIVE_COMBO as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(cb_drv, false);

    let letters = ["R:", "S:", "T:", "U:", "V:", "W:", "X:", "Y:", "Z:", "A:", "B:"];
    let mut selected_drv_idx = 0;
    for (i, &l) in letters.iter().enumerate() {
        let wide: Vec<u16> = l.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(cb_drv, CB_ADDSTRING, WPARAM(0), LPARAM(wide.as_ptr() as _));
        if l.eq_ignore_ascii_case(&config.mount_point) {
            selected_drv_idx = i;
        }
    }
    SendMessageW(cb_drv, CB_SETCURSEL, WPARAM(selected_drv_idx), LPARAM(0));

    // 磁碟容量
    let lbl_sz = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("記憶體容量 (Size):"),
        WS_CHILD | WS_VISIBLE,
        30,
        160,
        135,
        22,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_sz, false);

    let cb_sz = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWN as u32),
        175,
        157,
        295,
        200,
        hwnd,
        HMENU(ID_SIZE_COMBO as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(cb_sz, false);

    let sizes = ["512M", "1G", "2G", "4G", "8G", "10G", "16G", "32G", "64G"];
    let mut selected_sz_idx = 2; // default 2G
    for (i, &s) in sizes.iter().enumerate() {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        SendMessageW(cb_sz, CB_ADDSTRING, WPARAM(0), LPARAM(wide.as_ptr() as _));
        if s.eq_ignore_ascii_case(&config.size) {
            selected_sz_idx = i;
        }
    }
    SendMessageW(cb_sz, CB_SETCURSEL, WPARAM(selected_sz_idx), LPARAM(0));

    // 壓縮模式
    let lbl_cp = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("壓縮模式 (Compress):"),
        WS_CHILD | WS_VISIBLE,
        30,
        195,
        135,
        22,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_cp, false);

    let cb_cp = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
        175,
        192,
        295,
        120,
        hwnd,
        HMENU(ID_COMPRESS_COMBO as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(cb_cp, false);

    let wide_lz4: Vec<u16> = "LZ4 (即時透明壓縮 - 推薦)\0".encode_utf16().collect();
    let wide_none: Vec<u16> = "None (無壓縮 - 原始未壓縮記憶體)\0".encode_utf16().collect();
    SendMessageW(cb_cp, CB_ADDSTRING, WPARAM(0), LPARAM(wide_lz4.as_ptr() as _));
    SendMessageW(cb_cp, CB_ADDSTRING, WPARAM(0), LPARAM(wide_none.as_ptr() as _));
    let comp_idx = if config.compress.eq_ignore_ascii_case("none") { 1 } else { 0 };
    SendMessageW(cb_cp, CB_SETCURSEL, WPARAM(comp_idx), LPARAM(0));

    // 快照映像檔路徑
    let lbl_img = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("快照映像檔路徑:"),
        WS_CHILD | WS_VISIBLE,
        30,
        230,
        135,
        22,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_img, false);

    let default_img = config.image_path.clone().unwrap_or_default();
    let wide_def_img: Vec<u16> = default_img.encode_utf16().chain(std::iter::once(0)).collect();
    let ed_img = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("EDIT"),
        PCWSTR(wide_def_img.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        175,
        227,
        215,
        24,
        hwnd,
        HMENU(ID_IMAGE_EDIT as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(ed_img, false);

    let btn_browse = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("瀏覽..."),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        400,
        226,
        70,
        26,
        hwnd,
        HMENU(ID_BROWSE_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_browse, false);

    // 開機自動啟動 Checkbox
    let chk_boot = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("隨 Windows 開機自動背景啟動 (服務設為 AutoStart + Systray 常駐)"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        30,
        265,
        440,
        24,
        hwnd,
        HMENU(ID_AUTOSTART_CHECK as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(chk_boot, false);
    if config.auto_start {
        SendMessageW(chk_boot, BM_SETCHECK, WPARAM(1), LPARAM(0));
    }

    // 提示說明
    let lbl_note = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("💡 提示：磁碟由系統服務全權代管，即使關閉本設定視窗或退出 Systray，磁碟機依然正常運作。"),
        WS_CHILD | WS_VISIBLE,
        30,
        300,
        440,
        35,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_note, false);

    // 3. GroupBox: Windows 系統服務管理 (Service Management)
    let gb3 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("Windows 系統服務管理"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        15,
        355,
        485,
        150,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(gb3, true);

    let svc_text = get_service_status_text();
    let wide_svc_text: Vec<u16> = svc_text.encode_utf16().chain(std::iter::once(0)).collect();
    let lbl_svc = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(wide_svc_text.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        30,
        380,
        455,
        30,
        hwnd,
        HMENU(ID_SVC_STATUS_LABEL as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_svc, false);

    // 服務操作按鈕列
    let btn_svc_install = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("🚀 註冊並啟動服務"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        30,
        420,
        140,
        32,
        hwnd,
        HMENU(ID_SVC_INSTALL_START_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_svc_install, true);

    let btn_svc_start = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("▶️ 啟動服務"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        180,
        420,
        95,
        32,
        hwnd,
        HMENU(ID_SVC_START_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_svc_start, false);

    let btn_svc_stop = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("⏹️ 停止服務"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        285,
        420,
        95,
        32,
        hwnd,
        HMENU(ID_SVC_STOP_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_svc_stop, false);

    let btn_svc_uninst = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("🗑️ 卸載服務"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        390,
        420,
        95,
        32,
        hwnd,
        HMENU(ID_SVC_UNINSTALL_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_svc_uninst, false);

    let lbl_svc_hint = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("註冊為服務後，RAM Disk 將作為本機系統實體磁碟常駐，無需手動掛載。"),
        WS_CHILD | WS_VISIBLE,
        30,
        462,
        455,
        22,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_svc_hint, false);

    // 4. 底部按鈕區
    let btn_apply = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("套用並儲存設定"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        260,
        525,
        140,
        36,
        hwnd,
        HMENU(ID_APPLY_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_apply, true);

    let btn_cancel = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("關閉"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        410,
        525,
        90,
        36,
        hwnd,
        HMENU(ID_CANCEL_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_cancel, false);
}

unsafe fn read_current_dialog_config(hwnd: HWND) -> SettingsConfig {
    let cb_drv = GetDlgItem(hwnd, ID_DRIVE_COMBO as _).unwrap_or_default();
    let mut drv_buf = [0u16; 32];
    let cur_drv = SendMessageW(cb_drv, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize;
    SendMessageW(
        cb_drv,
        CB_GETLBTEXT,
        WPARAM(cur_drv),
        LPARAM(drv_buf.as_mut_ptr() as _),
    );
    let drv_len = drv_buf.iter().position(|&c| c == 0).unwrap_or(drv_buf.len());
    let mut drive_str = String::from_utf16_lossy(&drv_buf[..drv_len]).trim().to_string();
    if drive_str.is_empty() {
        drive_str = "R:".to_string();
    }

    let cb_sz = GetDlgItem(hwnd, ID_SIZE_COMBO as _).unwrap_or_default();
    let mut sz_buf = [0u16; 64];
    GetWindowTextW(cb_sz, &mut sz_buf);
    let sz_len = sz_buf.iter().position(|&c| c == 0).unwrap_or(sz_buf.len());
    let raw_sz = String::from_utf16_lossy(&sz_buf[..sz_len]).trim().to_string();
    let size_str = raw_sz.split_whitespace().next().unwrap_or("2G").to_string();

    let cb_cp = GetDlgItem(hwnd, ID_COMPRESS_COMBO as _).unwrap_or_default();
    let cur_cp = SendMessageW(cb_cp, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let compress_str = if cur_cp == 1 { "none" } else { "lz4" }.to_string();

    let ed_img = GetDlgItem(hwnd, ID_IMAGE_EDIT as _).unwrap_or_default();
    let mut img_buf = [0u16; 1024];
    GetWindowTextW(ed_img, &mut img_buf);
    let img_len = img_buf.iter().position(|&c| c == 0).unwrap_or(img_buf.len());
    let raw_img = String::from_utf16_lossy(&img_buf[..img_len]).trim().to_string();
    let image_opt = if raw_img.is_empty() { None } else { Some(raw_img) };

    let chk = GetDlgItem(hwnd, ID_AUTOSTART_CHECK as _).unwrap_or_default();
    let is_checked = SendMessageW(chk, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;

    SettingsConfig {
        mount_point: drive_str,
        size: size_str,
        compress: compress_str,
        image_path: image_opt,
        auto_start: is_checked,
    }
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let ctrl_id = (wparam.0 & 0xffff) as u16;
            match ctrl_id {
                ID_BROWSE_BTN => {
                    let mut file_buf = [0u16; 1024];
                    let filter: Vec<u16> =
                        "RAM Disk 映像檔 (*.img)\0*.img\0所有檔案 (*.*)\0*.*\0\0"
                            .encode_utf16()
                            .collect();
                    let title: Vec<u16> = "選擇 RAM Disk 快照映像檔儲存路徑\0".encode_utf16().collect();
                    let def_ext: Vec<u16> = "img\0".encode_utf16().collect();

                    let mut ofn = OPENFILENAMEW {
                        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
                        hwndOwner: hwnd,
                        lpstrFilter: PCWSTR(filter.as_ptr()),
                        lpstrFile: PWSTR(file_buf.as_mut_ptr()),
                        nMaxFile: file_buf.len() as u32,
                        lpstrTitle: PCWSTR(title.as_ptr()),
                        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST,
                        lpstrDefExt: PCWSTR(def_ext.as_ptr()),
                        ..Default::default()
                    };

                    if GetSaveFileNameW(&mut ofn).as_bool() {
                        let len = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
                        let path = String::from_utf16_lossy(&file_buf[..len]);
                        let wide_path: Vec<u16> =
                            path.encode_utf16().chain(std::iter::once(0)).collect();
                        if let Ok(ed) = GetDlgItem(hwnd, ID_IMAGE_EDIT as _) {
                            let _ = SetWindowTextW(ed, PCWSTR(wide_path.as_ptr()));
                        }
                    }
                }
                ID_SVC_INSTALL_START_BTN => {
                    let cfg = read_current_dialog_config(hwnd);
                    save_config_to_registry(&cfg);
                    let svc_cfg: ServiceConfig = cfg.into();
                    match service::install(&svc_cfg, true) {
                        Ok(_) => {
                            MessageBoxW(
                                hwnd,
                                w!("Rust RAM Disk 系統服務已成功註冊並啟動！\n\n虛擬磁碟已常駐於系統，關閉此介面不影響磁碟運作。"),
                                w!("服務已就緒"),
                                MB_ICONINFORMATION | MB_OK,
                            );
                            if let Some(host) = *HOST_WINDOW.lock().unwrap() {
                                let _ = PostMessageW(host.0, WM_APP_SERVICE_STATE_CHANGED, WPARAM(0), LPARAM(0));
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("註冊或啟動系統服務失敗: {}\n\n提示：請確認是否具備系統管理員權限。", e);
                            let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(wide_err.as_ptr()), w!("操作失敗"), MB_ICONERROR | MB_OK);
                        }
                    }
                    update_service_labels(hwnd);
                }
                ID_SVC_START_BTN => {
                    match service::start() {
                        Ok(_) => {
                            MessageBoxW(hwnd, w!("Rust RAM Disk 系統服務已成功啟動！"), w!("服務啟動"), MB_ICONINFORMATION | MB_OK);
                            if let Some(host) = *HOST_WINDOW.lock().unwrap() {
                                let _ = PostMessageW(host.0, WM_APP_SERVICE_STATE_CHANGED, WPARAM(0), LPARAM(0));
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("啟動服務失敗: {}", e);
                            let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(wide_err.as_ptr()), w!("啟動失敗"), MB_ICONERROR | MB_OK);
                        }
                    }
                    update_service_labels(hwnd);
                }
                ID_SVC_STOP_BTN => {
                    match service::stop() {
                        Ok(_) => {
                            MessageBoxW(hwnd, w!("Rust RAM Disk 系統服務已成功停止。"), w!("服務停止"), MB_ICONINFORMATION | MB_OK);
                            if let Some(host) = *HOST_WINDOW.lock().unwrap() {
                                let _ = PostMessageW(host.0, WM_APP_SERVICE_STATE_CHANGED, WPARAM(0), LPARAM(0));
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("停止服務失敗: {}", e);
                            let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(wide_err.as_ptr()), w!("停止失敗"), MB_ICONERROR | MB_OK);
                        }
                    }
                    update_service_labels(hwnd);
                }
                ID_SVC_UNINSTALL_BTN => {
                    match service::uninstall() {
                        Ok(_) => {
                            MessageBoxW(hwnd, w!("Rust RAM Disk 系統服務已成功卸載移除。"), w!("服務已卸載"), MB_ICONINFORMATION | MB_OK);
                            if let Some(host) = *HOST_WINDOW.lock().unwrap() {
                                let _ = PostMessageW(host.0, WM_APP_SERVICE_STATE_CHANGED, WPARAM(0), LPARAM(0));
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("卸載服務失敗: {}", e);
                            let wide_err: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            MessageBoxW(hwnd, PCWSTR(wide_err.as_ptr()), w!("卸載失敗"), MB_ICONERROR | MB_OK);
                        }
                    }
                    update_service_labels(hwnd);
                }
                ID_APPLY_BTN => {
                    let new_config = read_current_dialog_config(hwnd);
                    save_config_to_registry(&new_config);

                    // 存入待套用 slot
                    {
                        let mut pending = PENDING_CONFIG.lock().unwrap();
                        *pending = Some(new_config);
                    }

                    // 通知 Host 視窗進行重新掛載或服務重啟
                    if let Some(host) = *HOST_WINDOW.lock().unwrap() {
                        let _ = PostMessageW(host.0, WM_APP_APPLY_SETTINGS, WPARAM(0), LPARAM(0));
                    }

                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                ID_CANCEL_BTN => {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
