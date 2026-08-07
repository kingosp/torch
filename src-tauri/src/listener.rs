//! Global keyboard hook, hold detection and synthetic key output.
//!
//! Everything latency sensitive happens here and nowhere else. The low level
//! hook callback only takes an uncontended lock and pushes a message onto a
//! channel, so the OS input thread is never blocked by the UI, by disk I/O or
//! by the Tauri event loop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::config::Config;

/// Stamped into every synthetic event so the hook can ignore its own output and
/// never trigger itself recursively.
pub const TORCH_SIGNATURE: usize = 0x544f_5243; // "TORC"

/// What the popup UI needs to know about. Delivered on a worker thread, never
/// from inside the hook callback.
#[derive(Debug, Clone)]
pub enum HookEvent {
    Show {
        trigger: String,
        items: Vec<String>,
        index: usize,
    },
    Selection {
        index: usize,
    },
    Hide {
        committed: bool,
    },
}

#[derive(Debug)]
enum Action {
    /// Replay a trigger key that turned out to be a normal short press.
    Tap(u32),
    /// Type one of the mapped keys.
    Send(String),
    Notify(HookEvent),
}

#[derive(Debug, Clone)]
struct Press {
    vk: u32,
    at: Instant,
    triggered: bool,
}

#[derive(Debug, Clone)]
struct Session {
    vk: u32,
    items: Vec<String>,
    index: usize,
}

pub struct Listener {
    enabled: AtomicBool,
    hold_ms: AtomicU64,
    instant_passthrough: AtomicBool,
    mapping: Mutex<HashMap<u32, (String, Vec<String>)>>,
    press: Mutex<Option<Press>>,
    session: Mutex<Option<Session>>,
    tx: Sender<Action>,
}

static LISTENER: OnceLock<Arc<Listener>> = OnceLock::new();

pub fn instance() -> Option<&'static Arc<Listener>> {
    LISTENER.get()
}

impl Listener {
    /// Installs the global hook and spawns the worker + hold watcher threads.
    /// Returns the shared handle; calling it twice yields the first listener.
    pub fn start<F>(on_event: F) -> Arc<Listener>
    where
        F: Fn(HookEvent) + Send + Sync + 'static,
    {
        if let Some(existing) = LISTENER.get() {
            return existing.clone();
        }

        let (tx, rx) = mpsc::channel::<Action>();
        let listener = Arc::new(Listener {
            enabled: AtomicBool::new(true),
            hold_ms: AtomicU64::new(2000),
            instant_passthrough: AtomicBool::new(false),
            mapping: Mutex::new(HashMap::new()),
            press: Mutex::new(None),
            session: Mutex::new(None),
            tx,
        });
        let listener = LISTENER.get_or_init(|| listener).clone();

        std::thread::Builder::new()
            .name("torch-output".into())
            .spawn(move || {
                while let Ok(action) = rx.recv() {
                    match action {
                        Action::Tap(vk) => keys::tap_vk(vk),
                        Action::Send(name) => keys::send_named(&name),
                        Action::Notify(event) => on_event(event),
                    }
                }
            })
            .expect("spawn torch-output thread");

        let watcher = listener.clone();
        std::thread::Builder::new()
            .name("torch-hold".into())
            .spawn(move || loop {
                let armed = watcher.tick();
                std::thread::sleep(Duration::from_millis(if armed { 4 } else { 20 }));
            })
            .expect("spawn torch-hold thread");

        platform::install(listener.clone());
        listener
    }

    pub fn apply_config(&self, config: &Config) {
        self.enabled.store(config.enabled, Ordering::Relaxed);
        self.hold_ms.store(config.hold_time_ms(), Ordering::Relaxed);
        self.instant_passthrough
            .store(config.instant_passthrough, Ordering::Relaxed);

        let mut mapping = HashMap::with_capacity(config.mapping.len());
        for (trigger, targets) in &config.mapping {
            if let Some(vk) = keys::vk_from_name(trigger) {
                mapping.insert(vk, (trigger.clone(), targets.clone()));
            }
        }
        *self.mapping.lock() = mapping;
        self.cancel();
    }

    /// Mouse hover / programmatic highlight coming from the popup UI.
    pub fn set_selection(&self, index: usize) {
        let mut guard = self.session.lock();
        if let Some(session) = guard.as_mut() {
            if index < session.items.len() && session.index != index {
                session.index = index;
                let event = HookEvent::Selection { index };
                drop(guard);
                self.push(Action::Notify(event));
            }
        }
    }

