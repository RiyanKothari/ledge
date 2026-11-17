//! Voice: say "Hey Ledge, what's the weather?" (or press the hotkey / the dock's
//! mic and just ask). Ledge answers questions out loud — time, date, weather,
//! battery, what's playing, your next event, Claude usage, timers — and runs
//! commands: "open WhatsApp", "pause", "volume forty", "Wi-Fi off"...
//!
//! Recognition is Windows' own speech engine, on this PC, matching a small
//! grammar (wake word + commands + "open <app>") in the language that fits your
//! region (Indian English here, if installed). Nothing is sent anywhere.
//! Every result is written to `%APPDATA%\Ledge\voice.log` for troubleshooting.

use crate::{appbar, claude_web, config, controls, hub, shell, system, winwatch};
use chrono::{Datelike, Local, TimeZone};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use windows::Media::SpeechRecognition::{
    SpeechRecognitionConfidence, SpeechRecognitionGrammarFileConstraint, SpeechRecognitionListConstraint,
    SpeechRecognitionResultStatus, SpeechRecognizer,
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
    /// A question Ledge answers: "time", "date", "weather", "battery", "playing",
    /// "calendar", "claude", "timer", "help", "thanks", "name".
    Ask(&'static str),
    Timer(u32),
    CancelTimer,
    Lock,
    Screenshot,
    Cancel,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoiceState {
    /// "listening", "done", "unknown", "error"
    pub state: String,
    pub heard: String,
    pub said: String,
    pub detail: String,
    /// mic, clock, calendar, weather, battery, music, timer, check, help, lock, claude
    pub icon: String,
}

/// What Ledge shows (and maybe says) after a command.
#[derive(Default, Debug, PartialEq)]
pub struct Reply {
    pub said: String,
    pub detail: String,
    pub icon: &'static str,
    /// Spoken out loud (when "Speak answers" is on).
    pub speak: Option<String>,
}

fn reply(said: impl Into<String>, icon: &'static str) -> Reply {
    Reply { said: said.into(), icon, ..Default::default() }
}

fn answer(said: impl Into<String>, detail: impl Into<String>, icon: &'static str, speak: impl Into<String>) -> Reply {
    Reply { said: said.into(), detail: detail.into(), icon, speak: Some(speak.into()) }
}

/// Bumped whenever the card shows something new, so an older "hide" can't close it.
static CARD: AtomicU64 = AtomicU64::new(0);

fn publish(app: &AppHandle, state: &str, heard: &str, r: &Reply) {
    CARD.fetch_add(1, Ordering::SeqCst);
    let _ = app.emit_to(
        "voice",
        "voice",
        VoiceState { state: state.into(), heard: heard.into(), said: r.said.clone(), detail: r.detail.clone(), icon: r.icon.into() },
    );
}

// ---------- words ----------

/// 0-100 in words ("forty five"). Larger numbers stay as digits.
pub fn num_words(n: u32) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen",
        "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    match n {
        0..=19 => ONES[n as usize].into(),
        100 => "one hundred".into(),
        20..=99 if n % 10 == 0 => TENS[(n / 10) as usize].into(),
        20..=99 => format!("{} {}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]),
        _ => n.to_string(),
    }
}

/// One spelling for every way the engine might write a phrase: lower case, no
/// punctuation or apostrophes, numbers as words ("Wi-Fi 40" -> "wi fi forty").
pub fn canon(s: &str) -> String {
    s.to_lowercase()
        .replace(['\'', '’'], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.parse::<u32>().map(num_words).unwrap_or_else(|_| w.to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Spoken name for an app: "WhatsApp", "Google Chrome" -> also "Chrome".
fn spoken(name: &str) -> Vec<String> {
    let clean = canon(name);
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

const VERBS: [&str; 4] = ["open", "launch", "start", "switch to"];
/// Wake phrases, including what "Hey Ledge" often sounds like to the engine.
const WAKE: [&str; 11] =
    ["hey ledge", "hi ledge", "hello ledge", "ok ledge", "okay ledge", "hey ledger", "hey lege", "hey lodge", "a ledge", "hey ledges", "ledge"];

/// Ledge's own commands and questions (not "open <app>").
pub fn commands() -> Vec<(String, Action)> {
    let mut v: Vec<(String, Action)> = Vec::new();
    let mut add = |ps: &[&str], a: Action| {
        for p in ps {
            v.push((p.to_string(), a.clone()));
        }
    };
    // Questions
    add(&["what time is it", "what's the time", "what is the time", "tell me the time", "time", "the time"], Action::Ask("time"));
    add(
        &["what's the date", "what is the date", "what day is it", "what's today's date", "what is today", "today's date", "date"],
        Action::Ask("date"),
    );
    add(
        &[
            "what's the weather",
            "what is the weather",
            "what's the weather like",
            "what's the weather today",
            "how's the weather",
            "how is the weather",
            "weather",
            "what's the temperature",
            "temperature",
            "how hot is it",
            "how cold is it",
            "is it going to rain",
        ],
        Action::Ask("weather"),
    );
    add(
        &["battery", "battery level", "how much battery", "how much battery is left", "what's my battery", "what's the battery"],
        Action::Ask("battery"),
    );
    add(
        &["what's playing", "what is playing", "what song is this", "what's this song", "who sings this", "what am i listening to"],
        Action::Ask("playing"),
    );
    add(
        &[
            "what's next",
            "what's on my calendar",
            "what's my next meeting",
            "next meeting",
            "next event",
            "do i have any meetings",
            "do i have any meetings today",
            "what's on today",
            "what's my schedule",
        ],
        Action::Ask("calendar"),
    );
    add(
        &["claude usage", "what's my claude usage", "how much claude is left", "how much claude do i have left", "claude limits"],
        Action::Ask("claude"),
    );
    add(&["how much time is left", "how long is left on the timer", "timer"], Action::Ask("timer"));
    add(&["help", "what can you do", "what can i say"], Action::Ask("help"));
    add(&["thank you", "thanks"], Action::Ask("thanks"));
    add(&["who are you", "what's your name"], Action::Ask("name"));
    // Timers
    for m in [1u32, 2, 3, 4, 5, 10, 15, 20, 25, 30, 40, 45] {
        let unit = if m == 1 { "minute" } else { "minutes" };
        let n = num_words(m);
        add(&[&format!("set a timer for {n} {unit}"), &format!("timer for {n} {unit}"), &format!("{n} minute timer")], Action::Timer(m * 60));
    }
    for s in [10u32, 15, 20, 30, 45] {
        let n = num_words(s);
        add(&[&format!("set a timer for {n} seconds"), &format!("timer for {n} seconds")], Action::Timer(s));
    }
    add(&["set a timer for half an hour", "timer for half an hour"], Action::Timer(1800));
    add(&["set a timer for an hour", "set a timer for one hour", "timer for an hour", "one hour timer"], Action::Timer(3600));
    add(&["cancel the timer", "stop the timer", "cancel timer"], Action::CancelTimer);
    // Music
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
        add(&[p], Action::Media(a));
    }
    // Sound, screen, radios
    add(&["mute", "unmute"], Action::Volume(None, true));
    add(&["volume up", "louder", "turn it up"], Action::VolumeStep(0.1));
    add(&["volume down", "quieter", "turn it down"], Action::VolumeStep(-0.1));
    for n in (0..=100).step_by(10) {
        let w = num_words(n);
        add(&[&format!("volume {w}"), &format!("set volume to {w}"), &format!("set the volume to {w}")], Action::Volume(Some(n as f32 / 100.0), false));
        add(&[&format!("brightness {w}"), &format!("set brightness to {w}")], Action::Brightness(n as u8));
    }
    add(&["do not disturb on", "turn on do not disturb", "focus on"], Action::Dnd(true));
    add(&["do not disturb off", "turn off do not disturb", "focus off"], Action::Dnd(false));
    for (radio, word) in [("wifi", "wifi"), ("wifi", "wi fi"), ("bluetooth", "bluetooth")] {
        add(&[&format!("{word} on"), &format!("turn on {word}")], Action::Radio(radio, true));
        add(&[&format!("{word} off"), &format!("turn off {word}")], Action::Radio(radio, false));
    }
    // Windows and Ledge
    add(&["show desktop", "show the desktop"], Action::Shell("desktop"));
    add(&["task manager", "open task manager"], Action::Shell("taskManager"));
    add(&["lock", "lock the computer", "lock my pc", "lock the pc", "lock screen"], Action::Lock);
    add(&["take a screenshot", "screenshot"], Action::Screenshot);
    add(&["search"], Action::Panel("launcher"));
    add(&["show my calendar", "open calendar", "show calendar"], Action::Panel("calendar"));
    add(&["show claude usage"], Action::Panel("claude"));
    add(&["show notifications"], Action::Panel("center"));
    add(&["control center"], Action::Panel("control"));
    add(&["open ledge settings"], Action::Settings);
    add(&["cancel", "stop", "never mind", "nothing"], Action::Cancel);
    // One spelling per phrase; the first meaning wins.
    let mut seen = HashSet::new();
    v.retain(|(p, _)| seen.insert(canon(p)));
    v
}

/// Every phrase Ledge understands (commands, then "open <app>"), keyed by `canon`.
pub fn phrases(apps: &[shell::AppEntry]) -> Vec<(String, Action)> {
    let mut v = commands();
    // Commands win over an app with the same spoken name ("open calendar").
    let mut seen: HashSet<String> = v.iter().map(|(p, _)| canon(p)).collect();
    for a in apps {
        for name in spoken(&a.name) {
            for verb in VERBS {
                let p = format!("{verb} {name}");
                if seen.insert(canon(&p)) {
                    v.push((p, Action::Open(a.name.clone(), a.id.clone())));
                }
            }
        }
    }
    v
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An SRGS grammar: [wake word] then a command, or a command on its own. Much
/// smaller than listing every combination, so recognition is faster and surer.
pub fn grammar(lang: &str, apps: &[shell::AppEntry]) -> String {
    let item = |p: &str| format!("<item>{}</item>", xml_escape(p));
    let wake: String = WAKE.iter().map(|w| item(w)).collect();
    let mut cmds: Vec<String> = commands().iter().map(|(p, _)| item(p)).collect();
    let mut names: Vec<String> = apps.iter().flat_map(|a| spoken(&a.name)).collect();
    names.sort();
    names.dedup();
    let app_rule = if names.is_empty() {
        String::new()
    } else {
        cmds.push(r##"<item><ruleref uri="#verb"/><ruleref uri="#app"/></item>"##.into());
        format!(
            r#"<rule id="verb"><one-of>{}</one-of></rule><rule id="app"><one-of>{}</one-of></rule>"#,
            VERBS.iter().map(|v| item(v)).collect::<String>(),
            names.iter().map(|n| item(n)).collect::<String>()
        )
    };
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<grammar version="1.0" xml:lang="{lang}" root="top" xmlns="http://www.w3.org/2001/06/grammar">
<rule id="top" scope="public"><one-of>
<item><item repeat="0-1"><ruleref special="GARBAGE"/></item><ruleref uri="#wake"/><item repeat="0-1"><ruleref uri="#cmd"/></item></item>
<item><ruleref uri="#cmd"/></item>
</one-of></rule>
<rule id="wake"><one-of>{wake}</one-of></rule>
<rule id="cmd"><one-of>{}</one-of></rule>
{app_rule}
</grammar>"##,
        cmds.join("")
    )
}

// ---------- answers ----------

fn clock(ms: i64) -> String {
    let Some(t) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    if config::get().clock24 { t.format("%H:%M").to_string() } else { t.format("%-I:%M %p").to_string() }
}

fn day_name(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|t| t.format("%A, %-d %B").to_string()).unwrap_or_default()
}

fn duration_words(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let part = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    match (h, m) {
        (0, 0) => part(s, "second"),
        (0, _) if s >= 30 && m < 5 => format!("{} and {}", part(m, "minute"), part(s, "second")),
        (0, _) => part(m, "minute"),
        (_, 0) => part(h, "hour"),
        _ => format!("{} {}", part(h, "hour"), part(m, "minute")),
    }
}

/// (shown, spoken) for an Open-Meteo weather code.
fn weather_words(code: i64) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", "clear"),
        1 => ("Mostly clear", "mostly clear"),
        2 => ("Partly cloudy", "partly cloudy"),
        3 => ("Cloudy", "cloudy"),
        45 | 48 => ("Foggy", "foggy"),
        51..=57 => ("Drizzle", "drizzling"),
        61..=67 => ("Rain", "raining"),
        71..=77 => ("Snow", "snowing"),
        80..=82 => ("Showers", "showery"),
        85 | 86 => ("Snow showers", "snowy"),
        95..=99 => ("Thunderstorms", "stormy"),
        _ => ("", ""),
    }
}

fn round(v: &Value) -> Option<i64> {
    v.as_f64().map(|f| f.round() as i64)
}

pub fn weather_reply(w: Option<&Value>) -> Reply {
    let Some(w) = w.filter(|w| w["configured"].as_bool() == Some(true)) else {
        return answer("Weather isn't set up", "Add your city in Ledge settings", "weather", "Add your city in Ledge settings and I'll tell you the weather.");
    };
    let Some(t) = round(&w["temp"]) else {
        return answer("Weather isn't available", w["error"].as_str().unwrap_or("Try again in a minute"), "weather", "I couldn't get the weather right now.");
    };
    let place = w["place"].as_str().unwrap_or("");
    let (shown, said) = weather_words(w["code"].as_i64().unwrap_or(-1));
    let mut detail = place.to_string();
    let mut speak = if said.is_empty() { format!("It's {t} degrees") } else { format!("It's {t} degrees and {said}") };
    if !place.is_empty() {
        speak += &format!(" in {place}");
    }
    if let (Some(h), Some(l)) = (round(&w["high"]), round(&w["low"])) {
        detail += &format!("{}High {h}°  Low {l}°", if place.is_empty() { "" } else { " · " });
        speak += &format!(", with a high of {h} and a low of {l}");
    }
    let said_line = if shown.is_empty() { format!("{t}°") } else { format!("{t}° · {shown}") };
    answer(said_line, detail, "weather", speak + ".")
}

pub fn battery_reply(sys: Option<&Value>) -> Reply {
    let b = sys.map(|s| &s["battery"]).filter(|b| b.is_object());
    let Some(b) = b else { return answer("No battery", "This PC runs on mains power", "battery", "This PC doesn't have a battery.") };
    let p = b["percent"].as_u64().unwrap_or(0);
    let charging = b["charging"].as_bool() == Some(true);
    answer(
        format!("{p}%"),
        if charging { "Charging" } else { "On battery" },
        "battery",
        format!("The battery is at {p} percent{}.", if charging { " and charging" } else { "" }),
    )
}

pub fn playing_reply(m: Option<&Value>) -> Reply {
    let title = m.and_then(|m| m["title"].as_str()).unwrap_or("").trim().to_string();
    if title.is_empty() {
        return answer("Nothing is playing", "", "music", "Nothing is playing right now.");
    }
    let m = m.unwrap_or(&Value::Null);
    let artist = m["artist"].as_str().unwrap_or("").trim();
    let playing = m["playing"].as_bool() == Some(true);
    let detail = if artist.is_empty() { String::new() } else { artist.to_string() };
    let by = if artist.is_empty() { String::new() } else { format!(" by {artist}") };
    let speak = if playing { format!("This is {title}{by}.") } else { format!("{title}{by} is paused.") };
    answer(title, detail, "music", speak)
}

pub fn calendar_reply(cal: Option<&Value>, now: i64) -> Reply {
    let events: Vec<&Value> = cal.and_then(|c| c["events"].as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    let today = Local.timestamp_millis_opt(now).single().map(|t| t.date_naive());
    let same_day = |ms: i64| Local.timestamp_millis_opt(ms).single().map(|t| t.date_naive()) == today;
    let next = events.iter().find(|e| e["allDay"].as_bool() != Some(true) && e["end"].as_i64().unwrap_or(0) > now);
    match next {
        Some(e) => {
            let title = e["title"].as_str().unwrap_or("Event").to_string();
            let start = e["start"].as_i64().unwrap_or(now);
            let when = if start <= now {
                "Happening now".to_string()
            } else if same_day(start) {
                let mins = ((start - now) / 60_000).max(1) as u64;
                if mins < 90 { format!("{} · in {}", clock(start), duration_words(mins * 60)) } else { clock(start) }
            } else {
                format!("{} · {}", day_name(start), clock(start))
            };
            let speak = if start <= now {
                format!("{title} is happening now.")
            } else if same_day(start) {
                format!("Next up is {title} at {}.", clock(start))
            } else {
                format!("Nothing else today. Next is {title} on {} at {}.", day_name(start), clock(start))
            };
            answer(title, when, "calendar", speak)
        }
        None => answer("Your calendar is clear", "Nothing coming up", "calendar", "Your calendar is clear."),
    }
}

pub fn claude_reply(five: Option<(f64, Option<i64>)>, week: Option<f64>) -> Reply {
    let Some((used, resets)) = five else {
        return answer("Claude isn't connected", "Connect it in Ledge settings", "claude", "Connect Claude in Ledge settings to see your usage.");
    };
    let used = used.round() as i64;
    let mut detail = format!("5-hour limit {used}% used");
    let mut speak = format!("You've used {used} percent of your five hour limit");
    if let Some(w) = week {
        let w = w.round() as i64;
        detail += &format!(" · week {w}%");
        speak += &format!(", and {w} percent of the week");
    }
    if let Some(r) = resets {
        speak += &format!(". It resets at {}", clock(r));
    }
    answer(format!("{}% left", (100 - used).max(0)), detail, "claude", speak + ".")
}

// ---------- timers ----------

static TIMER: Mutex<Option<(u64, Instant, u32)>> = Mutex::new(None); // (id, ends, seconds)
static TIMER_ID: AtomicU64 = AtomicU64::new(0);

fn start_timer(app: &AppHandle, secs: u32) -> Reply {
    let id = TIMER_ID.fetch_add(1, Ordering::SeqCst) + 1;
    let ends = Instant::now() + Duration::from_secs(secs as u64);
    *TIMER.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, ends, secs));
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs as u64));
        let mine = TIMER.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|(t, _, _)| t == id);
        if !mine {
            return;
        }
        *TIMER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        log(&format!("timer done ({secs}s)"));
        let r = answer("Time's up", format!("{} timer", duration_words(secs as u64)), "timer", format!("Your {} timer is done.", duration_words(secs as u64)));
        publish(&app, "done", "", &r);
        show(&app, true);
        sound("Alarms\\Alarm01.wav", false);
        if let Some(s) = &r.speak {
            say(s);
        }
        hide_later(&app, Duration::from_secs(4));
    });
    let end_ms = Local::now().timestamp_millis() + secs as i64 * 1000;
    let d = duration_words(secs as u64);
    answer(format!("Timer · {d}"), format!("Ends at {}", clock(end_ms)), "timer", format!("Timer set for {d}."))
}

