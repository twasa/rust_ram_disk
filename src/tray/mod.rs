use muda::{Menu, MenuItem, PredefinedMenuItem};
use thiserror::Error;
use tray_icon::{BadIcon, Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("建立系統匣圖示失敗: {0}")]
    IconError(#[from] tray_icon::Error),
    #[error("無效的圖示格式: {0}")]
    BadIcon(#[from] BadIcon),
    #[error("建立右鍵選單失敗: {0}")]
    MenuError(#[from] muda::Error),
}

pub struct TrayManager {
    _tray_icon: TrayIcon,
    _menu: Menu,
    pub is_service_mode: bool,
    pub status_item: MenuItem,
    pub settings_item: MenuItem,
    pub view_logs_item: MenuItem,
    pub snapshot_item: MenuItem,
    pub service_toggle_item: Option<MenuItem>,
    pub service_restart_item: Option<MenuItem>,
    pub unmount_item: Option<MenuItem>,
    pub exit_item: MenuItem,
}

impl TrayManager {
    /// 建立系統服務監控模式的系統匣圖示
    pub fn new_service_mode(
        app_name: &str,
        mount_point: &str,
        capacity_str: &str,
        is_running: bool,
    ) -> Result<Self, TrayError> {
        let menu = Menu::new();

        let status_text = if is_running {
            format!("{}: {} ({}) [服務常駐]", app_name, mount_point, capacity_str)
        } else {
            format!("{}: 系統服務未啟動 / 離線", app_name)
        };
        let status_item = MenuItem::new(&status_text, false, None);

        let toggle_text = if is_running {
            "⏹️ 停止系統服務 (Stop Service)"
        } else {
            "▶️ 啟動系統服務 (Start Service)"
        };
        let service_toggle_item = MenuItem::new(toggle_text, true, None);
        let service_restart_item = MenuItem::new("🔄 重新啟動服務 (Restart Service)", true, None);

        let settings_item = MenuItem::new("⚙️ 系統服務與磁碟設定 (Settings)...", true, None);
        let view_logs_item = MenuItem::new("📋 查看運行日誌 (View Logs)...", true, None);
        let separator1 = PredefinedMenuItem::separator();
        let snapshot_item = MenuItem::new("💾 立即儲存快照 (Save Snapshot)", true, None);
        let separator2 = PredefinedMenuItem::separator();
        let exit_item = MenuItem::new("❌ 關閉監控面板 (Exit Tray - 磁碟保持常駐)", true, None);

        menu.append(&status_item)?;
        menu.append(&settings_item)?;
        menu.append(&view_logs_item)?;
        menu.append(&separator1)?;
        menu.append(&service_toggle_item)?;
        menu.append(&service_restart_item)?;
        menu.append(&snapshot_item)?;
        menu.append(&separator2)?;
        menu.append(&exit_item)?;

        // 依據服務狀態生成代表圖示 (綠色/青色代表運行中，橘黃色代表停止/離線)
        let icon_rgba = if is_running {
            Self::generate_service_running_icon_rgba(16, 16)
        } else {
            Self::generate_service_stopped_icon_rgba(16, 16)
        };
        let icon = Icon::from_rgba(icon_rgba, 16, 16)?;

        let tooltip = if is_running {
            format!("{} (服務常駐) - {} [{}]", app_name, mount_point, capacity_str)
        } else {
            format!("{} - 系統服務已停止", app_name)
        };

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray_icon: tray_icon,
            _menu: menu,
            is_service_mode: true,
            status_item,
            settings_item,
            view_logs_item,
            snapshot_item,
            service_toggle_item: Some(service_toggle_item),
            service_restart_item: Some(service_restart_item),
            unmount_item: None,
            exit_item,
        })
    }

    /// 建立獨立本機掛載模式的系統匣圖示 (相容獨立掛載指令)
    pub fn new_standalone_mode(
        app_name: &str,
        mount_point: &str,
        capacity_str: &str,
    ) -> Result<Self, TrayError> {
        let menu = Menu::new();

        let status_text = format!("{}: {} ({}) [本機掛載]", app_name, mount_point, capacity_str);
        let status_item = MenuItem::new(&status_text, false, None);
        let settings_item = MenuItem::new("⚙️ 磁碟設定 (Settings)...", true, None);
        let view_logs_item = MenuItem::new("📋 查看運行日誌 (View Logs)...", true, None);
        let separator1 = PredefinedMenuItem::separator();
        let snapshot_item = MenuItem::new("💾 立即儲存快照 (Save Snapshot)", true, None);
        let unmount_item = MenuItem::new("⏏️ 卸載磁碟 (Unmount)", true, None);
        let separator2 = PredefinedMenuItem::separator();
        let exit_item = MenuItem::new("❌ 結束並卸載 (Exit & Unmount)", true, None);

        menu.append(&status_item)?;
        menu.append(&settings_item)?;
        menu.append(&view_logs_item)?;
        menu.append(&separator1)?;
        menu.append(&snapshot_item)?;
        menu.append(&unmount_item)?;
        menu.append(&separator2)?;
        menu.append(&exit_item)?;

        let icon_rgba = Self::generate_service_running_icon_rgba(16, 16);
        let icon = Icon::from_rgba(icon_rgba, 16, 16)?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip(format!("{} - {}", app_name, mount_point))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray_icon: tray_icon,
            _menu: menu,
            is_service_mode: false,
            status_item,
            settings_item,
            view_logs_item,
            snapshot_item,
            service_toggle_item: None,
            service_restart_item: None,
            unmount_item: Some(unmount_item),
            exit_item,
        })
    }

    pub fn tray_icon(&self) -> &TrayIcon {
        &self._tray_icon
    }

    /// 動態更新服務運行狀態文字與圖示
    pub fn update_service_status(&self, mount_point: &str, capacity_str: &str, is_running: bool) {
        if is_running {
            self.status_item.set_text(format!(
                "Rust RAM Disk: {} ({}) [服務常駐]",
                mount_point, capacity_str
            ));
            if let Some(ref toggle) = self.service_toggle_item {
                toggle.set_text("⏹️ 停止系統服務 (Stop Service)");
            }
            let _ = self._tray_icon.set_tooltip(Some(format!(
                "Rust RAM Disk (服務常駐) - {} [{}]",
                mount_point, capacity_str
            )));
            if let Ok(icon) = Icon::from_rgba(Self::generate_service_running_icon_rgba(16, 16), 16, 16) {
                let _ = self._tray_icon.set_icon(Some(icon));
            }
        } else {
            self.status_item.set_text("Rust RAM Disk: 系統服務未啟動 / 離線");
            if let Some(ref toggle) = self.service_toggle_item {
                toggle.set_text("▶️ 啟動系統服務 (Start Service)");
            }
            let _ = self._tray_icon.set_tooltip(Some("Rust RAM Disk - 系統服務未啟動".to_string()));
            if let Ok(icon) = Icon::from_rgba(Self::generate_service_stopped_icon_rgba(16, 16), 16, 16) {
                let _ = self._tray_icon.set_icon(Some(icon));
            }
        }
    }

    /// 動態更新選單狀態列文字
    pub fn update_status(&self, text: &str) {
        self.status_item.set_text(text);
    }

    /// 生成服務正常運行之青藍色圖示
    fn generate_service_running_icon_rgba(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;
                if is_border {
                    rgba.extend_from_slice(&[0, 80, 160, 255]);
                } else if (x + y) % 3 == 0 {
                    rgba.extend_from_slice(&[0, 220, 255, 255]); // bright cyan
                } else {
                    rgba.extend_from_slice(&[0, 150, 220, 255]);
                }
            }
        }
        rgba
    }

    /// 生成服務停止/離線時之橘黃色警示圖示
    fn generate_service_stopped_icon_rgba(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;
                if is_border {
                    rgba.extend_from_slice(&[180, 100, 0, 255]);
                } else if (x + y) % 3 == 0 {
                    rgba.extend_from_slice(&[255, 180, 50, 255]); // bright orange/yellow
                } else {
                    rgba.extend_from_slice(&[220, 140, 20, 255]);
                }
            }
        }
        rgba
    }
}
