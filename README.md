# Torch

Torch is an offline Windows tray app for instant keyboard remapping. Set a modifier chord and a replacement key, for example `Ctrl + Alt + Q -> Enter`. When the chord is pressed, Torch suppresses `Q` and sends `Enter` to the focused app immediately.

## Use it

1. Start Torch and open **Settings** from its tray icon.
2. Choose one or more modifiers, then pick a working key and replacement on the visual keyboard.
3. Click **Add remap**. Changes are saved locally and take effect immediately.

If a shortcut includes `Shift` only as an extra modifier, Torch keeps Shift active for the replacement. For example, `Alt+W -> 2` types `@` when pressed as `Alt+Shift+W`. If an explicit `Alt+Shift+W` remap exists, that exact mapping takes priority.

The hook runs on a dedicated Windows thread. Unmatched keys pass through. A matched source key is swallowed on both key-down and key-up so it cannot leak into the active application. Before sending the replacement, Torch briefly releases the held source modifiers and restores them, which lets a replacement such as `Enter` behave as an unmodified key. Synthetic events carry a private signature and are ignored by the hook.

Torch stores settings in `config.json` beside a portable executable when present, otherwise under `%APPDATA%\app.torch.remap\config.json`. It makes no network requests and sends no telemetry. **Start with Windows** adds a per-user Run entry.

## Build on Windows

Requirements: Node.js 18+, Rust stable with the MSVC target, Microsoft C++ Build Tools, and WebView2.

```powershell
npm install
npm run build
```

The Tauri build creates an NSIS installer under `src-tauri\target\release\bundle\nsis` and a standalone executable under `src-tauri\target\release\torch.exe`.

## Configuration

The `remaps` object maps normalized shortcut strings to one key name:

```json
{
  "enabled": true,
  "startup": false,
  "remaps": {
    "ctrl+alt+q": "enter",
    "shift+capslock": "backspace"
  }
}
```

Supported modifiers are `ctrl`, `alt`, `shift`, and `win`. Keys can be letters, digits, punctuation, or named keys such as `enter`, `backspace`, `f5`, `numpad3`, `up`, and `space`.