fn timer_left() -> Reply {
    match *TIMER.lock().unwrap_or_else(|e| e.into_inner()) {
        Some((_, ends, _)) => {
            let left = ends.saturating_duration_since(Instant::now()).as_secs().max(1);
            let d = duration_words(left);
            answer(format!("{d} left"), "On your timer", "timer", format!("{d} left."))
        }
        None => answer("No timer running", "Say “set a timer for five minutes”", "timer", "There's no timer running."),
    }
}

// ---------- speaking ----------

static SPEAKING: AtomicBool = AtomicBool::new(false);

/// Plays a Windows sound (C:\Windows\Media\…).
fn sound(name: &str, wait: bool) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT, SND_SYNC};
    let dir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    let path = HSTRING::from(format!("{dir}\\Media\\{name}"));
    unsafe {
        let _ = PlaySoundW(&path, None, SND_FILENAME | SND_NODEFAULT | if wait { SND_SYNC } else { SND_ASYNC });
    }
}

fn synth(text: &str) -> windows::core::Result<Vec<u8>> {
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;
    let s = SpeechSynthesizer::new()?;
    // A touch quicker than default sounds less robotic.
    let _ = s.Options().and_then(|o| o.SetSpeakingRate(1.08));
    let stream = s.SynthesizeTextToStreamAsync(&HSTRING::from(text))?.join()?;
    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.join()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(buf)
}

