//! Current weather from Open-Meteo (free, no API key). Only the city you
//! type in Settings — or its coordinates — is sent.

use crate::{config, hub};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::AppHandle;

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    pub configured: bool,
    pub place: String,
    pub temp: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub code: Option<i64>,
    pub is_day: bool,
    pub unit: String,
    pub error: Option<String>,
}

async fn get_json(client: &reqwest::Client, url: &str, query: &[(&str, String)]) -> Result<Value, String> {
    client
        .get(url)
        .query(query)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.without_url().to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())
}

async fn geocode(client: &reqwest::Client, city: &str) -> Result<(f64, f64, String), String> {
    let v = get_json(
        client,
        "https://geocoding-api.open-meteo.com/v1/search",
        &[("name", city.to_string()), ("count", "1".into()), ("format", "json".into())],
    )
    .await?;
    let r = v.get("results").and_then(|r| r.get(0)).ok_or("City not found")?;
    let lat = r.get("latitude").and_then(Value::as_f64).ok_or("City not found")?;
    let lon = r.get("longitude").and_then(Value::as_f64).ok_or("City not found")?;
    let name = r.get("name").and_then(Value::as_str).unwrap_or(city);
    Ok((lat, lon, name.to_string()))
}

async fn fetch(client: &reqwest::Client, cfg: &config::Config) -> Weather {
    let unit = if cfg.fahrenheit { "°F" } else { "°C" }.to_string();
    let mut w = Weather { configured: !cfg.city.trim().is_empty(), unit, ..Default::default() };
    if !w.configured {
        return w;
    }
    let (lat, lon, place) = match (cfg.latitude, cfg.longitude) {
        (Some(lat), Some(lon)) => (lat, lon, cfg.place_label.clone()),
        _ => match geocode(client, cfg.city.trim()).await {
            Ok((lat, lon, name)) => {
                let label = name.clone();
                let _ = config::update(|c| {
                    c.latitude = Some(lat);
                    c.longitude = Some(lon);
                    c.place_label = label;
                });
                (lat, lon, name)
            }
            Err(e) => {
                w.error = Some(e);
                return w;
            }
        },
    };
    w.place = place;
    let mut q = vec![
        ("latitude", lat.to_string()),
        ("longitude", lon.to_string()),
        ("current", "temperature_2m,weather_code,is_day".to_string()),
        ("daily", "temperature_2m_max,temperature_2m_min".to_string()),
        ("timezone", "auto".to_string()),
        ("forecast_days", "1".to_string()),
    ];
    if cfg.fahrenheit {
        q.push(("temperature_unit", "fahrenheit".to_string()));
    }
    match get_json(client, "https://api.open-meteo.com/v1/forecast", &q).await {
        Ok(v) => {
            let cur = &v["current"];
            w.temp = cur["temperature_2m"].as_f64();
            w.code = cur["weather_code"].as_i64();
            w.is_day = cur["is_day"].as_i64() == Some(1);
            w.high = v["daily"]["temperature_2m_max"][0].as_f64();
            w.low = v["daily"]["temperature_2m_min"][0].as_f64();
        }
        Err(e) => w.error = Some(e),
    }
    w
}

pub async fn run(app: AppHandle) {
    let client = reqwest::Client::builder().user_agent("Ledge/0.1").timeout(Duration::from_secs(15)).build();
    let Ok(client) = client else { return };
    let mut seen_version = u64::MAX;
    let mut last: Option<Instant> = None;
    loop {
        let version = config::VERSION.load(Ordering::SeqCst);
        if version != seen_version || last.is_none_or(|t| t.elapsed() > Duration::from_secs(900)) {
            let cfg = config::get();
            let w = fetch(&client, &cfg).await;
            hub::publish(&app, "weather", &w);
            // Geocoding saves the config; don't treat our own save as a change.
            seen_version = config::VERSION.load(Ordering::SeqCst);
            last = Some(Instant::now());
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
