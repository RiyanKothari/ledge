//! Upcoming events from secret iCal (.ics) addresses — Google, Outlook and
//! iCloud all offer one, so no OAuth is needed.
//!
//! Supported: DTSTART/DTEND (UTC, floating, TZID, all-day), DURATION-less
//! events, RRULE with FREQ DAILY/WEEKLY/MONTHLY/YEARLY + INTERVAL, COUNT,
//! UNTIL and plain BYDAY for weekly rules, EXDATE, moved occurrences
//! (RECURRENCE-ID) and cancelled events.
//! Limitation: TZID times are read as this PC's local time — right whenever the
//! calendar's zone matches the PC's, which is the usual case.

use crate::{config, hub};
use chrono::{Datelike, Duration as CDur, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawEvent {
    pub uid: String,
    pub summary: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub rrule: Option<String>,
    pub exdates: Vec<i64>,
    pub recurrence_id: Option<i64>,
    pub cancelled: bool,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// "event", "allday", or for events added in Ledge: "renewal", "payout", "due".
    pub kind: String,
    /// Set for events added in Ledge (they can be deleted there).
    pub local_id: Option<String>,
}

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CalendarState {
    pub configured: bool,
    /// From now on (the dock's "next event").
    pub events: Vec<Event>,
    /// About six weeks back to ten weeks ahead, for the month view.
    pub month: Vec<Event>,
    pub error: Option<String>,
}

// ---------- events added in Ledge ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalEvent {
    pub id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub kind: String,
    /// "", "monthly" or "yearly".
    #[serde(default)]
    pub repeat: String,
}

static LOCAL_VERSION: AtomicU64 = AtomicU64::new(0);

fn local_path() -> std::path::PathBuf {
    config::dir().join("events.json")
}