/// Says `text` with the Windows voice (blocking). Ledge ignores what it hears
/// meanwhile, so it never answers itself.
fn say(text: &str) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    if !config::get().voice_replies {
        return;
    }
    com_init();
    match synth(text) {
        Ok(wav) => {
            SPEAKING.store(true, Ordering::SeqCst);
            unsafe {
                let _ = PlaySoundW(windows::core::PCWSTR(wav.as_ptr() as *const u16), None, SND_MEMORY | SND_SYNC | SND_NODEFAULT);
            }
            std::thread::sleep(Duration::from_millis(350));
            SPEAKING.store(false, Ordering::SeqCst);
        }
        Err(e) => log(&format!("speech synthesis failed: {}", e.message())),
    }
}

fn hush() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_FLAGS};
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR::null(), None, SND_FLAGS(0));
    }
}

// ---------- running a command ----------

fn run(app: &AppHandle, action: &Action) -> Reply {
    match action {
        Action::Open(name, id) => {
            if !winwatch::focus_app(id) {
                let id = id.clone();
                let _ = shell::run(move || shell::launch_app_id(&id));
            }
            reply(format!("Opening {name}"), "check")
        }
        Action::Media(a) => {
            crate::media::control(a, None);
            reply(
                match *a {
                    "next" => "Next song",
                    "previous" => "Previous song",
                    "shuffle" => "Shuffle",
                    "repeat" => "Repeat",
                    _ => "Play / pause",
                },
                "music",
            )
        }
        Action::Volume(level, mute) => {
            let v = system::set_volume(*level, *mute);
            match level {
                Some(l) => reply(format!("Volume {}", (l * 100.0).round()), "check"),
                None => reply(if v.is_some_and(|v| v.muted) { "Muted" } else { "Sound on" }, "check"),
            }
        }
        Action::VolumeStep(d) => {
            let now = system::volume().map(|v| v.level).unwrap_or(0.5);
            let next = (now + d).clamp(0.0, 1.0);
            let _ = system::set_volume(Some(next), false);
            reply(format!("Volume {}", (next * 100.0).round()), "check")
        }
        Action::Dnd(on) => {
            if let Ok(cfg) = config::update(|c| c.dnd = *on) {
                let _ = app.emit("config", &cfg);
            }
            reply(if *on { "Do not disturb is on" } else { "Do not disturb is off" }, "check")
        }
        Action::Radio(kind, on) => match controls::set_radio(kind, *on) {
            Ok(()) => reply(format!("{} {}", if *kind == "wifi" { "Wi-Fi" } else { "Bluetooth" }, if *on { "on" } else { "off" }), "check"),
            Err(e) => reply(e, "help"),
        },
        Action::Brightness(p) => match controls::set_brightness(*p) {
            Ok(()) => reply(format!("Brightness {p}"), "check"),
            Err(e) => reply(e, "help"),
        },
        Action::Shell(a) => {
            shell::shell_action(a);
            reply(if *a == "desktop" { "Showing the desktop" } else { "Task Manager" }, "check")
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
            reply(label, "check")
        }
        Action::Settings => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::open_settings_window(&app).await;
            });
            reply("Opening Ledge settings", "check")
        }
        Action::Ask(q) => ask(q),
        Action::Timer(s) => start_timer(app, *s),
        Action::CancelTimer => {
            let had = TIMER.lock().unwrap_or_else(|e| e.into_inner()).take().is_some();
            if had { answer("Timer cancelled", "", "timer", "Timer cancelled.") } else { timer_left() }
        }
        Action::Lock => {
            unsafe {
                let _ = windows::Win32::System::Shutdown::LockWorkStation();
            }
            reply("Locking", "lock")
        }
        Action::Screenshot => {
            let _ = shell::run(|| shell::launch("ms-screenclip:"));
            reply("Snip a screenshot", "check")
        }
        Action::Cancel => {
            hush();
            reply("", "mic")
        }
    }
}

