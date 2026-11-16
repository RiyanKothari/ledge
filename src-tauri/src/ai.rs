//! AI plan usage from logs the tools already write locally — no logins,
//! no tokens read, nothing sent anywhere.
//!
//! * Claude Code: ~/.claude/projects/**/*.jsonl carry per-request token usage.
//!   We rebuild the 5-hour session windows and a 7-day total. Anthropic does
//!   not log the plan's limit, so the bar compares the current window with
//!   your own busiest window of the last 8 days.
//! * Codex (ChatGPT plan): ~/.codex/sessions/**/*.jsonl[.zst] carry the real
//!   `rate_limits` (percent used, window length, reset time) from the server.

use crate::hub;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const HOUR: i64 = 3_600_000;
const BLOCK: i64 = 5 * HOUR;
const DAY: i64 = 24 * HOUR;
const RETAIN_DAYS: i64 = 15;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub ts: i64,
    pub tokens: u64,
    pub model: String,
}

/// Claude Code logs the server's verdict when a request is refused for a
/// plan limit. The tokens used up to that moment are the limit itself.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitHit {
    pub ts: i64,
    /// "five_hour", "seven_day", ...
    pub kind: String,
    pub resets_at: Option<i64>,
}

pub fn parse_limit_line(line: &str) -> Option<LimitHit> {
    if !line.contains("rate_limit_info") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let info = find_key(&v, "rate_limit_info")?;
    if info.get("status").and_then(Value::as_str) != Some("rejected") {
        return None;
    }
    let ts = chrono::DateTime::parse_from_rfc3339(v.get("timestamp")?.as_str()?).ok()?.timestamp_millis();
    Some(LimitHit {
        ts,
        kind: info.get("rateLimitType").and_then(Value::as_str).unwrap_or("").to_string(),
        resets_at: info.get("resetsAt").and_then(Value::as_i64).map(|s| s * 1000),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub start: i64,
    pub last: i64,
    pub tokens: u64,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    pub available: bool,
    pub session_tokens: u64,
    pub session_started_at: Option<i64>,
    pub session_resets_at: Option<i64>,
    pub peak_session_tokens: u64,
    pub week_tokens: u64,
    /// Busiest rolling 7 days in the retained history (the weekly bar's 100 %).
    pub peak_week_tokens: u64,
    /// Tokens per minute over the last 5 minutes.
    pub burn_per_min: u64,
    pub last_activity: Option<i64>,
    pub top_model: String,
    pub top_model_tokens: u64,
    /// Tokens per UTC day, oldest first, last 7 days.
    pub daily: Vec<u64>,
    /// Real 5-hour limit in tokens, learned from the last time Anthropic refused a request.
    pub five_hour_limit: Option<u64>,
    /// Real weekly limit in tokens, learned the same way.
    pub week_limit: Option<u64>,
    /// When the last limit hit happened (the calibration's date).
    pub calibrated_at: Option<i64>,
    /// Set while a limit is in force: requests are refused until then.
    pub limited_until: Option<i64>,
    pub limited_kind: Option<String>,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at: Option<i64>,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsage {
    pub available: bool,
    pub primary: Option<LimitWindow>,
    pub secondary: Option<LimitWindow>,
    pub plan_type: Option<String>,
    /// When the log line was written (ms). Old data is shown as such.
    pub as_of: i64,
}

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    pub claude: ClaudeUsage,
    pub codex: CodexUsage,
}

// ---------- Claude ----------

pub fn parse_claude_line(line: &str) -> Option<Entry> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let msg = v.get("message")?;
    let u = msg.get("usage")?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tokens = n("input_tokens") + n("output_tokens") + n("cache_creation_input_tokens");
    let ts = chrono::DateTime::parse_from_rfc3339(v.get("timestamp")?.as_str()?).ok()?.timestamp_millis();
    // One API response is logged once per content block: dedupe on request id.
    let id = v
        .get("requestId")
        .and_then(Value::as_str)
        .or_else(|| msg.get("id").and_then(Value::as_str))
        .or_else(|| v.get("uuid").and_then(Value::as_str))?
        .to_string();
    let model = msg.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    Some(Entry { id, ts, tokens, model })
}

/// "claude-opus-5-5-20261101" -> "Opus 5.5"
pub fn model_label(id: &str) -> String {
    let parts: Vec<&str> = id.trim_start_matches("claude-").split('-').collect();
    let Some(family) = parts.first().filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphabetic())) else {
        return id.to_string();
    };
    let nums: Vec<&str> =
        parts[1..].iter().copied().filter(|p| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit())).take(2).collect();
    let mut name = family.to_string();
    if let Some(f) = name.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    if nums.is_empty() { name } else { format!("{name} {}", nums.join(".")) }
}

