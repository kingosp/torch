// Torch runs headless: never spawn a console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod listener;
mod startup;
mod tray;

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use config::Config;
use listener::{HookEvent, Listener};

const POPUP_LABEL: &str = "popup";
const SETTINGS_LABEL: &str = "settings";
const POPUP_SIZE: f64 = 460.0;

const EVENT_SHOW: &str = "torch://show";
const EVENT_SELECTION: &str = "torch://selection";
const EVENT_HIDE: &str = "torch://hide";
const EVENT_CONFIG: &str = "torch://config";

struct AppState {
    config: Mutex<Config>,
    path: PathBuf,
}

#[derive(Serialize, Clone)]
struct ShowPayload {
    trigger: String,
    items: Vec<String>,
    index: usize,
}

#[derive(Serialize, Clone)]
struct SelectionPayload {
    index: usize,
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            set_enabled,
            popup_hover,
            popup_choose,
            popup_cancel,
            open_settings,
            quit
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let dir = app.path().app_config_dir()?;
            let path = config::resolve_path(&dir);
            let loaded = config::load(&path);
            app.manage(AppState {
                config: Mutex::new(loaded.clone()),
                path,
            });

            build_popup_window(&handle)?;
            tray::build(&handle, loaded.enabled)?;

            let events = handle.clone();
            let listener = Listener::start(move |event| on_hook_event(&events, event));
            listener.apply_config(&loaded);
            let _ = startup::set(loaded.startup);

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window must not take the background app down.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == SETTINGS_LABEL {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to start Torch")
        .run(|_app, event| {
            // No windows are open most of the time - keep the process alive.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

fn build_popup_window(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, POPUP_LABEL, WebviewUrl::App("index.html".into()))
        .title("Torch")
        .inner_size(POPUP_SIZE, POPUP_SIZE)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;
    make_non_activating(&window);
    Ok(())
}

pub fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let built =
        WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
            .title("Torch Settings")
            .inner_size(900.0, 680.0)
            .min_inner_size(760.0, 560.0)
            .decorations(false)
            .transparent(true)
            .center()
            .build();
    if let Ok(window) = built {
        let _ = window.set_focus();
    }
}

/// Marks the popup as a non-activating tool window so it never steals focus
/// from whatever the user is typing into.
#[cfg(windows)]
fn make_non_activating(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let Ok(raw) = window.hwnd() else { return };
    let hwnd = HWND(raw.0 as _);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let extra = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | extra);
    }
}

#[cfg(not(windows))]
fn make_non_activating(_window: &tauri::WebviewWindow) {}

// ---------------------------------------------------------------------------
// Hook -> UI
// ---------------------------------------------------------------------------

fn on_hook_event(app: &AppHandle, event: HookEvent) {
    match event {
        HookEvent::Show {
            trigger,
            items,
            index,
        } => {
            let anchor = {
                let state = app.state::<AppState>();
                let config = state.config.lock();
                config.popup_anchor.clone()
            };
            let Some(window) = app.get_webview_window(POPUP_LABEL) else {
                return;
            };
            let _ = app.emit_to(
                POPUP_LABEL,
                EVENT_SHOW,
                ShowPayload {
                    trigger,
                    items,
                    index,
                },
            );
            position_popup(&window, &anchor);
            let _ = window.show();
            let _ = window.set_always_on_top(true);
        }
        HookEvent::Selection { index } => {
            let _ = app.emit_to(POPUP_LABEL, EVENT_SELECTION, SelectionPayload { index });
        }
        HookEvent::Hide { committed } => {
            let _ = app.emit_to(POPUP_LABEL, EVENT_HIDE, committed);
            if let Some(window) = app.get_webview_window(POPUP_LABEL) {
                // Let the exit animation play before the window disappears.
                let handle = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(160));
                    let _ = handle.hide();
                });
            }
        }
    }
}

fn position_popup(window: &tauri::WebviewWindow, anchor: &config::PopupAnchor) {
    let size = match window.outer_size() {
        Ok(size) => size,
        Err(_) => return,
    };
    let (w, h) = (size.width as i32, size.height as i32);

    let (cx, cy) = match anchor {
        config::PopupAnchor::Cursor => listener::platform::cursor_position(),
        config::PopupAnchor::Center => match window.current_monitor() {
            Ok(Some(monitor)) => {
                let pos = monitor.position();
                let size = monitor.size();
                (
                    pos.x + size.width as i32 / 2,
                    pos.y + size.height as i32 / 2,
                )
            }
            _ => (w / 2, h / 2),
        },
    };

    let mut x = cx - w / 2;
    let mut y = cy - h / 2;

    if let Ok(Some(monitor)) = window.current_monitor() {
        let pos = monitor.position();
        let msize = monitor.size();
        let margin = 8;
        x = x.clamp(pos.x + margin, pos.x + msize.width as i32 - w - margin);
        y = y.clamp(pos.y + margin, pos.y + msize.height as i32 - h - margin);
    }

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

// ---------------------------------------------------------------------------
// Shared config plumbing
// ---------------------------------------------------------------------------

fn apply_config(app: &AppHandle, mut next: Config) -> Result<Config, String> {
    next.sanitize();

    let (path, previous) = {
        let state = app.state::<AppState>();
        let previous = state.config.lock().clone();
        (state.path.clone(), previous)
    };

    if next.startup != previous.startup {
        startup::set(next.startup)?;
    }
    config::save(&path, &next)?;

    {
        let state = app.state::<AppState>();
        *state.config.lock() = next.clone();
    }

    if let Some(listener) = listener::instance() {
        listener.apply_config(&next);
    }
    if next.enabled != previous.enabled {
        let _ = tray::sync(app, next.enabled);
    }
    let _ = app.emit(EVENT_CONFIG, next.clone());
    Ok(next)
}

pub fn toggle_enabled(app: &AppHandle) -> bool {
    let mut next = app.state::<AppState>().config.lock().clone();
    next.enabled = !next.enabled;
    let enabled = next.enabled;
    let _ = apply_config(app, next);
    enabled
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> Config {
    state.config.lock().clone()
}

#[tauri::command]
fn set_config(app: AppHandle, config: Config) -> Result<Config, String> {
    apply_config(&app, config)
}

#[tauri::command]
fn set_enabled(app: AppHandle, enabled: bool) -> Result<Config, String> {
    let mut next = app.state::<AppState>().config.lock().clone();
    next.enabled = enabled;
    apply_config(&app, next)
}

#[tauri::command]
fn popup_hover(index: usize) {
    if let Some(listener) = listener::instance() {
        listener.set_selection(index);
    }
}

#[tauri::command]
fn popup_choose(index: usize) {
    if let Some(listener) = listener::instance() {
        listener.choose(index);
    }
}

#[tauri::command]
fn popup_cancel() {
    if let Some(listener) = listener::instance() {
        listener.cancel();
    }
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    open_settings_window(&app);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}
