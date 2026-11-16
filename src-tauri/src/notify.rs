//! Notifications, read (read-only) from the database Windows keeps for its
//! notification center: %LOCALAPPDATA%\Microsoft\Windows\Notifications\wpndatabase.db.
//! New ones become Ledge's own glass banners; the list feeds Ledge's
//! notification center and the WhatsApp widget (WhatsApp Desktop and
//! WhatsApp mirrored from a phone through Phone Link).
//!
//! Windows' own banners are hidden by turning on Do Not Disturb — every
//! notification still lands in this database.

use crate::{appbar, config, hub};
use base64::Engine;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Notif {
    pub id: i64,
    pub order: i64,
    pub aumid: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub at: i64,
    /// Sender avatar / app logo override from the toast, as a data URL.
    pub image: Option<String>,
    pub whatsapp: bool,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct WhatsApp {
    pub count: usize,
    pub sender: String,
    pub text: String,
    pub at: i64,
    pub image: Option<String>,
    /// AUMID to open: WhatsApp Desktop when that's where they came from, else Phone Link.
    pub open: String,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Center {
    pub available: bool,
    pub items: Vec<Notif>,
    pub whatsapp: WhatsApp,
}

const FILETIME_EPOCH_DIFF: i64 = 116_444_736_000_000_000;
const PHONE_LINK: &str = "Microsoft.YourPhone_8wekyb3d8bbwe!App";

static DISMISSED: LazyLock<Mutex<HashSet<i64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static CLEARED_BELOW: Mutex<i64> = Mutex::new(0);
static NEWEST: AtomicI64 = AtomicI64::new(0);

fn db_path() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join(r"Microsoft\Windows\Notifications\wpndatabase.db"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&amp;", "&")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(unescape(&tag[start..end]))
}

/// (texts, image src) from a toast's XML payload.
pub fn parse_toast(xml: &str) -> (Vec<String>, Option<String>) {
    let mut texts = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<text") {
        let after = &rest[i..];
        let Some(gt) = after.find('>') else { break };
        let open = &after[..gt];
        if open.ends_with('/') {
            rest = &after[gt + 1..];
            continue;
        }
        let Some(close) = after.find("</text>") else { break };
        let inner = unescape(after[gt + 1..close].trim());
        if !inner.is_empty() && attr(open, "placement").as_deref() != Some("attribution") {
            texts.push(inner);
        }
        rest = &after[close + 7..];
    }
    let mut image = None;
    let mut rest = xml;
    while let Some(i) = rest.find("<image") {
        let after = &rest[i..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        if let Some(src) = attr(tag, "src") {
            let logo = attr(tag, "placement").as_deref() == Some("appLogoOverride");
            if logo || image.is_none() {
                image = Some(src);
            }
            if logo {
                break;
            }
        }
        rest = &after[end..];
    }
    (texts, image)
}

/// Phone Link repeats the app's name as the first line ("WhatsApp", sender, message).
pub fn strip_app_line(mut texts: Vec<String>, app: &str) -> Vec<String> {
    if texts.len() > 1 && texts[0].eq_ignore_ascii_case(app) {
        texts.remove(0);
    }
    texts
}

/// Friendly app name for an AUMID, including apps mirrored through Phone Link.
pub fn app_name(aumid: &str) -> String {
    if let Some(pkg) = aumid.split("YourPhoneNotifications_").nth(1) {
        return match pkg {
            "com.whatsapp" | "com.whatsapp.w4b" => "WhatsApp".into(),
            "com.instagram.android" => "Instagram".into(),
            "com.snapchat.android" => "Snapchat".into(),
            "com.google.android.gm" => "Gmail".into(),
            "org.telegram.messenger" => "Telegram".into(),
            other => {
                let last = other.rsplit('.').next().unwrap_or(other);
                let mut c = last.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        };
    }
    if aumid.contains("YourPhone") {
        return "Phone Link".into();
    }
    if aumid.to_lowercase().contains("whatsapp") {
        return "WhatsApp".into();
    }
    let display = crate::shell::run({
        let a = aumid.to_string();
        move || crate::shell::display_name(&format!("shell:AppsFolder\\{a}"))
    })
    .flatten();
    display.unwrap_or_else(|| {
        let base = aumid.split('!').next().unwrap_or(aumid);
        let base = base.split('_').next().unwrap_or(base);
        base.rsplit('.').next().unwrap_or(base).to_string()
    })
}

static NAMES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static IMAGES: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_name(aumid: &str) -> String {
    if let Some(n) = NAMES.lock().ok().and_then(|m| m.get(aumid).cloned()) {
        return n;
    }
    let n = app_name(aumid);
    if let Ok(mut m) = NAMES.lock() {
        m.insert(aumid.to_string(), n.clone());
    }
    n
}

/// Only local files (Phone Link writes contact avatars to disk). Never fetches URLs.
fn image_data(src: &str) -> Option<String> {
    if let Some(hit) = IMAGES.lock().ok()?.get(src) {
        return hit.clone();
    }
    let out = (|| {
        let p = src.strip_prefix("file:///").or_else(|| src.strip_prefix("file://"))?;
        let p = p.replace("%20", " ").replace('/', "\\");
        let bytes = std::fs::read(&p).ok().filter(|b| !b.is_empty() && b.len() < 2_000_000)?;
        let mime = if bytes.starts_with(&[0x89, b'P']) { "image/png" } else if bytes.starts_with(&[0xff, 0xd8]) { "image/jpeg" } else { "image/png" };
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    })();
    if let Ok(mut m) = IMAGES.lock() {
        m.insert(src.to_string(), out.clone());
    }
    out
}

fn read_all(conn: &Connection) -> rusqlite::Result<Vec<Notif>> {
    let mut stmt = conn.prepare_cached(
        "SELECT n.\"Order\", n.Id, h.PrimaryId, n.Payload, n.ArrivalTime FROM Notification n \
         JOIN NotificationHandler h ON h.RecordId = n.HandlerId WHERE n.Type = 'toast' \
         ORDER BY n.\"Order\" DESC LIMIT 80",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<Vec<u8>>>(3)?, r.get::<_, Option<i64>>(4)?))
    })?;
    let mut out = Vec::new();
    for row in rows.flatten() {
        let (order, id, aumid, payload, at) = row;
        let xml = String::from_utf8_lossy(&payload.unwrap_or_default()).into_owned();
        let (texts, img) = parse_toast(&xml);
        let app = cached_name(&aumid);
        let texts = strip_app_line(texts, &app);
        if texts.is_empty() {
            continue;
        }
        let title = texts[0].clone();
        let body = texts[1..].join("\n");
        out.push(Notif {
            id,
            order,
            whatsapp: aumid.to_lowercase().contains("whatsapp"),
            app,
            title,
            body,
            at: at.map(|t| (t - FILETIME_EPOCH_DIFF) / 10_000).unwrap_or(0),
            image: img.as_deref().and_then(image_data),
            aumid,
        });
    }
    Ok(out)
}

