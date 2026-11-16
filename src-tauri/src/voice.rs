//! Voice commands: press Ctrl+Alt+Space (or the dock's mic) and say
//! "open WhatsApp", "pause", "volume 40", "do not disturb on"...
//!
//! Recognition is Windows' own speech engine, on this PC, matching a fixed list
//! of phrases (your apps + Ledge's commands) — fast, accurate, and nothing is
//! sent anywhere. The microphone is on only while listening (a few seconds).

use crate::{appbar, config, controls, hub, shell, system, winwatch};
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use windows::Media::SpeechRecognition::{
    SpeechRecognitionConfidence, SpeechRecognitionListConstraint, SpeechRecognitionResultStatus, SpeechRecognizer,
};
use windows::core::HSTRING;

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Open(String, String), // (app name, AppsFolder id)
    Media(&'static str),
    Volume(Option<f32>, bool),
    VolumeStep(f32),
    Dnd(bool),
    Radio(&'static str, bool),
    Brightness(u8),
    Shell(&'static str),
    Panel(&'static str),
    Settings,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VoiceState {
    /// "listening", "done", "unknown", "error"
    pub state: String,
    pub heard: String,
    pub said: String,
}

fn publish(app: &AppHandle, state: &str, heard: &str, said: &str) {
    let _ = app.emit_to("voice", "voice", VoiceState { state: state.into(), heard: heard.into(), said: said.into() });
}

/// Spoken name for an app: "WhatsApp", "Google Chrome" -> also "Chrome".
fn spoken(name: &str) -> Vec<String> {
    let clean: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut out = vec![clean.clone()];
    for prefix in ["google ", "microsoft ", "windows "] {
        if let Some(rest) = clean.strip_prefix(prefix) {
            if rest.len() > 2 {
                out.push(rest.to_string());
            }
        }
    }
    out.retain(|s| !s.is_empty() && s.len() <= 40);
    out
}

/// Every phrase Ledge understands, and what it does.
pub fn phrases(apps: &[shell::AppEntry]) -> Vec<(String, Action)> {
    let mut v: Vec<(String, Action)> = Vec::new();
    let mut add = |p: &str, a: Action| v.push((p.to_string(), a));
    for (p, a) in [
        ("pause", "toggle"),
        ("play", "toggle"),
        ("pause music", "toggle"),
        ("play music", "toggle"),
        ("resume", "toggle"),
        ("next", "next"),
        ("next song", "next"),
        ("skip", "next"),
        ("skip song", "next"),
        ("previous", "previous"),
        ("previous song", "previous"),
        ("go back", "previous"),
        ("shuffle", "shuffle"),
        ("repeat", "repeat"),
    ] {
        add(p, Action::Media(a));
    }
    add("mute", Action::Volume(None, true));
    add("unmute", Action::Volume(None, true));
    add("volume up", Action::VolumeStep(0.1));
    add("louder", Action::VolumeStep(0.1));
    add("volume down", Action::VolumeStep(-0.1));
    add("quieter", Action::VolumeStep(-0.1));
    for n in (0..=100).step_by(10) {
        add(&format!("volume {n}"), Action::Volume(Some(n as f32 / 100.0), false));
        add(&format!("set volume to {n}"), Action::Volume(Some(n as f32 / 100.0), false));
        add(&format!("brightness {n}"), Action::Brightness(n as u8));
    }
    for (p, on) in [
        ("do not disturb on", true),
        ("turn on do not disturb", true),
        ("focus on", true),
        ("do not disturb off", false),
        ("turn off do not disturb", false),
        ("focus off", false),
    ] {
        add(p, Action::Dnd(on));
    }
    for (radio, word) in [("wifi", "wifi"), ("wifi", "wi fi"), ("bluetooth", "bluetooth")] {
        add(&format!("{word} on"), Action::Radio(radio, true));
        add(&format!("turn on {word}"), Action::Radio(radio, true));
        add(&format!("{word} off"), Action::Radio(radio, false));
        add(&format!("turn off {word}"), Action::Radio(radio, false));
    }
    add("show desktop", Action::Shell("desktop"));
    add("task manager", Action::Shell("taskManager"));
    add("open task manager", Action::Shell("taskManager"));
    add("search", Action::Panel("launcher"));
    add("show my calendar", Action::Panel("calendar"));
    add("open calendar", Action::Panel("calendar"));
    add("show calendar", Action::Panel("calendar"));
    add("claude usage", Action::Panel("claude"));
    add("show claude usage", Action::Panel("claude"));
    add("show notifications", Action::Panel("center"));
    add("control center", Action::Panel("control"));
    add("open ledge settings", Action::Settings);
    // Commands win over an app with the same spoken name ("open calendar").
    let mut seen: std::collections::HashSet<String> = v.iter().map(|(p, _)| p.clone()).collect();
    for a in apps {
        for name in spoken(&a.name) {
            for verb in ["open", "launch", "start", "switch to"] {
                let p = format!("{verb} {name}");
                if seen.insert(p.clone()) {
                    v.push((p, Action::Open(a.name.clone(), a.id.clone())));
                }
            }
        }
    }
    v
}

fn run(app: &AppHandle, action: &Action) -> String {
    match action {
        Action::Open(name, id) => {
            if !winwatch::focus_app(id) {
                let _ = shell::run({
                    let target = format!("shell:AppsFolder\\{id}");
                    move || shell::launch(&target)
                });
            }
            format!("Opening {name}")
        }
        Action::Media(a) => {
            crate::media::control(a, None);
            match *a {
                "next" => "Next song".into(),
                "previous" => "Previous song".into(),
                "shuffle" => "Shuffle".into(),
                "repeat" => "Repeat".into(),
                _ => "Play / pause".into(),
            }
        }
        Action::Volume(level, mute) => {
            let _ = system::set_volume(*level, *mute);
            match level {
                Some(l) => format!("Volume {}", (l * 100.0).round()),
                None => "Mute toggled".into(),
            }
        }
        Action::VolumeStep(d) => {
            let now = system::volume().map(|v| v.level).unwrap_or(0.5);
            let next = (now + d).clamp(0.0, 1.0);
            let _ = system::set_volume(Some(next), false);
            format!("Volume {}", (next * 100.0).round())
        }
        Action::Dnd(on) => {
            if let Ok(cfg) = config::update(|c| c.dnd = *on) {
                let _ = app.emit("config", &cfg);
            }
            if *on { "Do not disturb on".into() } else { "Do not disturb off".into() }
        }
        Action::Radio(kind, on) => match controls::set_radio(kind, *on) {
            Ok(()) => format!("{} {}", if *kind == "wifi" { "Wi-Fi" } else { "Bluetooth" }, if *on { "on" } else { "off" }),
            Err(e) => e,
        },
        Action::Brightness(p) => match controls::set_brightness(*p) {
            Ok(()) => format!("Brightness {p}"),
            Err(e) => e,
        },
        Action::Shell(a) => {
            shell::shell_action(a);
            if *a == "desktop" { "Showing the desktop".into() } else { "Task Manager".into() }
        }
        Action::Panel(kind) => {
            let (w, h, label) = match *kind {
                "calendar" => (1000.0, 660.0, "Your calendar"),
                "claude" => (340.0, 420.0, "Claude usage"),
                "center" => (384.0, 640.0, "Notifications"),
                "control" => (360.0, 520.0, "Control Center"),
                _ => (520.0, 560.0, "Search"),
            };
            let g = appbar::geometry();
            let _ = crate::flyout::open(app, kind.to_string(), None, if *kind == "calendar" { -1.0 } else { g.width / 2.0 }, w, h);
            label.into()
        }
        Action::Settings => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::open_settings_window(&app).await;
            });
            "Opening Ledge settings".into()
        }
    }
}

// ---------- one recognizer for everything ----------
//
// A single continuous recognizer hears both "Hey Ledge …" (any time) and plain
// commands. Plain commands only count while Ledge is "armed": for a few
// seconds after the mic button, Ctrl+Alt+Space, or "Hey Ledge" on its own.
// With "Hey Ledge" turned off, the recognizer runs only while armed.

const WAKE: [&str; 3] = ["hey ledge", "ok ledge", "hi ledge"];
const ARM_FOR: Duration = Duration::from_secs(7);

static REC: Mutex<Option<SpeechRecognizer>> = Mutex::new(None);
static LIST: Mutex<Vec<(String, Action)>> = Mutex::new(Vec::new());
static ARMED: Mutex<Option<Instant>> = Mutex::new(None);
static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Splits "hey ledge open whatsapp" into "open whatsapp"; None if it doesn't start with a wake phrase.
pub fn after_wake(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase();
    WAKE.iter().find_map(|w| t.strip_prefix(w)).map(|rest| rest.trim_start_matches([',', ' ']).trim().to_string())
}

fn text_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn armed() -> bool {
    ARMED.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < ARM_FOR)
}