    /// Mouse click on an item.
    pub fn choose(&self, index: usize) {
        self.set_selection(index);
        self.commit();
    }

    /// Sends the highlighted key and closes the popup.
    pub fn commit(&self) {
        let session = self.session.lock().take();
        let Some(session) = session else { return };
        *self.press.lock() = None;
        if let Some(name) = session.items.get(session.index) {
            self.push(Action::Send(name.clone()));
        }
        self.push(Action::Notify(HookEvent::Hide { committed: true }));
    }

    /// Closes the popup without emitting anything.
    pub fn cancel(&self) {
        *self.press.lock() = None;
        if self.session.lock().take().is_some() {
            self.push(Action::Notify(HookEvent::Hide { committed: false }));
        }
    }

    fn push(&self, action: Action) {
        let _ = self.tx.send(action);
    }

    /// Returns true while a trigger key is being held (watcher polls faster).
    fn tick(&self) -> bool {
        let hold = Duration::from_millis(self.hold_ms.load(Ordering::Relaxed));
        let mut guard = self.press.lock();
        let Some(press) = guard.as_mut() else {
            return false;
        };
        if press.triggered || press.at.elapsed() < hold {
            return true;
        }
        press.triggered = true;
        let vk = press.vk;
        drop(guard);

        let Some((trigger, items)) = self.mapping.lock().get(&vk).cloned() else {
            return true;
        };
        if items.is_empty() {
            return true;
        }
        *self.session.lock() = Some(Session {
            vk,
            items: items.clone(),
            index: 0,
        });
        self.push(Action::Notify(HookEvent::Show {
            trigger,
            items,
            index: 0,
        }));
        true
    }

    fn step_selection(&self, delta: isize) {
        let mut guard = self.session.lock();
        let Some(session) = guard.as_mut() else {
            return;
        };
        let len = session.items.len() as isize;
        if len == 0 {
            return;
        }
        let next = (session.index as isize + delta).rem_euclid(len) as usize;
        session.index = next;
        drop(guard);
        self.push(Action::Notify(HookEvent::Selection { index: next }));
    }

