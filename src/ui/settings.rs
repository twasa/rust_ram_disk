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

pub const WM_APP_APPLY_SETTINGS: u32 = WM_APP + 201;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    pub mount_point: String,
    pub size: String,
    pub compress: String,
    pub image_path: Option<String>,
    pub auto_start: bool,
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

/// 自登錄檔讀取設定 (若無則採用預設)
pub fn load_config_from_registry(
    default_mount: &str,
    default_size: &str,
    default_compress: &str,
    default_image: Option<&str>,
) -> SettingsConfig {
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

/// 儲存設定至登錄檔並更新開機自動啟動項
pub fn save_config_to_registry(config: &SettingsConfig) {
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

        // 開機自動啟動註冊
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
                    let mut cmd = format!(
                        "\"{}\" mount -s {} -m {} -c {} --tray",
                        exe.display(),
                        config.size,
                        config.mount_point,
                        config.compress
                    );
                    if let Some(ref img) = config.image_path {
                        if !img.is_empty() {
                            cmd.push_str(&format!(" -i \"{}\"", img));
                        }
                    }
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
        let width = 520;
        let height = 480;
        let x = (screen_w - width) / 2;
        let y = (screen_h - height) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("RustRAMDisk_SettingsWindowClass"),
            w!("Rust RAM Disk - 虛擬磁碟設定"),
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

unsafe fn init_controls(hwnd: HWND, config: &SettingsConfig) {
    let font = create_font(16, false);
    let bold_font = create_font(16, true);

    let set_ctrl_font = |hctrl: HWND, is_bold: bool| {
        let f = if is_bold { bold_font } else { font };
        SendMessageW(hctrl, WM_SETFONT, WPARAM(f.0 as usize), LPARAM(1));
    };

    // 1. GroupBox: 目前磁碟狀態
    let gb1 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("目前磁碟狀態"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        15,
        10,
        475,
        75,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(gb1, true);

    let status_text = format!(
        "掛載代號：{}　｜　目前容量：{}　｜　壓縮：{}\n運行狀態：● 正常運作中 (已掛載至檔案總管)",
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
        440,
        42,
        hwnd,
        HMENU(ID_STATUS_LABEL as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(st1, false);

    // 2. GroupBox: 重新配置參數
    let gb2 = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("虛擬磁碟重新配置"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        15,
        95,
        475,
        280,
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
        130,
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
        170,
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
        165,
        130,
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
        170,
        162,
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
        205,
        130,
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
        170,
        202,
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
        245,
        130,
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
        170,
        242,
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
        395,
        241,
        70,
        26,
        hwnd,
        HMENU(ID_BROWSE_BTN as _),
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(btn_browse, false);

    // 開機自動掛載啟動 Checkbox
    let chk_boot = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("隨 Windows 開機自動背景掛載啟動 (登錄檔 Run)"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        30,
        285,
        430,
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
        w!("💡 提示：點擊「套用並重新掛載」將自動儲存現有快照並無縫重新掛載。"),
        WS_CHILD | WS_VISIBLE,
        30,
        330,
        435,
        30,
        hwnd,
        None,
        HINSTANCE(std::ptr::null_mut()),
        None,
    )
    .unwrap_or_default();
    set_ctrl_font(lbl_note, false);

    // 3. 按鈕區
    let btn_apply = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("套用並重新掛載"),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        250,
        390,
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
        400,
        390,
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
                ID_APPLY_BTN => {
                    // 讀取磁碟機代號
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

                    // 讀取容量
                    let cb_sz = GetDlgItem(hwnd, ID_SIZE_COMBO as _).unwrap_or_default();
                    let mut sz_buf = [0u16; 64];
                    GetWindowTextW(cb_sz, &mut sz_buf);
                    let sz_len = sz_buf.iter().position(|&c| c == 0).unwrap_or(sz_buf.len());
                    let raw_sz = String::from_utf16_lossy(&sz_buf[..sz_len]).trim().to_string();
                    let size_str = raw_sz.split_whitespace().next().unwrap_or("2G").to_string();

                    // 讀取壓縮模式
                    let cb_cp = GetDlgItem(hwnd, ID_COMPRESS_COMBO as _).unwrap_or_default();
                    let cur_cp = SendMessageW(cb_cp, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
                    let compress_str = if cur_cp == 1 { "none" } else { "lz4" }.to_string();

                    // 讀取快照路徑
                    let ed_img = GetDlgItem(hwnd, ID_IMAGE_EDIT as _).unwrap_or_default();
                    let mut img_buf = [0u16; 1024];
                    GetWindowTextW(ed_img, &mut img_buf);
                    let img_len = img_buf.iter().position(|&c| c == 0).unwrap_or(img_buf.len());
                    let raw_img = String::from_utf16_lossy(&img_buf[..img_len]).trim().to_string();
                    let image_opt = if raw_img.is_empty() { None } else { Some(raw_img) };

                    // 讀取開機自啟
                    let chk = GetDlgItem(hwnd, ID_AUTOSTART_CHECK as _).unwrap_or_default();
                    let is_checked = SendMessageW(chk, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;

                    let new_config = SettingsConfig {
                        mount_point: drive_str,
                        size: size_str,
                        compress: compress_str,
                        image_path: image_opt,
                        auto_start: is_checked,
                    };

                    // 儲存至登錄檔
                    save_config_to_registry(&new_config);

                    // 存入待套用 slot
                    {
                        let mut pending = PENDING_CONFIG.lock().unwrap();
                        *pending = Some(new_config);
                    }

                    // 通知 Host 視窗進行重新掛載
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