fn disarm() {
    *ARMED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Listen for a plain command for a few seconds, with the pill showing.
fn arm(app: &AppHandle) {
    let at = Instant::now();
    *ARMED.lock().unwrap_or_else(|e| e.into_inner()) = Some(at);
    show(app, true);
    publish(app, "listening", "", "Listening…");
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ARM_FOR);
        let still = *ARMED.lock().unwrap_or_else(|e| e.into_inner()) == Some(at);
        if still {
            disarm();
            publish(&app, "unknown", "", "Didn't hear a command — try “open WhatsApp” or “pause”");
            std::thread::sleep(Duration::from_millis(1600));
            if !armed() {
                show(&app, false);
                if !config::get().wake_word {
                    stop();
                }
            }
        }
    });
}

/// Runs a recognised command and shows what happened.
fn execute(app: &AppHandle, cmd: &str) {
    disarm();
    let action = LIST.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p.eq_ignore_ascii_case(cmd)).map(|(_, a)| a.clone());
    show(app, true);
    match action {
        Some(a) => {
            let said = run(app, &a);
            publish(app, "done", &text_case(cmd), &said);
        }
        None => publish(app, "unknown", &text_case(cmd), "Didn't catch that"),
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1800));
        if !armed() {
            show(&app, false);
            if !config::get().wake_word {
                stop();
            }
        }
    });
}