pub fn local_events() -> Vec<LocalEvent> {
    std::fs::read_to_string(local_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_local(list: &[LocalEvent]) -> Result<(), String> {
    let _ = std::fs::create_dir_all(config::dir());
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    let tmp = local_path().with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, local_path()).map_err(|e| e.to_string())?;
    LOCAL_VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

pub fn add_local(mut e: LocalEvent) -> Result<(), String> {
    if e.title.trim().is_empty() {
        return Err("Give the event a name".into());
    }
    e.title = e.title.trim().to_string();
    if e.end < e.start {
        e.end = e.start;
    }
    if e.id.is_empty() {
        e.id = format!("l{}", Utc::now().timestamp_millis());
    }
    let mut list = local_events();
    list.push(e);
    save_local(&list)
}

pub fn remove_local(id: &str) -> Result<(), String> {
    let mut list = local_events();
    list.retain(|e| e.id != id);
    save_local(&list)
}

/// Occurrences of Ledge's own events between `from` and `to`.
pub fn expand_local(list: &[LocalEvent], from: i64, to: i64) -> Vec<Event> {
    let mut out = Vec::new();
    for e in list {
        let len = e.end - e.start;
        let Some(first) = Local.timestamp_millis_opt(e.start).single() else { continue };
        for n in 0..240i32 {
            let at = match e.repeat.as_str() {
                "monthly" => first.checked_add_months(chrono::Months::new(n as u32)),
                "yearly" => first.checked_add_months(chrono::Months::new(12 * n as u32)),
                _ if n == 0 => Some(first),
                _ => None,
            };
            let Some(at) = at else { break };
            let s = at.timestamp_millis();
            if s > to {
                break;
            }
            if s + len.max(1) > from {
                out.push(Event {
                    title: e.title.clone(),
                    start: s,
                    end: s + len,
                    all_day: e.all_day,
                    kind: if e.kind.is_empty() { "event".into() } else { e.kind.clone() },
                    local_id: Some(e.id.clone()),
                });
            }
        }
    }
    out
}

/// RFC 5545 line unfolding: a line starting with space/tab continues the previous one.
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

/// "NAME;P=V;Q=W:value" -> (NAME, params, value)
fn split_prop(line: &str) -> Option<(String, HashMap<String, String>, String)> {
    let mut in_quote = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            in_quote = !in_quote;
        }
        c == ':' && !in_quote
    })?;
    let (head, value) = (&line[..colon.0], &line[colon.0 + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some((name, params, value.to_string()))
}

fn local_ms(ndt: NaiveDateTime) -> Option<i64> {
    Local.from_local_datetime(&ndt).earliest().map(|d| d.timestamp_millis())
}

/// Returns (unix ms, all-day).
pub fn parse_dt(value: &str, params: &HashMap<String, String>) -> Option<(i64, bool)> {
    let v = value.trim();
    if params.get("VALUE").is_some_and(|x| x == "DATE") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok()?;
        return Some((local_ms(d.and_time(NaiveTime::MIN))?, true));
    }
    let ndt = NaiveDateTime::parse_from_str(v.get(..15)?, "%Y%m%dT%H%M%S").ok()?;
    if v.ends_with('Z') {
        Some((Utc.from_utc_datetime(&ndt).timestamp_millis(), false))
    } else {
        Some((local_ms(ndt)?, false))
    }
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

pub fn parse_ics(text: &str) -> Vec<RawEvent> {
    let mut out = Vec::new();
    let mut cur: Option<RawEvent> = None;
    let mut end_set = false;
    for line in unfold(text) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(RawEvent::default());
            end_set = false;
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(mut e) = cur.take() {
                if e.start != 0 {
                    if !end_set {
                        e.end = if e.all_day { e.start + 24 * 3_600_000 } else { e.start };
                    }
                    out.push(e);
                }
            }
            continue;
        }
        let Some(e) = cur.as_mut() else { continue };
        let Some((name, params, value)) = split_prop(&line) else { continue };
        match name.as_str() {
            "UID" => e.uid = value,
            "SUMMARY" => e.summary = unescape(&value),
            "DTSTART" => {
                if let Some((t, all_day)) = parse_dt(&value, &params) {
                    e.start = t;
                    e.all_day = all_day;
                }
            }
            "DTEND" => {
                if let Some((t, _)) = parse_dt(&value, &params) {
                    e.end = t;
                    end_set = true;
                }
            }
            "RRULE" => e.rrule = Some(value),
            "EXDATE" => e.exdates.extend(value.split(',').filter_map(|v| parse_dt(v, &params)).map(|(t, _)| t)),
            "RECURRENCE-ID" => e.recurrence_id = parse_dt(&value, &params).map(|(t, _)| t),
            "STATUS" => e.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    out
}

fn weekday(code: &str) -> Option<chrono::Weekday> {
    use chrono::Weekday::*;
    Some(match code {
        "MO" => Mon,
        "TU" => Tue,
        "WE" => Wed,
        "TH" => Thu,
        "FR" => Fri,
        "SA" => Sat,
        "SU" => Sun,
        _ => return None,
    })
}

fn add_months(d: NaiveDate, months: i64) -> Option<NaiveDate> {
    let total = d.year() as i64 * 12 + d.month0() as i64 + months;
    NaiveDate::from_ymd_opt((total / 12) as i32, (total % 12) as u32 + 1, d.day())
}

/// Occurrence (start, end) pairs of `ev` that overlap [from, to).
pub fn expand(ev: &RawEvent, from: i64, to: i64) -> Vec<(i64, i64)> {
    let dur = (ev.end - ev.start).max(0);
    let overlaps = |s: i64| s < to && s + dur.max(1) > from;
    let Some(rule) = ev.rrule.as_deref() else {
        return if overlaps(ev.start) { vec![(ev.start, ev.start + dur)] } else { vec![] };
    };
    let parts: HashMap<String, String> = rule
        .split(';')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.to_ascii_uppercase()))
        .collect();
    let interval = parts.get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let count = parts.get("COUNT").and_then(|v| v.parse::<u32>().ok());
    let until = parts.get("UNTIL").and_then(|v| parse_dt(v, &HashMap::new())).map(|(t, _)| t);
    let byday_raw: Vec<&str> = parts.get("BYDAY").map(|v| v.split(',').collect()).unwrap_or_default();
    // Ordinal BYDAY ("2MO", "-1FR") is not supported: show only the first occurrence.
    if byday_raw.iter().any(|d| d.len() > 2) {
        return if overlaps(ev.start) { vec![(ev.start, ev.start + dur)] } else { vec![] };
    }
    let Some(start_local) = Local.timestamp_millis_opt(ev.start).single() else { return vec![] };
    let (start_date, start_time) = (start_local.date_naive(), start_local.time());
    let freq = parts.get("FREQ").map(String::as_str).unwrap_or("");
    let exdates: HashSet<i64> = ev.exdates.iter().copied().collect();

    let mut out = Vec::new();
    let mut n = 0u32;
    let mut emit = |date: NaiveDate| -> bool {
        // returns false to stop
        let Some(s) = local_ms(date.and_time(start_time)) else { return true };
        if s < ev.start {
            return true;
        }
        n += 1;
        if count.is_some_and(|c| n > c) || until.is_some_and(|u| s > u) || s >= to {
            return false;
        }
        if overlaps(s) && !exdates.contains(&s) {
            out.push((s, s + dur));
        }
        true
    };
    for k in 0..5000i64 {
        let go_on = match freq {
            "DAILY" => emit(start_date + CDur::days(k * interval)),
            "WEEKLY" => {
                let monday = start_date - CDur::days(start_date.weekday().num_days_from_monday() as i64);
                let week = monday + CDur::weeks(k * interval);
                let mut days: Vec<u32> = byday_raw.iter().filter_map(|d| weekday(d)).map(|w| w.num_days_from_monday()).collect();
                if days.is_empty() {
                    days.push(start_date.weekday().num_days_from_monday());
                }
                days.sort();
                days.dedup();
                days.into_iter().all(|d| emit(week + CDur::days(d as i64)))
            }
            "MONTHLY" => match add_months(start_date, k * interval) {
                Some(d) => emit(d),
                None => true, // e.g. the 31st in a 30-day month
            },
            "YEARLY" => match add_months(start_date, 12 * k * interval) {
                Some(d) => emit(d),
                None => true,
            },
            _ => {
                if overlaps(ev.start) {
                    return vec![(ev.start, ev.start + dur)];
                }
                return vec![];
            }
        };
        if !go_on {
            break;
        }
    }
    out
}