fn ask(q: &str) -> Reply {
    let now = Local::now();
    let ms = now.timestamp_millis();
    match q {
        "time" => answer(clock(ms), day_name(ms), "clock", format!("It's {}.", clock(ms))),
        "date" => answer(
            now.format("%A, %-d %B").to_string(),
            format!("Week {} · {}", now.iso_week().week(), now.year()),
            "calendar",
            format!("Today is {}.", now.format("%A, the %-d of %B")),
        ),
        "weather" => weather_reply(hub::last("weather").as_ref()),
        "battery" => battery_reply(hub::last("system").as_ref()),
        "playing" => playing_reply(hub::last("media").as_ref()),
        "calendar" => calendar_reply(hub::last("calendar").as_ref(), ms),
        "claude" => {
            let s = claude_web::state();
            claude_reply(s.five_hour.map(|w| (w.used_percent, w.resets_at)), s.seven_day.map(|w| w.used_percent))
        }
        "timer" => timer_left(),
        "thanks" => answer("You're welcome", "", "check", "You're welcome."),
        "name" => answer("I'm Ledge", "Your dock — I listen on this PC only", "help", "I'm Ledge, your dock."),
        _ => answer(
            "Try asking",
            "“What's the weather?” · “Set a timer for five minutes” · “Open WhatsApp”",
            "help",
            "You can ask me the time, the weather or your next meeting, set a timer, control music, or open any app.",
        ),
    }
}