fn summarize(all: &[Notif]) -> Center {
    let dismissed = DISMISSED.lock().map(|d| d.clone()).unwrap_or_default();
    let below = *CLEARED_BELOW.lock().unwrap_or_else(|e| e.into_inner());
    let items: Vec<Notif> = all.iter().filter(|n| n.order > below && !dismissed.contains(&n.id)).take(40).cloned().collect();
    let wa: Vec<&Notif> = all.iter().filter(|n| n.whatsapp).collect();
    let whatsapp = match wa.first() {
        Some(last) => WhatsApp {
            count: wa.len(),
            sender: last.title.clone(),
            text: last.body.lines().next().unwrap_or_default().to_string(),
            at: last.at,
            image: last.image.clone(),
            // The WhatsApp app itself when it's installed — not Phone Link.
            open: if whatsapp_installed() {
                WHATSAPP_APP.into()
            } else if last.aumid.contains("YourPhone") {
                PHONE_LINK.into()
            } else {
                last.aumid.clone()
            },
        },
        None => WhatsApp::default(),
    };
    Center { available: true, items, whatsapp }
}

pub fn dismiss(id: Option<i64>) {
    match id {
        Some(id) => {
            if let Ok(mut d) = DISMISSED.lock() {
                d.insert(id);
            }
        }
        // "Clear all": hide everything up to the newest one seen so far.
        None => *CLEARED_BELOW.lock().unwrap_or_else(|e| e.into_inner()) = NEWEST.load(Ordering::SeqCst),
    }
}

