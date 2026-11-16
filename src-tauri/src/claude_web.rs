//! Exact Claude plan usage — the same numbers as claude.ai → Settings → Usage.
//!
//! Claude Code's logs only see Claude Code. Chats in the Claude app and on
//! claude.ai use the same 5-hour and weekly limits, so a log-based estimate can
//! never match. Instead the user signs in to claude.ai once, inside a Ledge
//! window: Ledge never sees the password, and the session cookie stays in
//! Ledge's own web view profile. Then, every couple of minutes (every 30 s while
//! Claude Code is busy), Ledge opens a hidden, short-lived web view on claude.ai,
//! asks the endpoint the Usage page itself uses, and closes the web view again.
//! The answer is handed back through the page title, so the remote page gets no
//! access to Ledge's commands.

use crate::{ai, config};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const SIGNIN: &str = "claude-signin";
const PROBE: &str = "claude-usage";
const MARK: &str = "LEDGE";

#[derive(Clone, Default, Debug, PartialEq)]
pub struct WebPlan {
    pub five_hour: Option<ai::PlanWindow>,
    pub seven_day: Option<ai::PlanWindow>,
    pub fetched_at: Option<i64>,
    pub error: Option<String>,
}

static STATE: Mutex<WebPlan> = Mutex::new(WebPlan { five_hour: None, seven_day: None, fetched_at: None, error: None });
static WAITER: Mutex<Option<tokio::sync::oneshot::Sender<String>>> = Mutex::new(None);

pub fn state() -> WebPlan {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Runs on claude.ai pages only. `MODE` is "signin" (wait until signed in) or "usage".
fn script(mode: &str) -> String {
    format!(
        r#"(() => {{
  if (location.hostname !== "claude.ai") return;
  const MODE = "{mode}";
  const say = (o) => {{
    if (!document.head) document.documentElement.prepend(document.createElement("head"));
    document.title = "{MARK}" + JSON.stringify(o);
  }};
  const get = (u) => fetch(u, {{ credentials: "include", headers: {{ accept: "application/json" }} }});
  async function usage() {{
    const r = await get("/api/organizations");
    if (!r.ok) return {{ signedIn: false, status: r.status }};
    const orgs = await r.json();
    if (!Array.isArray(orgs) || !orgs.length) return {{ signedIn: false, status: 0 }};
    const org = orgs.find((o) => (o.capabilities || []).some((c) => /max|pro|team|enterprise/i.test(c))) || orgs[0];
    const u = await get("/api/organizations/" + org.uuid + "/usage");
    if (!u.ok) return {{ signedIn: true, status: u.status }};
    return {{ signedIn: true, usage: await u.json() }};
  }}
  const run = async () => {{
    try {{ say(await usage()); }} catch (e) {{ say({{ error: String(e) }}); }}
  }};
  const start = () => {{
    if (MODE === "usage") return run();
    const t = setInterval(async () => {{
      try {{
        const r = await get("/api/organizations");
        if (r.ok) {{ clearInterval(t); run(); }}
      }} catch (e) {{}}
    }}, 2500);
  }};
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", start); else start();
}})();"#
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Applies one report from the page. Returns whether the user is signed in.
pub fn apply(report: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(report) else { return false };
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(u) = v.get("usage") {
        let (five, seven) = ai::parse_plan(u);
        st.five_hour = five;
        st.seven_day = seven;
        st.fetched_at = Some(now_ms());
        st.error = None;
        return true;
    }
    let signed_in = v.get("signedIn").and_then(Value::as_bool).unwrap_or(false);
    st.error = Some(if let Some(e) = v.get("error").and_then(Value::as_str) {
        format!("claude.ai didn't answer ({e})")
    } else if !signed_in {
        "Signed out of claude.ai — reconnect in Settings".into()
    } else {
        format!("claude.ai said {}", v.get("status").and_then(Value::as_i64).unwrap_or(0))
    });
    signed_in
}

fn report_of(title: &str) -> Option<&str> {
    title.strip_prefix(MARK).filter(|r| r.starts_with('{'))
}

/// Opens the sign-in window. When claude.ai says the user is in, Ledge
/// switches to exact numbers and closes the window.
pub fn open_sign_in(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(SIGNIN) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let url = "https://claude.ai/login".parse().map_err(|e| format!("{e}"))?;
    WebviewWindowBuilder::new(app, SIGNIN, WebviewUrl::External(url))
        .title("Connect Claude — Ledge")
        .inner_size(480.0, 760.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .initialization_script(script("signin"))
        .on_document_title_changed(|w, title| {
            let Some(report) = report_of(&title) else { return };
            if apply(report) {
                if let Ok(cfg) = config::update(|c| c.claude_web = true) {
                    use tauri::Emitter;
                    let _ = w.app_handle().emit("config", &cfg);
                }
                let w = w.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    let _ = w.close();
                });
            }
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// One reading: a hidden web view, alive for a few seconds.
async fn probe(app: &AppHandle) {
    if let Some(old) = app.get_webview_window(PROBE) {
        let _ = old.destroy();
    }
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    *WAITER.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    // A tiny static page on claude.ai: the script needs the origin, not the app.
    let Ok(url) = "https://claude.ai/robots.txt".parse() else { return };
    let built = WebviewWindowBuilder::new(app, PROBE, WebviewUrl::External(url))
        .title("Ledge")
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .inner_size(320.0, 240.0)
        .initialization_script(script("usage"))
        .on_document_title_changed(|_, title| {
            if let Some(report) = report_of(&title) {
                if let Some(tx) = WAITER.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = tx.send(report.to_string());
                }
            }
        })
        .build();
    let Ok(w) = built else {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).error = Some("Couldn't open claude.ai".into());
        return;
    };
    match tokio::time::timeout(Duration::from_secs(40), rx).await {
        Ok(Ok(report)) => {
            apply(&report);
        }
        _ => STATE.lock().unwrap_or_else(|e| e.into_inner()).error = Some("claude.ai didn't answer".into()),
    }
    let _ = w.destroy();
}

pub async fn run(app: AppHandle) {
    let mut last: Option<Instant> = None;
    let mut last_tokens = 0u64;
    loop {
        if config::get().claude_web {
            let tokens = ai::local_tokens();
            let age = last.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
            let busy = tokens != last_tokens;
            if age > Duration::from_secs(120) || (busy && age > Duration::from_secs(30)) {
                last_tokens = tokens;
                last = Some(Instant::now());
                probe(&app).await;
            }
        } else {
            last = None;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Read again right away (after sign-in, or from the panel's refresh).
pub fn refresh_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { probe(&app).await });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_usage_reports() {
        assert!(report_of("Claude").is_none());
        let r = r#"{"signedIn":true,"usage":{"five_hour":{"utilization":37.0,"resets_at":"2026-11-16T15:00:00+00:00"},"seven_day":{"utilization":12.5,"resets_at":null}}}"#;
        assert!(apply(report_of(&format!("LEDGE{r}")).unwrap()));
        let s = state();
        assert_eq!(s.five_hour.as_ref().map(|w| w.used_percent), Some(37.0));
        assert_eq!(s.seven_day.as_ref().map(|w| w.used_percent), Some(12.5));
        assert!(s.error.is_none());
        assert!(!apply(r#"{"signedIn":false,"status":401}"#));
        assert!(state().error.unwrap().contains("Signed out"));
    }
}