fn on_result(app: &AppHandle, text: &str, conf: SpeechRecognitionConfidence) {
    if conf == SpeechRecognitionConfidence::Rejected || text.trim().is_empty() {
        return;
    }
    if let Some(rest) = after_wake(text) {
        if rest.is_empty() {
            arm(app);
        } else {
            execute(app, &rest);
        }
    } else if armed() && conf != SpeechRecognitionConfidence::Low {
        execute(app, text);
    }
}

fn stop() {
    GEN.fetch_add(1, Ordering::SeqCst);
    let r = REC.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(r) = r {
        if let Ok(s) = r.ContinuousRecognitionSession() {
            let _ = s.StopAsync().and_then(|op| op.join());
        }
        let _ = r.Close();
    }
}

fn running() -> bool {
    REC.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Starts the recognizer if it isn't running. Takes a second or two the first time.
fn ensure_running(app: &AppHandle) -> Result<(), String> {
    use windows::Foundation::TypedEventHandler;
    use windows::Media::SpeechRecognition::{SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession};
    if running() {
        return Ok(());
    }
    let seq = GEN.load(Ordering::SeqCst);
    let list = phrases(&shell::apps_cached());
    let mut words: Vec<HSTRING> = WAKE.iter().map(|w| HSTRING::from(*w)).collect();
    for (p, _) in &list {
        words.push(HSTRING::from(p.as_str()));
        for w in WAKE {
            words.push(HSTRING::from(format!("{w} {p}")));
        }
    }
    *LIST.lock().unwrap_or_else(|e| e.into_inner()) = list;
    let r = SpeechRecognizer::new()
        .map_err(|_| "Windows speech recognition isn't available (add an English speech pack in Settings → Time & language → Speech)".to_string())?;
    let iter: windows_collections::IIterable<HSTRING> = words.into();
    let c = SpeechRecognitionListConstraint::Create(&iter).map_err(|e| e.message().to_string())?;
    r.Constraints().and_then(|cs| cs.Append(&c)).map_err(|e| e.message().to_string())?;
    let compiled = r.CompileConstraintsAsync().and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    if compiled.Status().ok() != Some(SpeechRecognitionResultStatus::Success) {
        return Err("Speech recognition couldn't start — check the microphone is allowed for desktop apps".into());
    }
    let session = r.ContinuousRecognitionSession().map_err(|e| e.message().to_string())?;
    let _ = session.SetAutoStopSilenceTimeout(windows::Foundation::TimeSpan { Duration: 24 * 3600 * 10_000_000 });
    let a = app.clone();
    session
        .ResultGenerated(&TypedEventHandler::<SpeechContinuousRecognitionSession, SpeechContinuousRecognitionResultGeneratedEventArgs>::new(
            move |_, args| {
                let Ok(args) = args.ok() else { return Ok(()) };
                let res = args.Result()?;
                let text = res.Text()?.to_string();
                let conf = res.Confidence()?;
                let app = a.clone();
                std::thread::spawn(move || on_result(&app, &text, conf));
                Ok(())
            },
        ))
        .map_err(|e| e.message().to_string())?;
    // The session can end on its own (sleep, mic change): start again if still wanted.
    let again = app.clone();
    session
        .Completed(&TypedEventHandler::new(move |_, _| {
            let app = again.clone();
            if GEN.load(Ordering::SeqCst) == seq {
                std::thread::spawn(move || {
                    *REC.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    std::thread::sleep(Duration::from_secs(2));
                    if GEN.load(Ordering::SeqCst) == seq && (config::get().wake_word || armed()) {
                        com_init();
                        let _ = ensure_running(&app);
                    }
                });
            }
            Ok(())
        }))
        .map_err(|e| e.message().to_string())?;
    session.StartAsync().and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    *REC.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
    Ok(())
}

fn com_init() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
}