/// Opens the app a notification came from.
pub const WHATSAPP_APP: &str = "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App";

pub fn whatsapp_installed() -> bool {
    dirs::data_local_dir().is_some_and(|d| d.join("Packages").join("5319275A.WhatsAppDesktop_cv1g1gvanyjgm").exists())
}

pub fn open_target(aumid: &str) -> String {
    if aumid.contains("YourPhone") {
        format!("shell:AppsFolder\\{PHONE_LINK}")
    } else {
        format!("shell:AppsFolder\\{aumid}")
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-notify".into())
        .spawn(move || {
            let mut conn: Option<Connection> = None;
            let mut last_order: Option<i64> = None;
            loop {
                if conn.is_none() {
                    conn = db_path().and_then(|p| {
                        Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()
                    });
                    if let Some(c) = &conn {
                        let _ = c.busy_timeout(Duration::from_millis(300));
                    }
                }
                let all = conn.as_ref().map(read_all);
                match all {
                    Some(Ok(all)) => {
                        let newest = all.first().map(|n| n.order).unwrap_or(0);
                        if let Some(prev) = last_order {
                            let cfg = config::get();
                            let banners = cfg.widgets.banners && !cfg.dnd;
                            if banners && !appbar::is_fullscreen_app() {
                                // Oldest first, at most 3 at once (a burst after sleep shouldn't flood).
                                for n in all.iter().filter(|n| n.order > prev).take(3).collect::<Vec<_>>().into_iter().rev() {
                                    let _ = app.emit("toast", n);
                                }
                            }
                        }
                        last_order = Some(newest.max(last_order.unwrap_or(0)));
                        NEWEST.store(newest, Ordering::SeqCst);
                        hub::publish(&app, "notifications", &summarize(&all));
                    }
                    Some(Err(_)) => conn = None, // reopen next time (DB rotated or locked)
                    None => hub::publish(&app, "notifications", &Center::default()),
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        })
        .expect("spawn notification watcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toast_text_and_logo() {
        let xml = r#"<toast launch="x"><visual><binding template="ToastGeneric"><image placement="appLogoOverride" hint-crop="circle" src="file:///C:/Users/a/b%20c.png"/><text>Anna</text><text>See you at 8 &amp; bring snacks</text><text placement="attribution">via Phone Link</text></binding></visual></toast>"#;
        let (t, img) = parse_toast(xml);
        assert_eq!(t, vec!["Anna", "See you at 8 & bring snacks"]);
        assert_eq!(img.as_deref(), Some("file:///C:/Users/a/b%20c.png"));
        assert_eq!(parse_toast("<toast><text/><text>Hi</text></toast>").0, vec!["Hi"]);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(strip_app_line(s(&["WhatsApp", "Anna", "hi"]), "WhatsApp"), s(&["Anna", "hi"]));
        assert_eq!(strip_app_line(s(&["WhatsApp"]), "WhatsApp"), s(&["WhatsApp"]));
    }

    #[test]
    fn names_phone_link_apps() {
        assert_eq!(app_name("Microsoft.YourPhone_8wekyb3d8bbwe!YourPhoneNotifications_com.whatsapp"), "WhatsApp");
        assert_eq!(app_name("Microsoft.YourPhone_8wekyb3d8bbwe!YourPhoneNotifications_com.example.coolapp"), "Coolapp");
        assert_eq!(app_name("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"), "WhatsApp");
    }
}