    /// Core hook decision. `true` means "swallow this event".
    ///
    /// Runs on the OS input thread: no allocation-heavy work, no blocking.
    fn handle_key(&self, vk: u32, down: bool) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }

        if let Some(session_vk) = self.session.lock().as_ref().map(|s| s.vk) {
            return self.handle_key_with_popup(vk, down, session_vk);
        }

        let instant = self.instant_passthrough.load(Ordering::Relaxed);
        let mapped = self.mapping.lock().contains_key(&vk);

        if down {
            if mapped {
                let mut guard = self.press.lock();
                match guard.as_ref() {
                    Some(press) if press.vk == vk => {} // auto repeat
                    _ => {
                        *guard = Some(Press {
                            vk,
                            at: Instant::now(),
                            triggered: false,
                        })
                    }
                }
                return !instant;
            }
            // Another key interrupts the hold: flush the pending trigger first
            // so the typed order stays intact.
            if let Some(press) = self.press.lock().take() {
                if !instant && !press.triggered {
                    self.push(Action::Tap(press.vk));
                }
            }
            return false;
        }

        let released = {
            let mut guard = self.press.lock();
            match guard.as_ref() {
                Some(press) if press.vk == vk => guard.take(),
                _ => None,
            }
        };
        match released {
            Some(press) if !instant && !press.triggered => {
                self.push(Action::Tap(vk));
                true
            }
            _ => mapped && !instant,
        }
    }

    fn handle_key_with_popup(&self, vk: u32, down: bool, session_vk: u32) -> bool {
        if !down {
            if vk == session_vk {
                self.commit();
                return true;
            }
            return keys::is_popup_control(vk);
        }

        if vk == session_vk {
            return true; // auto repeat of the held trigger
        }
        match vk {
            keys::VK_LEFT | keys::VK_UP => {
                self.step_selection(-1);
                true
            }
            keys::VK_RIGHT | keys::VK_DOWN => {
                self.step_selection(1);
                true
            }
            keys::VK_RETURN | keys::VK_SPACE => {
                self.commit();
                true
            }
            keys::VK_ESCAPE => {
                self.cancel();
                true
            }
            _ => {
                self.cancel();
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Key name <-> virtual key translation and synthetic output.
// ---------------------------------------------------------------------------

pub mod keys {
    pub const VK_BACK: u32 = 0x08;
    pub const VK_TAB: u32 = 0x09;
    pub const VK_RETURN: u32 = 0x0D;
    pub const VK_SHIFT: u32 = 0x10;
    pub const VK_CONTROL: u32 = 0x11;
    pub const VK_MENU: u32 = 0x12;
    pub const VK_ESCAPE: u32 = 0x1B;
    pub const VK_SPACE: u32 = 0x20;
    pub const VK_LEFT: u32 = 0x25;
    pub const VK_UP: u32 = 0x26;
    pub const VK_RIGHT: u32 = 0x27;
    pub const VK_DOWN: u32 = 0x28;
    pub const VK_LWIN: u32 = 0x5B;

    pub fn is_popup_control(vk: u32) -> bool {
        matches!(
            vk,
            VK_LEFT | VK_UP | VK_RIGHT | VK_DOWN | VK_RETURN | VK_SPACE | VK_ESCAPE
        )
    }

    /// Keys that must carry `KEYEVENTF_EXTENDEDKEY` to be delivered correctly.
    pub fn is_extended(vk: u32) -> bool {
        matches!(
            vk,
            0x21..=0x28 // pageup..down arrow
                | 0x2D | 0x2E // insert, delete
                | 0x5B | 0x5C | 0x5D // win keys, apps
                | 0x6F // numpad divide
                | 0x90 // numlock
                | 0xA3 // right control
                | 0xA5 // right alt
        )
    }

    /// Accepts single characters (`"w"`), named keys (`"enter"`, `"f5"`,
    /// `"numpad3"`) and modifier chords (`"ctrl+shift+t"`).
    pub fn vk_from_name(name: &str) -> Option<u32> {
        let key = name.trim().to_ascii_lowercase();
        if key.len() == 1 {
            let c = key.as_bytes()[0];
            if c.is_ascii_lowercase() {
                return Some((c.to_ascii_uppercase()) as u32);
            }
            if c.is_ascii_digit() {
                return Some(c as u32);
            }
            return match c {
                b';' => Some(0xBA),
                b'=' => Some(0xBB),
                b',' => Some(0xBC),
                b'-' => Some(0xBD),
                b'.' => Some(0xBE),
                b'/' => Some(0xBF),
                b'`' => Some(0xC0),
                b'[' => Some(0xDB),
                b'\\' => Some(0xDC),
                b']' => Some(0xDD),
                b'\'' => Some(0xDE),
                _ => None,
            };
        }
        if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
            if (1..=24).contains(&n) {
                return Some(0x70 + n - 1);
            }
        }
        if let Some(n) = key
            .strip_prefix("numpad")
            .and_then(|n| n.parse::<u32>().ok())
        {
            if n <= 9 {
                return Some(0x60 + n);
            }
        }
        Some(match key.as_str() {
            "backspace" | "back" => VK_BACK,
            "tab" => VK_TAB,
            "enter" | "return" => VK_RETURN,
            "shift" => VK_SHIFT,
            "ctrl" | "control" => VK_CONTROL,
            "alt" => VK_MENU,
            "pause" => 0x13,
            "capslock" => 0x14,
            "esc" | "escape" => VK_ESCAPE,
            "space" => VK_SPACE,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "end" => 0x23,
            "home" => 0x24,
            "left" => VK_LEFT,
            "up" => VK_UP,
            "right" => VK_RIGHT,
            "down" => VK_DOWN,
            "printscreen" => 0x2C,
            "insert" => 0x2D,
            "delete" | "del" => 0x2E,
            "win" | "meta" | "super" => VK_LWIN,
            "apps" | "menu" => 0x5D,
            "multiply" => 0x6A,
            "add" | "plus" => 0x6B,
            "subtract" => 0x6D,
            "decimal" => 0x6E,
            "divide" => 0x6F,
            "numlock" => 0x90,
            "scrolllock" => 0x91,
            "volumemute" => 0xAD,
            "volumedown" => 0xAE,
            "volumeup" => 0xAF,
            "medianext" => 0xB0,
            "mediaprev" => 0xB1,
            "mediastop" => 0xB2,
            "mediaplay" => 0xB3,
            _ => return None,
        })
    }

    /// Splits `"ctrl+shift+t"` into its modifiers and the final key.
    pub fn parse_chord(name: &str) -> (Vec<u32>, Option<String>) {
        let mut modifiers = Vec::new();
        let mut parts: Vec<&str> = name.split('+').collect();
        // A literal "+" target survives as an empty tail segment.
        if name.trim() == "+" {
            return (modifiers, Some("+".into()));
        }
        let key = parts.pop().map(|k| k.trim().to_string());
        for part in parts {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.push(VK_CONTROL),
                "shift" => modifiers.push(VK_SHIFT),
                "alt" => modifiers.push(VK_MENU),
                "win" | "meta" | "super" => modifiers.push(VK_LWIN),
                _ => {}
            }
        }
        (modifiers, key)
    }

    #[cfg(windows)]
    pub fn tap_vk(vk: u32) {
        super::platform::send_vk(vk, true);
        super::platform::send_vk(vk, false);
    }

    #[cfg(windows)]
    pub fn send_named(name: &str) {
        let (modifiers, key) = parse_chord(name);
        let Some(key) = key else { return };
        for m in &modifiers {
            super::platform::send_vk(*m, true);
        }
        match vk_from_name(&key) {
            Some(vk) => tap_vk(vk),
            None => {
                for ch in key.chars() {
                    super::platform::send_unicode(ch);
                }
            }
        }
        for m in modifiers.iter().rev() {
            super::platform::send_vk(*m, false);
        }
    }

    #[cfg(not(windows))]
    pub fn tap_vk(_vk: u32) {}

    #[cfg(not(windows))]
    pub fn send_named(_name: &str) {}
}

// ---------------------------------------------------------------------------
// Win32 glue.
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub mod platform {
    use std::sync::Arc;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE,
        MAPVK_VK_TO_VSC, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::{Listener, TORCH_SIGNATURE};

    /// Runs the hook on its own thread with a dedicated message pump; the hook
    /// callback is therefore never delayed by the Tauri/WebView loop.
    pub fn install(_listener: Arc<Listener>) {
        std::thread::Builder::new()
            .name("torch-hook".into())
            .spawn(|| unsafe {
                let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
                    Ok(hook) => hook,
                    Err(err) => {
                        eprintln!("torch: failed to install keyboard hook: {err}");
                        return;
                    }
                };
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(hook);
            })
            .expect("spawn torch-hook thread");
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Ignore anything Torch itself injected.
            if info.dwExtraInfo != TORCH_SIGNATURE {
                let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
                if down || up {
                    if let Some(listener) = super::instance() {
                        if listener.handle_key(info.vkCode, down) {
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn send_vk(vk: u32, down: bool) {
        unsafe {
            let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) as u16;
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if !down {
                flags |= KEYEVENTF_KEYUP;
            }
            if super::keys::is_extended(vk) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if scan != 0 {
                flags |= KEYEVENTF_SCANCODE;
            }
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk as u16),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: TORCH_SIGNATURE,
                    },
                },
            };
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_unicode(ch: char) {
        let mut buf = [0u16; 2];
        for unit in ch.encode_utf16(&mut buf) {
            for up in [false, true] {
                let mut flags = KEYEVENTF_UNICODE;
                if up {
                    flags |= KEYEVENTF_KEYUP;
                }
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: *unit,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: TORCH_SIGNATURE,
                        },
                    },
                };
                unsafe {
                    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                }
            }
        }
    }

    /// Physical cursor position, used to anchor the radial popup.
    pub fn cursor_position() -> (i32, i32) {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        (point.x, point.y)
    }
}

#[cfg(not(windows))]
pub mod platform {
    use std::sync::Arc;

    use super::Listener;

    pub fn install(_listener: Arc<Listener>) {}
    pub fn send_vk(_vk: u32, _down: bool) {}
    pub fn send_unicode(_ch: char) {}
    pub fn cursor_position() -> (i32, i32) {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::keys::*;

    #[test]
    fn resolves_letters_digits_and_named_keys() {
        assert_eq!(vk_from_name("w"), Some(0x57));
        assert_eq!(vk_from_name("3"), Some(0x33));
        assert_eq!(vk_from_name("F5"), Some(0x74));
        assert_eq!(vk_from_name("enter"), Some(VK_RETURN));
        assert_eq!(vk_from_name("numpad7"), Some(0x67));
        assert_eq!(vk_from_name("nonsense"), None);
    }

    #[test]
    fn parses_modifier_chords() {
        let (modifiers, key) = parse_chord("ctrl+shift+t");
        assert_eq!(modifiers, vec![VK_CONTROL, VK_SHIFT]);
        assert_eq!(key.as_deref(), Some("t"));
    }
}
