//! Offline configuration store (`config.json`).
//!
//! The file lives next to the executable when Torch runs as a portable build and
//! falls back to the per-user app config directory otherwise, so a machine that
//! is never online still keeps every setting locally.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Auto,
    Dark,
    Light,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PopupAnchor {
    Cursor,
    Center,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomTheme {
    pub accent: String,
    pub bg: String,
    pub text: String,
    pub highlight: String,
    /// Backdrop blur radius in pixels.
    pub blur: u32,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            accent: "#00aaff".into(),
            bg: "#1a1a1a".into(),
            text: "#ffffff".into(),
            highlight: "#ffffff".into(),
            blur: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub startup: bool,
    /// Seconds a trigger key must be held before the radial popup appears.
    pub hold_time: f64,
    pub theme: Theme,
    pub popup_anchor: PopupAnchor,
    /// When true a trigger key is forwarded the instant it goes down instead of
    /// on release. Typing feels completely untouched, but the character is
    /// already committed by the time the radial selector opens.
    pub instant_passthrough: bool,
    pub custom_theme: CustomTheme,
    /// Instant source chord -> replacement key, e.g. `ctrl+alt+q` -> `enter`.
    #[serde(default)]
    pub remaps: BTreeMap<String, String>,
    /// Trigger key -> keys offered by the radial selector.
    pub mapping: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        let mapping = BTreeMap::new();
        Self {
            enabled: true,
            startup: false,
            hold_time: 2.0,
            theme: Theme::Auto,
            popup_anchor: PopupAnchor::Cursor,
            instant_passthrough: false,
            custom_theme: CustomTheme::default(),
            remaps: BTreeMap::new(),
            mapping,
        }
    }
}

impl Config {
    /// Clamps user supplied values into the ranges the UI advertises.
    pub fn sanitize(&mut self) {
        self.hold_time = self.hold_time.clamp(0.2, 3.0);
        self.custom_theme.blur = self.custom_theme.blur.clamp(0, 60);
        self.mapping.retain(|trigger, targets| {
            targets.retain(|target| !target.trim().is_empty());
            !trigger.trim().is_empty() && !targets.is_empty()
        });
        self.remaps.retain(|source, target| {
            !source.trim().is_empty() && !target.trim().is_empty()
        });
    }

    pub fn hold_time_ms(&self) -> u64 {
        (self.hold_time * 1000.0).round() as u64
    }
}

/// Resolves the config path, preferring a `config.json` that sits beside the
/// executable (portable install) over the roaming app data copy.
pub fn resolve_path(fallback_dir: &Path) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let portable = dir.join(CONFIG_FILE);
            if portable.exists() {
                return portable;
            }
        }
    }
    fallback_dir.join(CONFIG_FILE)
}

pub fn load(path: &Path) -> Config {
    let mut config = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Config>(&raw).ok())
        .unwrap_or_default();
    config.sanitize();
    config
}

pub fn save(path: &Path, config: &Config) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

