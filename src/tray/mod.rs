use muda::{Menu, MenuItem, PredefinedMenuItem};
use thiserror::Error;
use tray_icon::{BadIcon, Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Error)]
pub enum TrayError {
    #[error("Failed to build tray icon: {0}")]
    IconError(#[from] tray_icon::Error),
    #[error("Invalid tray icon format: {0}")]
    BadIcon(#[from] BadIcon),
    #[error("Failed to build tray menu: {0}")]
    MenuError(#[from] muda::Error),
}

pub struct TrayManager {
    _tray_icon: TrayIcon,
    _menu: Menu,
    pub status_item: MenuItem,
    pub settings_item: MenuItem,
    pub view_logs_item: MenuItem,
    pub snapshot_item: MenuItem,
    pub unmount_item: MenuItem,
    pub exit_item: MenuItem,
}

impl TrayManager {
    pub fn new(app_name: &str, mount_point: &str, capacity_str: &str) -> Result<Self, TrayError> {
        let menu = Menu::new();

        let status_text = format!("{}: {} ({})", app_name, mount_point, capacity_str);
        let status_item = MenuItem::new(&status_text, false, None);
        let settings_item = MenuItem::new("⚙️ 磁碟設定 (Settings)...", true, None);
        let view_logs_item = MenuItem::new("📋 查看運行日誌 (View Logs)...", true, None);
        let separator1 = PredefinedMenuItem::separator();
        let snapshot_item = MenuItem::new("💾 立即儲存快照 (Save Snapshot)", true, None);
        let unmount_item = MenuItem::new("⏏️ 卸載磁碟 (Unmount)", true, None);
        let separator2 = PredefinedMenuItem::separator();
        let exit_item = MenuItem::new("❌ 結束 (Exit)", true, None);

        menu.append(&status_item)?;
        menu.append(&settings_item)?;
        menu.append(&view_logs_item)?;
        menu.append(&separator1)?;
        menu.append(&snapshot_item)?;
        menu.append(&unmount_item)?;
        menu.append(&separator2)?;
        menu.append(&exit_item)?;

        // Generate a 16x16 default colored RGBA icon
        let icon_rgba = Self::generate_default_icon_rgba(16, 16);
        let icon = Icon::from_rgba(icon_rgba, 16, 16)?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip(format!("{} - {}", app_name, mount_point))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray_icon: tray_icon,
            _menu: menu,
            status_item,
            settings_item,
            view_logs_item,
            snapshot_item,
            unmount_item,
            exit_item,
        })
    }

    pub fn tray_icon(&self) -> &TrayIcon {
        &self._tray_icon
    }

    /// Dynamically update the status line in the tray menu.
    pub fn update_status(&self, text: &str) {
        self.status_item.set_text(text);
    }

    fn generate_default_icon_rgba(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let is_border = x == 0 || x == width - 1 || y == 0 || y == height - 1;
                if is_border {
                    rgba.extend_from_slice(&[0, 80, 160, 255]);
                } else if (x + y) % 3 == 0 {
                    rgba.extend_from_slice(&[0, 200, 255, 255]); // bright cyan
                } else {
                    rgba.extend_from_slice(&[0, 150, 220, 255]);
                }
            }
        }
        rgba
    }
}
