# Torch

Torch is an offline Windows background app that gives broken or unreachable keyboard keys a
second life. Hold a trigger key and a glass radial selector fades in around the cursor; release
the key and Torch types the highlighted replacement.

Nothing leaves the machine: no telemetry, no network calls, no accounts. The whole state lives in
a single `config.json`.

## How it behaves

| Input | Result |
| --- | --- |
| Tap a trigger key (< `hold_time`) | The key is passed through untouched |
| Hold a trigger key (>= `hold_time`) | The radial selector opens |
| Move the mouse / press arrow keys | The highlight snaps to the nearest option |
| Release the trigger key | The highlighted key is typed and the popup closes |
| Click an option | Same as releasing on it |
| `Esc`, or any other key | Cancels without typing anything |

A tapped trigger key is replayed on release by default, so the hold gesture never leaks a stray
character while you wait. If you would rather have the key committed on key-down, turn on
**Instant pass-through** in settings - the wheel still opens, but the trigger key is already in
the text field when it does. Keys that are not part of any mapping are never touched: the hook
returns them to Windows immediately.

## Architecture

```
src/                     frontend (no framework, no bundler)
  index.html  style.css  glass popup shell
  radial.js              radial layout, magnetic snapping, interpolation loop
  app.js                 popup controller, bridges Rust events to the selector
  settings.html/.css/.js settings panel with a live preview of the wheel
  theme.js               shared auto / dark / light / custom theme resolution

src-tauri/
  src/main.rs            Tauri wiring, windows, commands, popup placement
  src/listener.rs        WH_KEYBOARD_LL hook, hold detection, SendInput output
  src/tray.rs            tray icon + menu (Enable / Settings / Exit)
  src/config.rs          config.json load / save / sanitize
  src/startup.rs         HKCU\...\CurrentVersion\Run integration
```

### Input path

The low level keyboard hook runs on its own thread with a dedicated message pump, so the WebView
can never stall it. The callback itself only takes an uncontended lock and pushes onto a channel;
every synthetic key press, every Tauri event and all disk I/O happen on worker threads.

Synthetic events are stamped with `dwExtraInfo = 0x544f5243` and the hook drops anything carrying
that signature, which is what keeps a mapping like `s -> s` from triggering itself.

## Configuration

`config.json` is read from next to the executable when present (portable install), otherwise from
`%APPDATA%\app.torch.remap\config.json`.

```json
{
  "enabled": true,
  "startup": false,
  "hold_time": 2.0,
  "theme": "auto",
  "popup_anchor": "cursor",
  "instant_passthrough": false,
  "custom_theme": {
    "accent": "#00aaff",
    "bg": "#1a1a1a",
    "text": "#ffffff",
    "highlight": "#ffffff",
    "blur": 20
  },
  "mapping": {
    "s": ["w", "x", "e"],
    "d": ["k", "l"]
  }
}
```

Mapping targets accept single characters (`"w"`), named keys (`"enter"`, `"f5"`, `"numpad3"`,
`"up"`) and chords (`"ctrl+shift+t"`). Anything that is not a known key name is typed as Unicode
text.

## Build

```powershell
npm install
npm run build     # -> src-tauri/target/release/bundle/nsis/Torch_<version>_x64-setup.exe
npm run dev       # hot-reloading dev build
npm run icons     # regenerate the flame artwork from tools/make-icons.mjs
```

Requirements: Rust (MSVC toolchain), Visual Studio Build Tools with the C++ workload, Node 18+
and the WebView2 runtime (preinstalled on Windows 10 21H2 and later).

The build produces both a portable `torch.exe` (~3.4 MB) and an NSIS installer (~1.2 MB), well
under the 30 MB budget.

## Notes

- The popup is a `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` window, so it never steals focus from the
  app you are typing into and never shows up in Alt-Tab or the taskbar.
- Torch has no main window. The tray icon is colored while active and grayscale while paused.
- "Start with Windows" writes a single `Torch` value under
  `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`.