// ---------- one recognizer for everything ----------
//
// A single continuous recognizer hears both "Hey Ledge …" (any time) and plain
// commands. Plain commands only count while Ledge is "armed": for a few
// seconds after the mic button, the hotkey, or "Hey Ledge" on its own. With
// "Hey Ledge" turned off, the recognizer runs only while armed.

const ARM_FOR: Duration = Duration::from_secs(8);

static REC: Mutex<Option<SpeechRecognizer>> = Mutex::new(None);
static STARTING: Mutex<()> = Mutex::new(());
static LIST: Mutex<Option<HashMap<String, Action>>> = Mutex::new(None);
static ARMED: Mutex<Option<Instant>> = Mutex::new(None);
static SHOWN: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Splits "(um) hey ledge open whatsapp" into "open whatsapp"; None without a wake phrase.
/// The wake phrase may come after other words; the earliest (then longest) wins.
pub fn after_wake(text: &str) -> Option<String> {
    let t = format!(" {} ", canon(text));
    WAKE.iter()
        .filter_map(|w| t.find(&format!(" {w} ")).map(|i| (i, std::cmp::Reverse(w.len()), i + w.len() + 1)))
        .min()
        .map(|(_, _, end)| t[end..].trim().to_string())
}

fn text_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Appends a line to %APPDATA%\Ledge\voice.log (kept under ~128 KB).
fn log(line: &str) {
    let path = config::dir().join("voice.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 128 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {line}", Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

fn armed() -> bool {
    ARMED.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < ARM_FOR)
}

fn disarm() {
    *ARMED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn hide_later(app: &AppHandle, after: Duration) {
    let app = app.clone();
    let id = CARD.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        if !armed() && CARD.load(Ordering::SeqCst) == id {
            show(&app, false);
            if !config::get().wake_word {
                stop();
            }
        }
    });
}

/// Listen for a command for a few seconds, with the card showing.
fn arm(app: &AppHandle, chime: bool) {
    arm_quiet(app);
    publish(app, "listening", "", &reply("Listening…", "mic"));
    show(app, true);
    if chime {
        sound("Speech On.wav", false);
    }
}

/// Count plain commands for a few seconds; if none comes, say so and hide.
fn arm_quiet(app: &AppHandle) {
    let at = Instant::now();
    *ARMED.lock().unwrap_or_else(|e| e.into_inner()) = Some(at);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ARM_FOR);
        if *ARMED.lock().unwrap_or_else(|e| e.into_inner()) == Some(at) {
            disarm();
            publish(&app, "unknown", "", &Reply { said: "I didn't catch that".into(), detail: "Try “what's the weather?” or “open WhatsApp”".into(), icon: "help", speak: None });
            hide_later(&app, Duration::from_millis(1800));
        }
    });
}

fn lookup(cmd: &str) -> Option<Action> {
    LIST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&canon(cmd)).cloned())
}

/// Runs a recognised command, shows the result, and says the answer.
fn execute(app: &AppHandle, cmd: &str) {
    disarm();
    let Some(action) = lookup(cmd) else {
        publish(app, "unknown", &text_case(cmd), &reply("I didn't catch that", "help"));
        show(app, true);
        hide_later(app, Duration::from_millis(1800));
        return;
    };
    if action == Action::Cancel {
        show(app, false);
        return;
    }
    let r = run(app, &action);
    log(&format!("  -> {:?}: {}", action, r.said));
    publish(app, "done", &text_case(cmd), &r);
    show(app, true);
    let app = app.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        if let Some(s) = &r.speak {
            say(s);
        }
        // Answers stay long enough to read; quick actions just confirm.
        let keep = if r.speak.is_some() { Duration::from_millis(5000) } else { Duration::from_millis(1800) };
        hide_later(&app, keep.saturating_sub(start.elapsed()).max(Duration::from_millis(900)));
    });
}