/// 5-hour windows: a window opens at the hour of the first request after the
/// previous one closed, and lasts five hours.
pub fn blocks(sorted: &[Entry]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for e in sorted {
        match out.last_mut() {
            Some(b) if e.ts < b.start + BLOCK => {
                b.last = e.ts;
                b.tokens += e.tokens;
            }
            _ => out.push(Block { start: e.ts - e.ts.rem_euclid(HOUR), last: e.ts, tokens: e.tokens }),
        }
    }
    out
}

pub fn summarize(entries: &[Entry], hits: &[LimitHit], now: i64) -> ClaudeUsage {
    let mut seen = HashSet::new();
    let mut list: Vec<Entry> = entries.iter().filter(|e| seen.insert(e.id.clone())).cloned().collect();
    list.sort_by_key(|e| e.ts);
    let bl = blocks(&list);
    let active = bl.last().filter(|b| now < b.start + BLOCK);
    let week: Vec<&Entry> = list.iter().filter(|e| e.ts >= now - 7 * DAY).collect();
    let mut by_model: HashMap<String, u64> = HashMap::new();
    for e in &week {
        if !e.model.is_empty() && !e.model.starts_with('<') {
            *by_model.entry(model_label(&e.model)).or_default() += e.tokens;
        }
    }
    let (top_model, top_model_tokens) = by_model.into_iter().max_by_key(|(_, t)| *t).unwrap_or_default();
    let today = now.div_euclid(DAY);
    let mut days: HashMap<i64, u64> = HashMap::new();
    for e in &list {
        *days.entry(e.ts.div_euclid(DAY)).or_default() += e.tokens;
    }
    let first = days.keys().min().copied().unwrap_or(today);
    let peak_week_tokens = (first..=today)
        .map(|end| (end - 6..=end).map(|d| days.get(&d).copied().unwrap_or(0)).sum::<u64>())
        .max()
        .unwrap_or(0);
    // Calibrate from the newest refusal of each kind.
    let latest = |kind: &str| hits.iter().filter(|h| h.kind.contains(kind)).max_by_key(|h| h.ts);
    let five = latest("five_hour").and_then(|h| {
        let block = bl.iter().find(|b| h.ts >= b.start && h.ts < b.start + BLOCK)?;
        let used: u64 = list.iter().filter(|e| e.ts >= block.start && e.ts <= h.ts).map(|e| e.tokens).sum();
        (used > 0).then_some((used, h.ts))
    });
    let wk = latest("seven_day").and_then(|h| {
        let used: u64 = list.iter().filter(|e| e.ts > h.ts - 7 * DAY && e.ts <= h.ts).map(|e| e.tokens).sum();
        (used > 0).then_some((used, h.ts))
    });
    let blocking = hits.iter().filter(|h| h.resets_at.is_some_and(|r| r > now)).max_by_key(|h| h.resets_at);
    ClaudeUsage {
        five_hour_limit: five.map(|f| f.0),
        week_limit: wk.map(|w| w.0),
        calibrated_at: five.map(|f| f.1).max(wk.map(|w| w.1)),
        limited_until: blocking.and_then(|h| h.resets_at),
        limited_kind: blocking.map(|h| h.kind.clone()),
        available: !list.is_empty(),
        session_tokens: active.map(|b| b.tokens).unwrap_or(0),
        session_started_at: active.map(|b| b.start),
        session_resets_at: active.map(|b| b.start + BLOCK),
        peak_session_tokens: bl.iter().map(|b| b.tokens).max().unwrap_or(0),
        week_tokens: week.iter().map(|e| e.tokens).sum(),
        peak_week_tokens,
        burn_per_min: list.iter().filter(|e| e.ts >= now - 5 * 60_000).map(|e| e.tokens).sum::<u64>() / 5,
        last_activity: list.last().map(|e| e.ts),
        top_model,
        top_model_tokens,
        daily: (today - 6..=today).map(|d| days.get(&d).copied().unwrap_or(0)).collect(),
    }
}

