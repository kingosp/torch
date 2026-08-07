//! "Launch at login" support through the Windows Run registry key.

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "Torch";

#[cfg(windows)]
pub fn set(enabled: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu
        .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)
        .map_err(|e| e.to_string())?;

    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = format!("\"{}\"", exe.display());
        run.set_value(VALUE_NAME, &command)
            .map_err(|e| e.to_string())
    } else {
        match run.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(windows))]
pub fn set(_enabled: bool) -> Result<(), String> {
    Err("startup integration is only available on Windows".into())
}
