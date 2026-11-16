//! The last value of each live channel ("system", "media", "ai", ...).
//! Background workers publish here; a window that mounts later asks for
//! the latest value instead of waiting for the next tick.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};

static LAST: LazyLock<Mutex<HashMap<String, Value>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn publish<T: Serialize>(app: &AppHandle, channel: &str, value: &T) {
    let Ok(v) = serde_json::to_value(value) else { return };
    {
        let mut map = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(channel) == Some(&v) {
            return; // unchanged: don't wake the UI
        }
        map.insert(channel.to_string(), v.clone());
    }
    let _ = app.emit(channel, v);
}

pub fn last(channel: &str) -> Option<Value> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).get(channel).cloned()
}
