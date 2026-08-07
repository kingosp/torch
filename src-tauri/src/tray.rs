//! System tray presence. Torch has no main window; the tray is the only
//! permanently visible part of the app.

use std::sync::OnceLock;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};

pub const TRAY_ID: &str = "torch-tray";

static ENABLED_ITEM: OnceLock<CheckMenuItem<Wry>> = OnceLock::new();

const ICON_ON: &[u8] = include_bytes!("../icons/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../icons/tray-off.png");

fn icon(enabled: bool) -> tauri::Result<Image<'static>> {
    Image::from_bytes(if enabled { ICON_ON } else { ICON_OFF })
}

pub fn build(app: &AppHandle, enabled: bool) -> tauri::Result<()> {
    let toggle = CheckMenuItem::with_id(app, "toggle", "Enabled", true, enabled, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &settings, &separator, &quit])?;
    let _ = ENABLED_ITEM.set(toggle);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(enabled)?)
        .tooltip(tooltip(enabled))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => {
                let next = crate::toggle_enabled(app);
                let _ = sync(app, next);
            }
            "settings" => crate::open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Keeps the tray icon (colored vs grayscale) and check state in sync.
pub fn sync(app: &AppHandle, enabled: bool) -> tauri::Result<()> {
    if let Some(item) = ENABLED_ITEM.get() {
        let _ = item.set_checked(enabled);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(icon(enabled)?))?;
        tray.set_tooltip(Some(tooltip(enabled)))?;
    }
    Ok(())
}

fn tooltip(enabled: bool) -> &'static str {
    if enabled {
        "Torch - active"
    } else {
        "Torch - paused"
    }
}