#[derive(Default)]
struct FileState {
    offset: u64,
    entries: Vec<Entry>,
    hits: Vec<LimitHit>,
}

static FILES: LazyLock<Mutex<HashMap<PathBuf, FileState>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn walk(dir: &Path, depth: u32, cutoff: SystemTime, ext: &[&str], out: &mut Vec<(SystemTime, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, cutoff, ext, out);
            }
        } else {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
            if ext.iter().any(|x| name.ends_with(x)) && mtime >= cutoff {
                out.push((mtime, p));
            }
        }
    }
}

/// Reads only the bytes appended since the last pass.
fn scan(path: &Path, st: &mut FileState) {
    let Ok(mut f) = std::fs::File::open(path) else { return };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len < st.offset {
        *st = FileState::default(); // rewritten
    }
    if len == st.offset || f.seek(SeekFrom::Start(st.offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return;
    }
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return };
    for line in String::from_utf8_lossy(&buf[..=end]).lines() {
        // A refused request is logged as an assistant line too (with empty
        // usage), so check both, independently.
        if let Some(h) = parse_limit_line(line) {
            st.hits.push(h);
        }
        if let Some(e) = parse_claude_line(line) {
            st.entries.push(e);
        }
    }
    st.offset += end as u64 + 1;
}

fn claude_usage(now: i64) -> ClaudeUsage {
    let Some(home) = dirs::home_dir() else { return ClaudeUsage::default() };
    let cutoff = SystemTime::now() - Duration::from_secs(RETAIN_DAYS as u64 * 24 * 3600);
    let mut files = Vec::new();
    walk(&home.join(".claude").join("projects"), 4, cutoff, &[".jsonl"], &mut files);
    let mut map = FILES.lock().unwrap_or_else(|e| e.into_inner());
    let live: HashSet<PathBuf> = files.iter().map(|(_, p)| p.clone()).collect();
    map.retain(|p, _| live.contains(p));
    let mut all = Vec::new();
    let mut hits = Vec::new();
    for (_, p) in files {
        let st = map.entry(p.clone()).or_default();
        scan(&p, st);
        st.entries.retain(|e| e.ts >= now - RETAIN_DAYS * DAY);
        all.extend(st.entries.iter().cloned());
        hits.extend(st.hits.iter().cloned());
    }
    summarize(&all, &hits, now)
}

// ---------- Claude plan limits (opt-in) ----------
//
// Off unless the user turns on "Live Claude plan limits" in Settings. Then,
// every 90 s (every 20 s while tokens are being used), Ledge reads the access
// token Claude Code saved in ~/.claude/.credentials.json and asks
// api.anthropic.com — the service that issued it — for the plan's usage.
// The token is never logged, stored, refreshed or sent anywhere else. When it
// has expired Ledge waits for Claude Code to renew it.

static LOCAL_TOKENS: AtomicU64 = AtomicU64::new(0);