fn on_result(app: &AppHandle, text: &str, conf: SpeechRecognitionConfidence) {
    let level = match conf {
        SpeechRecognitionConfidence::High => "high",
        SpeechRecognitionConfidence::Medium => "medium",
        SpeechRecognitionConfidence::Low => "low",
        _ => "rejected",
    };
    log(&format!("heard \"{text}\" ({level}){}", if SPEAKING.load(Ordering::SeqCst) { " [ignored: speaking]" } else { "" }));
    RESULTS.fetch_add(1, Ordering::SeqCst);
    if text.trim().is_empty() || SPEAKING.load(Ordering::SeqCst) {
        return;
    }
    // Already handled from the live words (the engine was slow to finish).
    if FALLBACK.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|(t, at)| at.elapsed() < Duration::from_secs(4) && *t == after_wake(text).unwrap_or_else(|| canon(text))) {
        return;
    }
    let wake = after_wake(text);
    // A low-scoring result still counts when it carries the wake word: that's
    // what makes "Hey Ledge" feel deaf otherwise.
    if conf == SpeechRecognitionConfidence::Rejected && wake.is_none() {
        return;
    }
    if let Some(rest) = wake {
        if rest.is_empty() {
            arm(app, true);
        } else {
            execute(app, &rest);
        }
    } else if armed() {
        execute(app, text);
    }
}

/// Live words while you speak: shows the card as soon as "Hey Ledge" is heard.
fn on_hypothesis(app: &AppHandle, text: &str) {
    if SPEAKING.load(Ordering::SeqCst) {
        return;
    }
    let rest = after_wake(text);
    if rest.is_none() && !armed() {
        return;
    }
    if rest.is_some() && !armed() {
        arm_quiet(app);
    }
    let words = rest.unwrap_or_else(|| canon(text));
    publish(app, "listening", "", &reply(if words.is_empty() { "Listening…".to_string() } else { text_case(&words) }, "mic"));
    show(app, true);
    // If Windows drops the final result (it does, for quiet or quick speech),
    // act on the last live words once you've paused.
    let id = HYP.fetch_add(1, Ordering::SeqCst) + 1;
    let seen = RESULTS.load(Ordering::SeqCst);
    let (app, text) = (app.clone(), text.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1100));
        if HYP.load(Ordering::SeqCst) != id || RESULTS.load(Ordering::SeqCst) != seen || SPEAKING.load(Ordering::SeqCst) {
            return;
        }
        let cmd = match after_wake(&text) {
            Some(rest) => rest,
            None if armed() => canon(&text),
            None => return,
        };
        if cmd.is_empty() {
            log(&format!("live words \"{text}\": wake only"));
            arm(&app, true);
        } else if lookup(&cmd).is_some() {
            log(&format!("live words \"{text}\": no final result, running it"));
            *FALLBACK.lock().unwrap_or_else(|e| e.into_inner()) = Some((canon(&cmd), Instant::now()));
            execute(&app, &cmd);
        }
    });
}

static HYP: AtomicU64 = AtomicU64::new(0);
static RESULTS: AtomicU64 = AtomicU64::new(0);
static FALLBACK: Mutex<Option<(String, Instant)>> = Mutex::new(None);

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

/// The recognition language: your first English language if Windows can
/// recognise it (en-IN here), else Windows' speech language.
fn language() -> Option<windows::Globalization::Language> {
    use windows::System::UserProfile::GlobalizationPreferences;
    let supported = SpeechRecognizer::SupportedGrammarLanguages().ok()?;
    let tags: Vec<String> = (0..supported.Size().ok()?).filter_map(|i| supported.GetAt(i).ok()?.LanguageTag().ok().map(|t| t.to_string())).collect();
    let prefs = GlobalizationPreferences::Languages().ok()?;
    for i in 0..prefs.Size().ok()? {
        let p = prefs.GetAt(i).ok()?.to_string();
        if !p.to_lowercase().starts_with("en") {
            continue;
        }
        if let Some(t) = tags.iter().find(|t| t.eq_ignore_ascii_case(&p)) {
            return windows::Globalization::Language::CreateLanguage(&HSTRING::from(t.as_str())).ok();
        }
    }
    None
}

fn new_recognizer(lang: &Option<windows::Globalization::Language>) -> Result<SpeechRecognizer, String> {
    match lang {
        Some(l) => SpeechRecognizer::Create(l),
        None => SpeechRecognizer::new(),
    }
    .map_err(|_| "Windows speech recognition isn't available (add an English speech pack in Settings → Time & language → Speech)".to_string())
}

/// Compiles the grammar; falls back to a plain phrase list if Windows rejects it.
fn build(apps: &[shell::AppEntry], list: &[(String, Action)]) -> Result<SpeechRecognizer, String> {
    let lang = language();
    let tag = lang.as_ref().and_then(|l| l.LanguageTag().ok()).map(|t| t.to_string()).unwrap_or_else(|| "en-US".into());
    let path = config::dir().join("voice-grammar.xml");
    let r = new_recognizer(&lang)?;
    let grammar_ok = std::fs::write(&path, grammar(&tag, apps)).is_ok()
        && (|| -> windows::core::Result<bool> {
            let file = windows::Storage::StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.join()?;
            let c = SpeechRecognitionGrammarFileConstraint::CreateWithTag(&file, &HSTRING::from("ledge"))?;
            r.Constraints()?.Append(&c)?;
            Ok(r.CompileConstraintsAsync()?.join()?.Status()? == SpeechRecognitionResultStatus::Success)
        })()
        .unwrap_or(false);
    if grammar_ok {
        log(&format!("recognizer ready: grammar, {tag}, {} phrases", list.len()));
        return Ok(r);
    }
    let _ = r.Close();
    log("grammar rejected; using a phrase list");
    let r = new_recognizer(&lang)?;
    let mut words: Vec<HSTRING> = WAKE.iter().map(|w| HSTRING::from(*w)).collect();
    for (p, _) in list {
        words.push(HSTRING::from(p.as_str()));
        for w in WAKE.iter().take(2) {
            words.push(HSTRING::from(format!("{w} {p}")));
        }
    }
    let iter: windows_collections::IIterable<HSTRING> = words.into();
    let c = SpeechRecognitionListConstraint::Create(&iter).map_err(|e| e.message().to_string())?;
    r.Constraints().and_then(|cs| cs.Append(&c)).map_err(|e| e.message().to_string())?;
    let compiled = r.CompileConstraintsAsync().and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    if compiled.Status().ok() != Some(SpeechRecognitionResultStatus::Success) {
        return Err("Speech recognition couldn't start — check the microphone is allowed for desktop apps".into());
    }
    log(&format!("recognizer ready: phrase list, {tag}"));
    Ok(r)
}