/// Mic button / Ctrl+Alt+Space: listen for one command.
pub fn listen(app: &AppHandle) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        com_init();
        show(&app, true);
        publish(&app, "listening", "", "Listening…");
        match ensure_running(&app) {
            Ok(()) => arm(&app),
            Err(e) => {
                publish(&app, "error", "", &e);
                std::thread::sleep(Duration::from_millis(2600));
                show(&app, false);
            }
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Turns the "Hey Ledge" listener on or off (Settings → General → Voice).
pub fn set_wake(app: &AppHandle, on: bool) {
    if !on {
        if !armed() {
            stop();
        }
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        com_init();
        let r = ensure_running(&app);
        hub::publish(&app, "voice-wake", &serde_json::json!({ "on": r.is_ok(), "error": r.err() }));
    });
}

/// The listening pill, centred above the dock.
fn show(app: &AppHandle, on: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow};
    let Some(w) = app.get_webview_window("voice") else { return };
    let Ok(h) = w.hwnd() else { return };
    let h = HWND(h.0);
    unsafe {
        if !on {
            let _ = ShowWindow(h, SW_HIDE);
            return;
        }
        let g = appbar::geometry();
        let s = g.scale;
        // Room around the orb so its glow never meets the window edge.
        let (pw, ph) = ((460.0 * s) as i32, (320.0 * s) as i32);
        let x = (g.mon_left + g.mon_right) / 2 - pw / 2;
        let y = g.dock_top - (4.0 * s) as i32 - ph;
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, pw, ph, SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_phrase_splits_off_the_command() {
        assert_eq!(after_wake("Hey Ledge open WhatsApp").as_deref(), Some("open whatsapp"));
        assert_eq!(after_wake("hey ledge").as_deref(), Some(""));
        assert_eq!(after_wake("ok ledge, pause").as_deref(), Some("pause"));
        assert_eq!(after_wake("open whatsapp"), None);
    }

    #[test]
    fn builds_phrases_for_apps_and_commands() {
        let apps = vec![
            shell::AppEntry { name: "WhatsApp".into(), id: "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into() },
            shell::AppEntry { name: "Google Chrome".into(), id: "Chrome".into() },
        ];
        let p = phrases(&apps);
        let find = |s: &str| p.iter().find(|(x, _)| x == s).map(|(_, a)| a.clone());
        assert!(matches!(find("open whatsapp"), Some(Action::Open(n, _)) if n == "WhatsApp"));
        assert!(matches!(find("open chrome"), Some(Action::Open(n, _)) if n == "Google Chrome"));
        assert!(matches!(find("switch to google chrome"), Some(Action::Open(..))));
        assert_eq!(find("volume 40"), Some(Action::Volume(Some(0.4), false)));
        assert_eq!(find("turn on do not disturb"), Some(Action::Dnd(true)));
        assert_eq!(find("next song"), Some(Action::Media("next")));
        // Every phrase is unique, so a match is never ambiguous.
        let mut all: Vec<_> = p.iter().map(|(x, _)| x.clone()).collect();
        all.sort();
        let n = all.len();
        all.dedup();
        assert_eq!(n, all.len());
    }
}
