//! User settings in %APPDATA%\Ledge\config.json.
//! A file that fails to parse is moved aside (config.json.bad), never overwritten.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Pin {
    pub name: String,
    /// What to launch: a .lnk, an .exe, or `shell:AppsFolder\<AUMID>`.
    pub path: String,
    /// AppUserModelID, when known — matches running windows of the same app.
    pub aumid: Option<String>,
    /// Resolved executable, when known — the fallback match for running windows.
    pub target: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Widgets {
    pub ai: bool,
    pub system: bool,
    pub media: bool,
    pub calendar: bool,
    pub weather: bool,
    pub whatsapp: bool,
    pub portfolio: bool,
    /// Ledge's own notification banners.
    pub banners: bool,
    pub phone: bool,
    /// The battery ring (separate from CPU/RAM so either can be hidden).
    pub battery: bool,
}

impl Default for Widgets {
    fn default() -> Self {
        Self { ai: true, system: true, media: true, calendar: true, weather: true, whatsapp: true, portfolio: true, banners: true, phone: true, battery: true }
    }
}

/// How the dock's glass looks.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// "black", "white", "liquid", "ios", "midnight" or "custom".
    pub style: String,
    /// How much the tint covers the frosted wallpaper, 0 (clear) to 1 (solid).
    pub opacity: f64,
    /// Tint colour, "#rrggbb".
    pub tint: String,
    /// Frost strength in px.
    pub blur: u32,
    /// Accent for bars and highlights, "#rrggbb", or "" for the default.
    pub accent: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { style: "black".into(), opacity: 0.55, tint: "#16161c".into(), blur: 28, accent: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// None until first run copies the user's Windows taskbar pins.
    pub pins: Option<Vec<Pin>>,
    pub city: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub place_label: String,
    pub fahrenheit: bool,
    pub clock24: bool,
    /// Secret iCal addresses (Google/Outlook/iCloud "secret address in iCal format").
    pub ics_urls: Vec<String>,
    pub widgets: Widgets,
    pub hide_native_taskbar: bool,
    /// Run Ledge at sign-in (release builds only).
    pub start_with_windows: bool,
    /// Opt-in: ask Anthropic for the real plan limits with Claude Code's sign-in.
    pub claude_live_limits: bool,
    /// Slide the dock away until the pointer touches the bottom edge.
    pub dock_autohide: bool,
    /// Dock items left to right (widget ids and "apps"); unknown/missing ids are fixed up by the UI.
    pub dock_order: Vec<String>,
    /// Bumped when defaults change, so older files can be migrated once.
    /// A file without it is from before versioning: 0, not today's version.
    #[serde(default)]
    pub config_version: u32,
    /// RennSquare's data\dashboard\live.json.
    pub portfolio_path: String,
    /// Do not disturb: notifications are collected but no banners slide in.
    pub dnd: bool,
    /// How long a banner stays, in seconds (1-30).
    pub banner_seconds: u32,
    /// Read the exact plan usage from claude.ai, signed in inside Ledge.
    pub claude_web: bool,
    pub appearance: Appearance,
    /// Background-app (tray) icons, left to right, by executable path.
    pub tray_order: Vec<String>,
}

fn default_portfolio() -> String {
    dirs::home_dir()
        .map(|h| h.join("RennSquare Investments").join("data").join("dashboard").join("live.json"))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pins: None,
            city: String::new(),
            latitude: None,
            longitude: None,
            place_label: String::new(),
            fahrenheit: false,
            clock24: false,
            ics_urls: Vec::new(),
            widgets: Widgets::default(),
            hide_native_taskbar: true,
            start_with_windows: true,
            claude_live_limits: false,
            portfolio_path: default_portfolio(),
            dock_autohide: true,
            dock_order: Vec::new(),
            config_version: CURRENT_VERSION,
            dnd: false,
            banner_seconds: 4,
            claude_web: false,
            appearance: Appearance::default(),
            tray_order: Vec::new(),
        }
    }
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

const CURRENT_VERSION: u32 = 2;

/// One-time upgrades for files written by older versions.
pub fn migrate(c: &mut Config) -> bool {
    if c.config_version >= CURRENT_VERSION {
        return false;
    }
    if c.config_version < 2 {
        // v0.3: auto-hide dock and live Claude limits became the defaults the user asked for.
        c.dock_autohide = true;
        c.claude_live_limits = true;
    }
    c.config_version = CURRENT_VERSION;
    true
}
/// Bumped on every save so background loops know to refresh early.
pub static VERSION: AtomicU64 = AtomicU64::new(0);

pub fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Ledge")
}

fn path() -> PathBuf {
    dir().join("config.json")
}

pub fn load() -> Config {
    let p = path();
    let cfg = match std::fs::read_to_string(&p) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(mut c) => {
                if migrate(&mut c) {
                    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(c.clone());
                    let _ = save(&c);
                }
                c
            }
            Err(_) => {
                let _ = std::fs::rename(&p, p.with_extension("json.bad"));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg.clone());
    cfg
}

pub fn get() -> Config {
    let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(c) => c.clone(),
        None => {
            drop(guard);
            load()
        }
    }
}

/// Writes atomically (temp file + rename) so a crash never leaves half a file.
pub fn save(cfg: &Config) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let tmp = path().with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path()).map_err(|e| e.to_string())?;
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg.clone());
    VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

pub fn update(f: impl FnOnce(&mut Config)) -> Result<Config, String> {
    let mut c = get();
    f(&mut c);
    save(&c)?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let c: Config = serde_json::from_str(r#"{"city":"Pune"}"#).unwrap();
        assert_eq!(c.city, "Pune");
        assert!(c.hide_native_taskbar);
        assert!(c.widgets.ai && c.widgets.weather);
        assert!(c.pins.is_none());
        // A file from before versioning migrates once to the new defaults.
        let mut old: Config = serde_json::from_str(r#"{"claudeLiveLimits":false,"dockAutohide":false}"#).unwrap();
        assert!(migrate(&mut old));
        assert!(old.claude_live_limits && old.dock_autohide && old.config_version == CURRENT_VERSION);
        old.dock_autohide = false;
        assert!(!migrate(&mut old), "only once");
        assert!(!old.dock_autohide, "later choices stick");
    }
}