pub fn upcoming(events: &[RawEvent], now: i64, horizon_ms: i64, limit: usize) -> Vec<Event> {
    // Occurrences moved or cancelled individually are removed from their series.
    let mut moved: HashMap<&str, Vec<i64>> = HashMap::new();
    for e in events.iter().filter(|e| e.recurrence_id.is_some()) {
        moved.entry(e.uid.as_str()).or_default().push(e.recurrence_id.unwrap_or(0));
    }
    let mut out: Vec<Event> = Vec::new();
    for e in events.iter().filter(|e| !e.cancelled) {
        let mut ev = e.clone();
        if e.recurrence_id.is_none() {
            if let Some(m) = moved.get(e.uid.as_str()) {
                ev.exdates.extend(m);
            }
        }
        for (s, end) in expand(&ev, now, now + horizon_ms) {
            out.push(Event {
                title: ev.summary.clone(),
                start: s,
                end,
                all_day: ev.all_day,
                kind: if ev.all_day { "allday".into() } else { "event".into() },
                local_id: None,
            });
        }
    }
    out.sort_by_key(|e| (e.start, e.title.clone()));
    out.dedup();
    out.truncate(limit);
    out
}

async fn fetch_all(client: &reqwest::Client, urls: &[String]) -> (Vec<RawEvent>, Option<String>) {
    let mut events = Vec::new();
    let mut failed = 0;
    for url in urls {
        let url = url.trim();
        let url = match url.strip_prefix("webcal://") {
            Some(rest) => format!("https://{rest}"),
            None => url.to_string(),
        };
        let res = async {
            let r = client.get(&url).send().await?.error_for_status()?;
            r.text().await
        }
        .await;
        match res {
            Ok(text) => events.extend(parse_ics(&text)),
            Err(_) => failed += 1, // never echo the secret URL
        }
    }
    let err = (failed > 0).then(|| format!("{failed} of {} calendars could not be loaded", urls.len()));
    (events, err)
}

