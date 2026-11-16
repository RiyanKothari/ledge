//! Your phone, as Phone Link and Bluetooth see it:
//! * battery — the level the phone reports over its Bluetooth hands-free link
//!   (Windows keeps it as a device property; Phone Link itself doesn't store it);
//! * wallpaper — Phone Link's copy of the phone's wallpaper, for the widget;
//! * name — the hands-free device's name.
//! Published on the "phone" channel; notifications come from `notify.rs`.

use crate::hub;
use base64::Engine;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tauri::AppHandle;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT, CM_Get_DevNode_PropertyW, CM_Get_Device_ID_ListW,
    CM_Get_Device_ID_List_SizeW, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, PCWSTR, w};

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    /// Last level each Bluetooth device reported (paired devices keep their last value;
    /// the UI pairs this with the live connection state).
    pub batteries: Vec<DeviceBattery>,
    pub wallpaper: Option<String>,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceBattery {
    pub name: String,
    pub level: u8,
}

const BT_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };
const FRIENDLY_NAME: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

fn prop(dev: u32, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    unsafe {
        let mut ty = DEVPROPTYPE::default();
        let mut size = 0u32;
        let _ = CM_Get_DevNode_PropertyW(dev, key, &mut ty, None, &mut size, 0);
        if size == 0 || size > 4096 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        (CM_Get_DevNode_PropertyW(dev, key, &mut ty, Some(buf.as_mut_ptr()), &mut size, 0) == CR_SUCCESS).then_some(buf)
    }
}

fn utf16(buf: &[u8]) -> String {
    let w: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&w)
}

/// "Sam's Galaxy S24 Hands-Free HF" → "Sam's Galaxy S24"
pub fn phone_name(device: &str) -> String {
    let mut s = device.to_string();
    for suffix in [" Hands-Free HF", " Hands-Free AG", " Hands-Free", " Audio Gateway"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
        }
    }
    s
}

/// (phone name, battery %) for every present Bluetooth device reporting a battery level.
pub fn batteries() -> Vec<(String, u8)> {
    let mut out = Vec::new();
    unsafe {
        let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
        let mut len = 0u32;
        if CM_Get_Device_ID_List_SizeW(&mut len, w!("BTHENUM"), flags) != CR_SUCCESS || len == 0 {
            return out;
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(w!("BTHENUM"), &mut buf, flags) != CR_SUCCESS {
            return out;
        }
        for id in buf.split(|&c| c == 0).filter(|s| !s.is_empty()) {
            let id: Vec<u16> = id.iter().copied().chain(Some(0)).collect();
            let mut dev = 0u32;
            if CM_Locate_DevNodeW(&mut dev, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
                continue;
            }
            let Some(b) = prop(dev, &BT_BATTERY) else { continue };
            let Some(&level) = b.first() else { continue };
            let name = prop(dev, &FRIENDLY_NAME).map(|n| utf16(&n)).unwrap_or_default();
            if level <= 100 {
                out.push((phone_name(&name), level));
            }
        }
    }
    out
}

/// Phone Link's copy of the phone wallpaper, from the most recently used device.
fn wallpaper() -> Option<(PathBuf, SystemTime, String)> {
    let base = dirs::data_local_dir()?.join(r"Packages\Microsoft.YourPhone_8wekyb3d8bbwe\LocalCache\Indexed");
    let newest = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path().join(r"System\Database\deviceData.db"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .max()?;
    let conn = Connection::open_with_flags(&newest.1, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let blob: Vec<u8> = conn.query_row("SELECT blob FROM wallpaper ORDER BY wallpaper_type LIMIT 1", [], |r| r.get(0)).ok()?;
    if blob.is_empty() || blob.len() > 8_000_000 {
        return None;
    }
    let mime = if blob.starts_with(&[0x89, b'P']) { "image/png" } else { "image/jpeg" };
    Some((newest.1, newest.0, format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(blob))))
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-phone".into())
        .spawn(move || {
            let mut wall: Option<(PathBuf, SystemTime, String)> = None;
            let mut tick = 0u64;
            loop {
                if tick % 30 == 0 {
                    // Re-read the wallpaper when Phone Link updates it.
                    if let Some(w) = wallpaper() {
                        if wall.as_ref().map(|o| (&o.0, o.1)) != Some((&w.0, w.1)) {
                            wall = Some(w);
                        }
                    }
                }
                let mut list: Vec<DeviceBattery> = Vec::new();
                for (name, level) in batteries() {
                    if !list.iter().any(|d| d.name == name) {
                        list.push(DeviceBattery { name, level });
                    }
                }
                let state = Phone { batteries: list, wallpaper: wall.as_ref().map(|w| w.2.clone()) };
                hub::publish(&app, "phone", &state);
                tick += 1;
                std::thread::sleep(Duration::from_secs(10));
            }
        })
        .expect("spawn phone watcher");
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_profile_suffixes() {
        assert_eq!(super::phone_name("Sam's Galaxy S24 Hands-Free HF"), "Sam's Galaxy S24");
        assert_eq!(super::phone_name("Pixel 9"), "Pixel 9");
    }

    /// Read-only: `cargo test --lib live_phone -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_phone_battery() {
        println!("batteries: {:?}", super::batteries());
        println!("wallpaper bytes: {:?}", super::wallpaper().map(|w| w.2.len()));
    }
}