/// Changes whenever Claude Code uses tokens (lets readers poll faster while busy).
pub fn local_tokens() -> u64 {
    LOCAL_TOKENS.load(Ordering::SeqCst)
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanWindow {
    pub used_percent: f64,
    pub resets_at: Option<i64>,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanLimits {
    pub enabled: bool,
    pub five_hour: Option<PlanWindow>,
    pub seven_day: Option<PlanWindow>,
    pub fetched_at: Option<i64>,
    pub error: Option<String>,
    /// "claude.ai" or "claude-code".
    pub source: String,
}

fn plan_window(v: &Value) -> Option<PlanWindow> {
    let used = v.get("utilization").and_then(Value::as_f64)?;
    let resets_at = v
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|d| d.timestamp_millis());
    Some(PlanWindow { used_percent: used.clamp(0.0, 100.0), resets_at })
}

pub fn parse_plan(v: &Value) -> (Option<PlanWindow>, Option<PlanWindow>) {
    (plan_window(&v["five_hour"]), plan_window(&v["seven_day"]))
}

async fn fetch_plan(client: &reqwest::Client) -> Result<(Option<PlanWindow>, Option<PlanWindow>), String> {
    let path = dirs::home_dir().ok_or("no home folder")?.join(".claude").join(".credentials.json");
    let text = std::fs::read_to_string(path).map_err(|_| "Sign in to Claude Code first".to_string())?;
    let creds: Value = serde_json::from_str(&text).map_err(|_| "Claude Code sign-in file unreadable".to_string())?;
    let oauth = &creds["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().ok_or("Claude Code isn't signed in with a plan")?;
    if oauth["expiresAt"].as_i64().is_some_and(|e| e <= now_ms()) {
        return Err("Sign-in expired — run Claude Code once to renew it".into());
    }
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Anthropic said {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_plan(&v))
}

pub async fn plan_loop(app: AppHandle) {
    let Ok(client) = reqwest::Client::builder().user_agent("Ledge/0.2").timeout(Duration::from_secs(15)).build() else {
        return;
    };
    let mut last: Option<Instant> = None;
    let mut last_tokens = 0u64;
    let mut state = PlanLimits::default();
    hub::publish(&app, "ai-plan", &state);
    loop {
        if crate::config::get().claude_web {
            // Exact numbers from claude.ai (see claude_web.rs).
            let web = crate::claude_web::state();
            let next = PlanLimits {
                enabled: true,
                five_hour: web.five_hour,
                seven_day: web.seven_day,
                fetched_at: web.fetched_at,
                error: web.error,
                source: "claude.ai".into(),
            };
            if next != state {
                state = next;
                hub::publish(&app, "ai-plan", &state);
            }
        } else if !crate::config::get().claude_live_limits {
            if state.enabled {
                state = PlanLimits::default();
                hub::publish(&app, "ai-plan", &state);
            }
        } else {
            let tokens = LOCAL_TOKENS.load(Ordering::SeqCst);
            let age = last.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
            let busy = tokens != last_tokens;
            if !state.enabled || age > Duration::from_secs(90) || (busy && age > Duration::from_secs(20)) {
                last_tokens = tokens;
                last = Some(Instant::now());
                state.enabled = true;
                state.source = "claude-code".into();
                match fetch_plan(&client).await {
                    Ok((five, seven)) => {
                        state.five_hour = five;
                        state.seven_day = seven;
                        state.fetched_at = Some(now_ms());
                        state.error = None;
                    }
                    Err(e) => state.error = Some(e),
                }
                hub::publish(&app, "ai-plan", &state);
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// ---------- Codex ----------

fn limit_window(v: &Value) -> Option<LimitWindow> {
    if v.is_null() {
        return None;
    }
    Some(LimitWindow {
        used_percent: v.get("used_percent")?.as_f64()?,
        window_minutes: v.get("window_minutes").and_then(Value::as_u64).unwrap_or(0),
        resets_at: v.get("resets_at").and_then(Value::as_i64).map(|s| s * 1000),
    })
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.get(key).or_else(|| m.values().find_map(|c| find_key(c, key))),
        _ => None,
    }
}

/// The newest `rate_limits` with a primary window in a Codex session log.
pub fn parse_codex_log(text: &str) -> Option<(CodexUsage, Option<i64>)> {
    for line in text.lines().rev().filter(|l| l.contains("\"rate_limits\"")) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(rl) = find_key(&v, "rate_limits") else { continue };
        let Some(primary) = rl.get("primary").and_then(limit_window) else { continue };
        let ts = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|d| d.timestamp_millis());
        return Some((
            CodexUsage {
                available: true,
                primary: Some(primary),
                secondary: rl.get("secondary").and_then(limit_window),
                plan_type: rl.get("plan_type").and_then(Value::as_str).map(str::to_owned),
                as_of: 0,
            },
            ts,
        ));
    }
    None
}

static CODEX_CACHE: Mutex<Option<(PathBuf, SystemTime, CodexUsage)>> = Mutex::new(None);

fn codex_usage(now: i64) -> CodexUsage {
    let Some(home) = dirs::home_dir() else { return CodexUsage::default() };
    let mut files = Vec::new();
    walk(&home.join(".codex").join("sessions"), 4, UNIX_EPOCH, &[".jsonl", ".jsonl.zst"], &mut files);
    files.sort();
    // Newest file that actually has rate limits (the newest may be empty).
    for (mtime, path) in files.into_iter().rev().take(5) {
        {
            let c = CODEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((p, m, u)) = c.as_ref() {
                if *p == path && *m == mtime {
                    return expire(u.clone(), now);
                }
            }
        }
        let text = if path.to_string_lossy().ends_with(".zst") {
            std::fs::File::open(&path).ok().and_then(|f| zstd::stream::decode_all(f).ok())
        } else {
            std::fs::read(&path).ok()
        };
        let Some(text) = text else { continue };
        if let Some((mut u, ts)) = parse_codex_log(&String::from_utf8_lossy(&text)) {
            u.as_of = ts.unwrap_or_else(|| mtime.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0));
            *CODEX_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((path, mtime, u.clone()));
            return expire(u, now);
        }
    }
    CodexUsage::default()
}

/// A window whose reset time has passed is back to 0%.
pub fn expire(mut u: CodexUsage, now: i64) -> CodexUsage {
    for w in [&mut u.primary, &mut u.secondary].into_iter().flatten() {
        if w.resets_at.is_some_and(|r| r <= now) {
            w.used_percent = 0.0;
            w.resets_at = None;
        }
    }
    u
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-ai".into())
        .spawn(move || {
            let mut tick = 0u64;
            let mut codex = CodexUsage::default();
            loop {
                let now = now_ms();
                if tick % 12 == 0 {
                    codex = codex_usage(now);
                }
                let claude = claude_usage(now);
                LOCAL_TOKENS.store(claude.week_tokens, Ordering::SeqCst);
                hub::publish(&app, "ai", &AiUsage { claude, codex: codex.clone() });
                tick += 1;
                // Claude Code appends to its log while it streams: a short
                // tick makes the bars move while you work.
                std::thread::sleep(Duration::from_secs(4));
            }
        })
        .expect("spawn ai usage watcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, ts: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"{id}","timestamp":"{ts}","message":{{"id":"m{id}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":10,"cache_read_input_tokens":999}}}}}}"#
        )
    }

    #[test]
    fn parses_usage_and_ignores_cache_reads() {
        let e = parse_claude_line(&line("r1", "2026-11-15T10:20:00Z", 5, 7)).unwrap();
        assert_eq!(e.tokens, 22);
        assert_eq!(e.id, "r1");
        assert!(parse_claude_line(r#"{"type":"user","message":{"content":"usage"}}"#).is_none());
    }

    #[test]
    fn duplicate_lines_count_once_and_windows_split_at_five_hours() {
        let ts = |h: i64, m: i64| 1_700_000_000_000 - 1_700_000_000_000 % HOUR + h * HOUR + m * 60_000;
        let e = |id: &str, t: i64, n: u64| Entry { id: id.into(), ts: t, tokens: n, model: "claude-opus-5-5".into() };
        let entries = vec![
            e("a", ts(0, 30), 100),
            e("a", ts(0, 30), 100), // same request logged twice
            e("b", ts(4, 50), 50),
            e("c", ts(5, 10), 7), // past the 5h mark from 00:00 → new window
        ];
        let u = summarize(&entries, &[], ts(6, 0));
        assert_eq!(u.session_tokens, 7);
        assert_eq!(u.session_resets_at, Some(ts(10, 0)));
        assert_eq!(u.peak_session_tokens, 150);
        assert_eq!(u.week_tokens, 157);
        let idle = summarize(&entries, &[], ts(11, 0));
        assert_eq!(idle.session_tokens, 0);
        assert_eq!(idle.session_resets_at, None);
        assert_eq!(u.top_model, "Opus 5.5");
        assert_eq!(u.top_model_tokens, 157);
        assert_eq!(u.daily.iter().sum::<u64>(), 157);
    }

    /// Read-only, this PC's logs: `cargo test --lib live_claude -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_claude_calibration() {
        let u = claude_usage(now_ms());
        println!(
            "session={} limit5h={:?} week={} weekLimit={:?} calibratedAt={:?} limitedUntil={:?}",
            u.session_tokens, u.five_hour_limit, u.week_tokens, u.week_limit, u.calibrated_at, u.limited_until
        );
        assert!(u.five_hour_limit.is_some(), "expected a calibration from the logged refusal");
    }

    #[test]
    fn calibrates_limits_from_a_refusal() {
        let h = |m: i64| 1_700_000_000_000 - 1_700_000_000_000 % HOUR + m * 60_000;
        let e = |id: &str, t: i64, n: u64| Entry { id: id.into(), ts: t, tokens: n, model: String::new() };
        let entries = vec![e("a", h(10), 400), e("b", h(100), 600), e("c", h(400), 50)];
        // Refused at minute 120 of the first window: 1000 tokens was the limit.
        let line = format!(
            r#"{{"type":"assistant","timestamp":"{}","apiErrorParams":{{"rate_limit_info":{{"status":"rejected","resetsAt":{},"rateLimitType":"five_hour"}}}}}}"#,
            chrono::DateTime::from_timestamp_millis(h(120)).unwrap().to_rfc3339(),
            h(300) / 1000
        );
        let hit = parse_limit_line(&line).unwrap();
        assert_eq!(hit.kind, "five_hour");
        let during = summarize(&entries, &[hit.clone()], h(150));
        assert_eq!(during.five_hour_limit, Some(1000));
        assert_eq!(during.limited_until, Some(h(300)));
        let after = summarize(&entries, &[hit], h(420));
        assert_eq!(after.limited_until, None);
        assert_eq!(after.session_tokens, 50);
        assert!(parse_limit_line(r#"{"rate_limit_info":{"status":"allowed"}}"#).is_none());
    }

    #[test]
    fn labels_models_and_reads_plan() {
        assert_eq!(model_label("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_label("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(model_label("claude-haiku-5-5"), "Haiku 5.5");
        let v: Value = serde_json::from_str(
            r#"{"five_hour":{"utilization":34.0,"resets_at":"2026-11-15T21:00:00+00:00"},"seven_day":{"utilization":37,"resets_at":null},"seven_day_opus":null}"#,
        )
        .unwrap();
        let (f, s) = parse_plan(&v);
        assert_eq!(f.unwrap().used_percent, 34.0);
        let s = s.unwrap();
        assert_eq!((s.used_percent, s.resets_at), (37.0, None));
    }

    #[test]
    fn reads_codex_rate_limits_and_expires_them() {
        let log = concat!(
            r#"{"timestamp":"2026-09-13T08:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":40,"window_minutes":300,"resets_at":100},"secondary":null}}}"#,
            "\n",
            r#"{"timestamp":"2026-09-13T09:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":88,"window_minutes":43200,"resets_at":1791881150},"secondary":null,"plan_type":"go"}}}"#,
        );
        let (u, ts) = parse_codex_log(log).unwrap();
        let p = u.primary.clone().unwrap();
        assert_eq!(p.used_percent, 88.0);
        assert_eq!(p.window_minutes, 43200);
        assert_eq!(p.resets_at, Some(1_791_881_150_000));
        assert_eq!(u.plan_type.as_deref(), Some("go"));
        assert!(ts.is_some());
        let later = expire(u, 1_791_881_150_001);
        assert_eq!(later.primary.unwrap().used_percent, 0.0);
    }
}