pub async fn run(app: AppHandle) {
    let client = reqwest::Client::builder().user_agent("Ledge/0.1").timeout(Duration::from_secs(20)).build();
    let Ok(client) = client else { return };
    let mut seen_version = u64::MAX;
    let mut last_fetch: Option<Instant> = None;
    let mut cached: Vec<RawEvent> = Vec::new();
    let mut error: Option<String> = None;
    let mut seen_local = u64::MAX;
    let mut last_publish: Option<Instant> = None;
    loop {
        let local_v = LOCAL_VERSION.load(Ordering::SeqCst);
        if local_v == seen_local && last_publish.is_some_and(|t| t.elapsed() < Duration::from_secs(15)) {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        seen_local = local_v;
        last_publish = Some(Instant::now());
        let version = config::VERSION.load(Ordering::SeqCst);
        let cfg = config::get();
        let stale = last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(600));
        if version != seen_version || stale {
            seen_version = version;
            last_fetch = Some(Instant::now());
            (cached, error) = if cfg.ics_urls.is_empty() { (Vec::new(), None) } else { fetch_all(&client, &cfg.ics_urls).await };
        }
        let now = Local::now().timestamp_millis();
        const DAY: i64 = 24 * 3_600_000;
        let local = local_events();
        let mut events = upcoming(&cached, now, 35 * DAY, 80);
        events.extend(expand_local(&local, now, now + 35 * DAY));
        events.sort_by_key(|e| (e.start, e.title.clone()));
        events.truncate(80);
        let mut month = upcoming(&cached, now - 45 * DAY, 120 * DAY, 600);
        month.extend(expand_local(&local, now - 45 * DAY, now + 75 * DAY));
        month.sort_by_key(|e| (e.start, e.title.clone()));
        let state = CalendarState {
            configured: !cfg.ics_urls.is_empty() || !local.is_empty(),
            events,
            month,
            error: error.clone(),
        };
        hub::publish(&app, "calendar", &state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_ledge_events_monthly() {
        let start = Local.with_ymd_and_hms(2026, 1, 31, 9, 0, 0).unwrap().timestamp_millis();
        let e = LocalEvent { id: "a".into(), title: "Netflix".into(), start, end: start, all_day: true, kind: "renewal".into(), repeat: "monthly".into() };
        let from = Local.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap().timestamp_millis();
        let to = Local.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap().timestamp_millis();
        let got = expand_local(&[e], from, to);
        // Feb has no 31st: chrono clamps to the 28th.
        let days: Vec<u32> = got.iter().map(|g| Local.timestamp_millis_opt(g.start).unwrap().day()).collect();
        assert_eq!(days, vec![28, 31]);
        assert!(got.iter().all(|g| g.kind == "renewal" && g.local_id.as_deref() == Some("a")));
    }

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        local_ms(NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap()).unwrap()
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:lec\r\nSUMMARY:Calculus\r\n  lecture\r\nDTSTART;TZID=Asia/Kolkata:20261102T200000\r\nDTEND;TZID=Asia/Kolkata:20261102T210000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TH;COUNT=10\r\nEXDATE;TZID=Asia/Kolkata:20261116T200000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:lec\r\nRECURRENCE-ID;TZID=Asia/Kolkata:20261119T200000\r\nSUMMARY:Calculus (moved)\r\nDTSTART;TZID=Asia/Kolkata:20261119T180000\r\nDTEND;TZID=Asia/Kolkata:20261119T190000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:x\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261117\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261117T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_folded_lines_and_all_day_events() {
        let ev = parse_ics(ICS);
        assert_eq!(ev.len(), 4);
        assert_eq!(ev[0].summary, "Calculus lecture");
        let holiday = ev.iter().find(|e| e.uid == "x").unwrap();
        assert!(holiday.all_day);
        assert_eq!(holiday.end - holiday.start, 24 * 3_600_000);
    }

    #[test]
    fn weekly_rule_with_exdate_override_and_cancellation() {
        let ev = parse_ics(ICS);
        let now = local(2026, 11, 15, 18, 0);
        let up = upcoming(&ev, now, 9 * 24 * 3_600_000, 10);
        let titles: Vec<(&str, i64)> = up.iter().map(|e| (e.title.as_str(), e.start)).collect();
        assert_eq!(
            titles,
            vec![
                // Mon 16th is excluded by EXDATE
                ("Holiday", local(2026, 11, 17, 0, 0)),
                ("Calculus (moved)", local(2026, 11, 19, 18, 0)),
                ("Calculus lecture", local(2026, 11, 23, 20, 0)),
            ]
        );
    }

    #[test]
    fn count_and_until_stop_the_series() {
        let base = RawEvent {
            start: local(2026, 1, 1, 9, 0),
            end: local(2026, 1, 1, 10, 0),
            rrule: Some("FREQ=DAILY;COUNT=3".into()),
            ..Default::default()
        };
        let all = expand(&base, local(2025, 12, 1, 0, 0), local(2026, 2, 1, 0, 0));
        assert_eq!(all.len(), 3);
        let until = RawEvent { rrule: Some("FREQ=DAILY;UNTIL=20260102T235959".into()), ..base.clone() };
        assert_eq!(expand(&until, local(2025, 12, 1, 0, 0), local(2026, 2, 1, 0, 0)).len(), 2);
        let monthly = RawEvent { rrule: Some("FREQ=MONTHLY;INTERVAL=2".into()), ..base };
        assert_eq!(expand(&monthly, local(2026, 1, 1, 0, 0), local(2026, 7, 1, 0, 0)).len(), 3);
    }
}
