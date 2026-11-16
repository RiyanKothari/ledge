//! RennSquare paper funds, from the live.json its monitor writes
//! (data\dashboard\live.json). Read-only; re-read when the file changes.

use crate::{config, hub};
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, SystemTime};
use tauri::AppHandle;

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Fund {
    pub id: String,
    pub name: String,
    pub value: f64,
    pub day_pnl: f64,
    pub day_return: f64,
    pub positions: u64,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub level: String,
    pub what: String,
    pub detail: String,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Portfolio {
    pub configured: bool,
    pub error: Option<String>,
    pub prices_as_of: String,
    pub checked_at: String,
    pub nifty_day: Option<f64>,
    pub funds: Vec<Fund>,
    pub total_value: f64,
    pub total_day_pnl: f64,
    pub total_day_return: f64,
    pub bad_alerts: usize,
    pub warn_alerts: usize,
    pub alerts: Vec<Alert>,
}

pub fn pretty(id: &str) -> String {
    id.split('_')
        .filter(|w| !w.is_empty() && *w != "shadow")
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse(v: &Value) -> Portfolio {
    let f = |x: &Value| x.as_f64().unwrap_or(0.0);
    let mut funds: Vec<Fund> = v["funds"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    let id = x["fund"].as_str().unwrap_or("").to_string();
                    Fund {
                        name: pretty(&id),
                        id,
                        value: x["value_now"].as_f64().or(x["last_value"].as_f64()).unwrap_or(0.0),
                        day_pnl: f(&x["day_pnl"]),
                        day_return: f(&x["day_return"]),
                        positions: x["positions"].as_u64().unwrap_or(0),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    funds.sort_by(|a, b| b.day_return.partial_cmp(&a.day_return).unwrap_or(std::cmp::Ordering::Equal));
    let total_value: f64 = funds.iter().map(|x| x.value).sum();
    let total_day_pnl: f64 = funds.iter().map(|x| x.day_pnl).sum();
    let base = total_value - total_day_pnl;
    let mut alerts: Vec<Alert> = v["alerts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| Alert {
                    level: x["level"].as_str().unwrap_or("").into(),
                    what: x["what"].as_str().unwrap_or("").into(),
                    detail: x["detail"].as_str().unwrap_or("").into(),
                })
                .collect()
        })
        .unwrap_or_default();
    let bad_alerts = alerts.iter().filter(|a| a.level == "bad").count();
    let warn_alerts = alerts.iter().filter(|a| a.level == "warn").count();
    alerts.sort_by_key(|a| if a.level == "bad" { 0 } else { 1 });
    alerts.truncate(8);
    Portfolio {
        configured: true,
        error: None,
        prices_as_of: v["prices_as_of"].as_str().unwrap_or("").into(),
        checked_at: v["checked_at"].as_str().unwrap_or("").into(),
        nifty_day: v["nifty50_day"].as_f64(),
        funds,
        total_value,
        total_day_pnl,
        total_day_return: if base > 0.0 { total_day_pnl / base } else { 0.0 },
        bad_alerts,
        warn_alerts,
        alerts,
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-portfolio".into())
        .spawn(move || {
            let mut seen: Option<(String, SystemTime)> = None;
            loop {
                let path = config::get().portfolio_path;
                if path.trim().is_empty() {
                    hub::publish(&app, "portfolio", &Portfolio::default());
                    seen = None;
                } else {
                    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                    let key = mtime.map(|m| (path.clone(), m));
                    if key != seen {
                        seen = key;
                        let p = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
                            Ok(v) => parse(&v),
                            Err(e) => Portfolio { configured: true, error: Some(format!("Can't read RennSquare data: {e}")), ..Default::default() },
                        };
                        hub::publish(&app, "portfolio", &p);
                    }
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        })
        .expect("spawn portfolio watcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_and_names() {
        let v: Value = serde_json::from_str(
            r#"{"prices_as_of":"2026-11-06T15:30","nifty50_day":0.02,
            "funds":[{"fund":"berkshire_p20","value_now":110.0,"day_pnl":10.0,"day_return":0.1,"positions":20},
                     {"fund":"shadow_lgbm_p10_5cr","value_now":90.0,"day_pnl":-10.0,"day_return":-0.1,"positions":10}],
            "alerts":[{"level":"warn","what":"a","detail":""},{"level":"bad","what":"b","detail":""}]}"#,
        )
        .unwrap();
        let p = parse(&v);
        assert_eq!(p.total_value, 200.0);
        assert_eq!(p.total_day_pnl, 0.0);
        assert_eq!(p.funds[0].name, "Berkshire P20");
        assert_eq!(p.funds[1].name, "Lgbm P10 5cr");
        assert_eq!((p.bad_alerts, p.warn_alerts), (1, 1));
        assert_eq!(p.alerts[0].level, "bad");
    }
}
