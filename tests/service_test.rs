use rust_ram_disk::service::{self, parse_size_bytes, ServiceConfig, ServiceState};
use rust_ram_disk::tray::TrayManager;
use rust_ram_disk::ui::settings::SettingsConfig;

#[test]
fn test_parse_size_bytes() {
    assert_eq!(parse_size_bytes("512M").unwrap(), 512 * 1024 * 1024);
    assert_eq!(parse_size_bytes("1G").unwrap(), 1024 * 1024 * 1024);
    assert_eq!(parse_size_bytes("2g").unwrap(), 2 * 1024 * 1024 * 1024);
    assert_eq!(parse_size_bytes("4G").unwrap(), 4 * 1024 * 1024 * 1024);
    assert_eq!(parse_size_bytes("1024K").unwrap(), 1024 * 1024);
    assert_eq!(parse_size_bytes("4096").unwrap(), 4096);
    assert!(parse_size_bytes("").is_err());
    assert!(parse_size_bytes("invalid").is_err());
}

#[test]
fn test_service_config_serialization() {
    let cfg = ServiceConfig {
        mount_point: "S:".to_string(),
        size: "4G".to_string(),
        compress: "lz4".to_string(),
        image_path: Some("C:\\data\\ramdisk.img".to_string()),
        auto_start: true,
    };

    let json = serde_json::to_string_pretty(&cfg).unwrap();
    let loaded: ServiceConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, loaded);
}

#[test]
fn test_service_config_to_settings_config_conversion() {
    let svc_cfg = ServiceConfig {
        mount_point: "R:".to_string(),
        size: "2G".to_string(),
        compress: "lz4".to_string(),
        image_path: None,
        auto_start: true,
    };

    let settings_cfg: SettingsConfig = svc_cfg.clone().into();
    assert_eq!(settings_cfg.mount_point, "R:");
    assert_eq!(settings_cfg.size, "2G");
    assert_eq!(settings_cfg.compress, "lz4");
    assert_eq!(settings_cfg.image_path, None);
    assert!(settings_cfg.auto_start);

    let roundtrip: ServiceConfig = settings_cfg.into();
    assert_eq!(svc_cfg, roundtrip);
}

#[test]
fn test_service_state_display() {
    assert_eq!(format!("{}", ServiceState::Running), "運行中 (Running)");
    assert_eq!(format!("{}", ServiceState::Stopped), "已停止 (Stopped)");
    assert_eq!(format!("{}", ServiceState::NotInstalled), "未安裝 (Not Installed)");
}

#[test]
fn test_service_query_status_smoke() {
    #[cfg(target_os = "windows")]
    {
        // 查詢本機 SCM 服務狀態，應回傳有效狀態而不崩潰
        let status = service::query_status();
        println!("Service query status result: {:?}", status);
        assert!(status.is_ok());
    }
}

#[test]
fn test_tray_manager_service_mode_creation() {
    #[cfg(target_os = "windows")]
    {
        let tray_res = TrayManager::new_service_mode("Rust RAM Disk", "R:", "2G", true);
        assert!(tray_res.is_ok());
        let tray = tray_res.unwrap();
        assert!(tray.is_service_mode);
        assert!(tray.service_toggle_item.is_some());
        assert!(tray.service_restart_item.is_some());
        assert!(tray.unmount_item.is_none());
    }
}

#[test]
fn test_tray_manager_standalone_mode_creation() {
    #[cfg(target_os = "windows")]
    {
        let tray_res = TrayManager::new_standalone_mode("Rust RAM Disk", "R:", "2G");
        assert!(tray_res.is_ok());
        let tray = tray_res.unwrap();
        assert!(!tray.is_service_mode);
        assert!(tray.service_toggle_item.is_none());
        assert!(tray.unmount_item.is_some());
    }
}