/// Starts the recognizer if it isn't running. Takes a second or two the first time.
fn ensure_running(app: &AppHandle) -> Result<(), String> {
    use windows::Foundation::TypedEventHandler;
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs, SpeechContinuousRecognitionResultGeneratedEventArgs,
        SpeechContinuousRecognitionSession, SpeechRecognitionHypothesisGeneratedEventArgs,
    };
    let _one = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if running() {
        return Ok(());
    }
    let seq = GEN.load(Ordering::SeqCst);
    let apps = shell::apps_cached();
    let list = phrases(&apps);
    *LIST.lock().unwrap_or_else(|e| e.into_inner()) = Some(list.iter().map(|(p, a)| (canon(p), a.clone())).collect());
    let r = build(&apps, &list).inspect_err(|e| log(&format!("start failed: {e}")))?;
    let h = app.clone();
    let _ = r.HypothesisGenerated(&TypedEventHandler::<SpeechRecognizer, SpeechRecognitionHypothesisGeneratedEventArgs>::new(move |_, args| {
        if let Some(t) = args.as_ref().and_then(|a| a.Hypothesis().ok()).and_then(|h| h.Text().ok()) {
            let app = h.clone();
            let text = t.to_string();
            std::thread::spawn(move || on_hypothesis(&app, &text));
        }
        Ok(())
    }));
    let session = r.ContinuousRecognitionSession().map_err(|e| e.message().to_string())?;
    let _ = session.SetAutoStopSilenceTimeout(windows::Foundation::TimeSpan { Duration: 24 * 3600 * 10_000_000 });
    let a = app.clone();
    session
        .ResultGenerated(&TypedEventHandler::<SpeechContinuousRecognitionSession, SpeechContinuousRecognitionResultGeneratedEventArgs>::new(
            move |_, args| {
                let Some(args) = args.as_ref() else { return Ok(()) };
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
        .Completed(&TypedEventHandler::<SpeechContinuousRecognitionSession, SpeechContinuousRecognitionCompletedEventArgs>::new(move |_, args| {
            let status = args.as_ref().and_then(|a| a.Status().ok()).map(|s| s.0).unwrap_or(-1);
            log(&format!("session ended (status {status})"));
            let app = again.clone();
            if GEN.load(Ordering::SeqCst) == seq {
                std::thread::spawn(move || {
                    if let Some(r) = REC.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        let _ = r.Close();
                    }
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

/// Mic button / hotkey: listen for one command (or close the card if it's open).
pub fn listen(app: &AppHandle) {
    if armed() {
        disarm();
        show(app, false);
        return;
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        com_init();
        publish(&app, "listening", "", &reply("Listening…", "mic"));
        show(&app, true);
        match ensure_running(&app) {
            Ok(()) => arm(&app, false),
            Err(e) => {
                publish(&app, "error", "", &Reply { said: "Voice isn't available".into(), detail: e, icon: "help", speak: None });
                std::thread::sleep(Duration::from_millis(3200));
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
    let a = app.clone();
    std::thread::spawn(move || {
        com_init();
        let r = ensure_running(&a);
        hub::publish(&a, "voice-wake", &serde_json::json!({ "on": r.is_ok(), "error": r.err() }));
    });
    // A watchdog: if the listener ever stops (sleep, a new microphone, an
    // error), bring it back within half a minute.
    if !WATCHING.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        std::thread::spawn(move || {
            com_init();
            loop {
                std::thread::sleep(Duration::from_secs(30));
                if config::get().wake_word && !running() && !BUSY.load(Ordering::SeqCst) {
                    log("watchdog: restarting the listener");
                    let _ = ensure_running(&app);
                }
            }
        });
    }
}

/// The voice card, centred just above the dock.
fn show(app: &AppHandle, on: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow};
    let Some(w) = app.get_webview_window("voice") else { return };
    let Ok(h) = w.hwnd() else { return };
    let h = HWND(h.0);
    unsafe {
        if !on {
            SHOWN.store(false, Ordering::SeqCst);
            let _ = ShowWindow(h, SW_HIDE);
            return;
        }
        if SHOWN.swap(true, Ordering::SeqCst) {
            return;
        }
        let g = appbar::geometry();
        let s = g.scale;
        let (pw, ph) = ((CARD_W * s) as i32, (CARD_H * s) as i32);
        let x = (g.mon_left + g.mon_right) / 2 - pw / 2;
        let y = g.dock_top - (12.0 * s) as i32 - ph;
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, pw, ph, SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
    }
}

/// The card's size in DIPs (flyout.rs creates the window at this size).
pub const CARD_W: f64 = 420.0;
pub const CARD_H: f64 = 84.0;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wake_phrase_splits_off_the_command() {
        assert_eq!(after_wake("Hey Ledge open WhatsApp").as_deref(), Some("open whatsapp"));
        assert_eq!(after_wake("hey ledge").as_deref(), Some(""));
        assert_eq!(after_wake("OK Ledge, what's the time?").as_deref(), Some("whats the time"));
        assert_eq!(after_wake("hey ledger").as_deref(), Some(""));
        assert_eq!(after_wake("um so hey ledge open spotify").as_deref(), Some("open spotify"));
        assert_eq!(after_wake("ledge what time is it").as_deref(), Some("what time is it"));
        assert_eq!(after_wake("the knowledge base"), None);
        assert_eq!(after_wake("open whatsapp"), None);
    }

    #[test]
    fn canon_matches_any_spelling() {
        assert_eq!(canon("Volume 40"), "volume forty");
        assert_eq!(canon("volume forty"), "volume forty");
        assert_eq!(canon("Turn off Wi-Fi"), "turn off wi fi");
        assert_eq!(canon("What's the weather?"), "whats the weather");
        assert_eq!(num_words(45), "forty five");
        assert_eq!(num_words(100), "one hundred");
    }

    #[test]
    fn builds_phrases_for_apps_and_commands() {
        let apps = vec![
            shell::AppEntry { name: "WhatsApp".into(), id: "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App".into() },
            shell::AppEntry { name: "Google Chrome".into(), id: "Chrome".into() },
            shell::AppEntry { name: "Calendar".into(), id: "cal".into() },
        ];
        let p = phrases(&apps);
        let find = |s: &str| p.iter().find(|(x, _)| canon(x) == canon(s)).map(|(_, a)| a.clone());
        assert!(matches!(find("open whatsapp"), Some(Action::Open(n, _)) if n == "WhatsApp"));
        assert!(matches!(find("open chrome"), Some(Action::Open(n, _)) if n == "Google Chrome"));
        assert!(matches!(find("switch to google chrome"), Some(Action::Open(..))));
        assert_eq!(find("open calendar"), Some(Action::Panel("calendar")), "commands win over apps");
        assert_eq!(find("volume 40"), Some(Action::Volume(Some(0.4), false)));
        assert_eq!(find("what's the weather"), Some(Action::Ask("weather")));
        assert_eq!(find("what time is it"), Some(Action::Ask("time")));
        assert_eq!(find("set a timer for 5 minutes"), Some(Action::Timer(300)));
        assert_eq!(find("turn on do not disturb"), Some(Action::Dnd(true)));
        // Every phrase is unique, so a match is never ambiguous.
        let mut all: Vec<_> = p.iter().map(|(x, _)| canon(x)).collect();
        all.sort();
        let n = all.len();
        all.dedup();
        assert_eq!(n, all.len());
    }

    #[test]
    fn grammar_is_compact_and_escaped() {
        let apps = vec![shell::AppEntry { name: "Tom & Jerry".into(), id: "x".into() }];
        let g = grammar("en-IN", &apps);
        assert!(g.contains(r#"xml:lang="en-IN""#));
        assert!(g.contains("<item>tom jerry</item>"));
        assert!(!g.contains("& "));
        assert!(g.contains("<item>hey ledge</item>"));
        assert!(g.len() < 40_000, "grammar is {} bytes", g.len());
    }

    #[test]
    fn answers_read_naturally() {
        let w = json!({ "configured": true, "place": "Chennai", "temp": 28.4, "high": 31.2, "low": 24.0, "code": 2, "unit": "°C" });
        let r = weather_reply(Some(&w));
        assert_eq!(r.said, "28° · Partly cloudy");
        assert_eq!(r.detail, "Chennai · High 31°  Low 24°");
        assert_eq!(r.speak.as_deref(), Some("It's 28 degrees and partly cloudy in Chennai, with a high of 31 and a low of 24."));
        assert_eq!(weather_reply(Some(&json!({ "configured": false }))).said, "Weather isn't set up");

        let b = battery_reply(Some(&json!({ "battery": { "percent": 76, "charging": true } })));
        assert_eq!((b.said.as_str(), b.detail.as_str()), ("76%", "Charging"));
        assert_eq!(battery_reply(Some(&json!({ "battery": null }))).said, "No battery");

        let m = playing_reply(Some(&json!({ "title": "Blinding Lights", "artist": "The Weeknd", "playing": true })));
        assert_eq!(m.speak.as_deref(), Some("This is Blinding Lights by The Weeknd."));
        assert_eq!(playing_reply(None).said, "Nothing is playing");

        let c = claude_reply(Some((42.0, None)), Some(18.0));
        assert_eq!(c.said, "58% left");
        assert_eq!(c.detail, "5-hour limit 42% used · week 18%");

        let now = Local::now().timestamp_millis();
        let cal = json!({ "events": [
            { "title": "Holiday", "start": now - 1000, "end": now + 3_600_000, "allDay": true },
            { "title": "Standup", "start": now + 25 * 60_000, "end": now + 40 * 60_000, "allDay": false }
        ]});
        let r = calendar_reply(Some(&cal), now);
        assert_eq!(r.said, "Standup");
        assert!(r.detail.contains("in 25 minutes") || r.speak.as_deref().unwrap_or("").contains("Nothing else today"), "{r:?}");
        assert_eq!(calendar_reply(None, now).said, "Your calendar is clear");
        assert_eq!(duration_words(300), "5 minutes");
        assert_eq!(duration_words(90), "1 minute and 30 seconds");
        assert_eq!(duration_words(3600), "1 hour");
    }

    /// Compiles the real grammar with Windows' speech engine.
    /// `cargo test --lib voice::tests::live_grammar_compiles -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_grammar_compiles() {
        com_init();
        let apps = shell::apps_cached();
        let list = phrases(&apps);
        let lang = language();
        println!("language: {:?}", lang.as_ref().and_then(|l| l.LanguageTag().ok()));
        println!("apps: {}, phrases: {}, grammar bytes: {}", apps.len(), list.len(), grammar("en-IN", &apps).len());
        let r = build(&apps, &list).expect("recognizer");
        let _ = r.Close();
        let log = std::fs::read_to_string(config::dir().join("voice.log")).unwrap_or_default();
        let last = log.lines().last().unwrap_or("");
        println!("{last}");
        assert!(last.contains("grammar"), "grammar was rejected: {last}");
    }

    #[test]
    #[ignore]
    fn live_speak() {
        com_init();
        let wav = synth("It's 28 degrees and partly cloudy in Chennai.").expect("synth");
        assert!(wav.len() > 10_000);
    }
}
