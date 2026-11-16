//! Wi-Fi through the native WLAN API: current connection, scan, connect
//! (saved profile, open network, or WPA2/WPA3 with a password the user types
//! in Ledge), disconnect. The radio itself is switched in `radios.rs`.
//!
//! Note: Windows 11 only reports network names to desktop apps when
//! Settings → Privacy → Location → "Let desktop apps access your location" is on.

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::NetworkManagement::WiFi::*;
use windows::core::{GUID, HSTRING, PCWSTR};

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WifiNow {
    pub present: bool,
    pub connected: bool,
    pub ssid: String,
    /// 0-100
    pub signal: u32,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub ssid: String,
    pub signal: u32,
    pub secure: bool,
    pub saved: bool,
    pub connected: bool,
    /// "open" | "wpa2" | "wpa3" | "enterprise" | "other"
    pub auth: String,
}

struct Client {
    h: HANDLE,
    iface: GUID,
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.h, None);
        }
    }
}

fn open() -> Option<Client> {
    unsafe {
        let (mut ver, mut h) = (0u32, HANDLE::default());
        if WlanOpenHandle(2, None, &mut ver, &mut h) != ERROR_SUCCESS.0 {
            return None;
        }
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(h, None, &mut list) != ERROR_SUCCESS.0 || list.is_null() {
            WlanCloseHandle(h, None);
            return None;
        }
        let l = &*list;
        let iface = (l.dwNumberOfItems > 0).then(|| l.InterfaceInfo[0].InterfaceGuid);
        WlanFreeMemory(list as *const _);
        match iface {
            Some(iface) => Some(Client { h, iface }),
            None => {
                WlanCloseHandle(h, None);
                None
            }
        }
    }
}

fn ssid_str(s: &DOT11_SSID) -> String {
    let n = (s.uSSIDLength as usize).min(32);
    String::from_utf8_lossy(&s.ucSSID[..n]).into_owned()
}

fn wide(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

pub fn now() -> WifiNow {
    let Some(c) = open() else { return WifiNow::default() };
    let mut out = WifiNow { present: true, ..Default::default() };
    unsafe {
        let (mut size, mut data) = (0u32, std::ptr::null_mut());
        let r = WlanQueryInterface(c.h, &c.iface, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None);
        if r == ERROR_SUCCESS.0 && !data.is_null() {
            let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            if a.isState == wlan_interface_state_connected {
                out.connected = true;
                out.ssid = ssid_str(&a.wlanAssociationAttributes.dot11Ssid);
                out.signal = a.wlanAssociationAttributes.wlanSignalQuality;
            }
            WlanFreeMemory(data);
        }
    }
    out
}

fn auth_name(a: DOT11_AUTH_ALGORITHM, secure: bool) -> &'static str {
    match a {
        DOT11_AUTH_ALGO_80211_OPEN if !secure => "open",
        DOT11_AUTH_ALGO_RSNA_PSK | DOT11_AUTH_ALGO_WPA_PSK => "wpa2",
        DOT11_AUTH_ALGO_WPA3_SAE => "wpa3",
        DOT11_AUTH_ALGO_RSNA | DOT11_AUTH_ALGO_WPA | DOT11_AUTH_ALGO_WPA3_ENT | DOT11_AUTH_ALGO_WPA3_ENT_192 => "enterprise",
        _ => "other",
    }
}

/// Asks the adapter for a fresh scan, waits for it, and returns one row per SSID.
pub fn scan() -> Vec<Network> {
    let Some(c) = open() else { return Vec::new() };
    unsafe {
        let _ = WlanScan(c.h, &c.iface, None, None, None);
    }
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let mut out: Vec<Network> = Vec::new();
    unsafe {
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        if WlanGetAvailableNetworkList(c.h, &c.iface, 0, None, &mut list) != ERROR_SUCCESS.0 || list.is_null() {
            return out;
        }
        let l = &*list;
        let rows = std::slice::from_raw_parts(l.Network.as_ptr(), l.dwNumberOfItems as usize);
        for n in rows {
            let ssid = ssid_str(&n.dot11Ssid);
            if ssid.is_empty() {
                continue; // hidden network
            }
            let secure = n.bSecurityEnabled.as_bool();
            let row = Network {
                ssid: ssid.clone(),
                signal: n.wlanSignalQuality,
                secure,
                saved: n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
                connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                auth: auth_name(n.dot11DefaultAuthAlgorithm, secure).into(),
            };
            // The same SSID shows up once per profile and once without: merge.
            match out.iter_mut().find(|o| o.ssid == ssid) {
                Some(o) => {
                    o.saved |= row.saved;
                    o.connected |= row.connected;
                    o.signal = o.signal.max(row.signal);
                }
                None => out.push(row),
            }
        }
        WlanFreeMemory(list as *const _);
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.saved.cmp(&a.saved)).then(b.signal.cmp(&a.signal)));
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// WLAN profile XML for a personal network. The password goes only to Windows.
pub fn profile_xml(ssid: &str, auth: &str, password: Option<&str>) -> Option<String> {
    let name = xml_escape(ssid);
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    let security = match auth {
        "open" => "<authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption>".to_string(),
        "wpa2" | "wpa3" => {
            let pw = password?;
            if pw.len() < 8 || pw.len() > 63 {
                return None;
            }
            let a = if auth == "wpa3" { "WPA3SAE" } else { "WPA2PSK" };
            format!(
                "<authEncryption><authentication>{a}</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption><sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(pw)
            )
        }
        _ => return None,
    };
    Some(format!(
        r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>{name}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security>{security}</security></MSM></WLANProfile>"#
    ))
}

/// Connects to `ssid`. Uses the saved profile when there is one; otherwise
/// creates one from `password` (or none for open networks).
pub fn connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let c = open().ok_or("No Wi-Fi adapter")?;
    let net = scan().into_iter().find(|n| n.ssid == ssid).ok_or("Network not in range")?;
    unsafe {
        if !net.saved || password.is_some() {
            let xml = profile_xml(ssid, &net.auth, password).ok_or(match net.auth.as_str() {
                "enterprise" => "Work/school networks need Windows settings",
                "open" | "wpa2" | "wpa3" => "Password must be 8–63 characters",
                _ => "This network type isn't supported",
            })?;
            let mut reason = 0u32;
            let r = WlanSetProfile(c.h, &c.iface, 0, &HSTRING::from(xml), PCWSTR::null(), true, None, &mut reason);
            if r != ERROR_SUCCESS.0 {
                return Err(format!("Windows rejected the network profile ({r}/{reason})"));
            }
        }
        let profile = HSTRING::from(ssid);
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(profile.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let r = WlanConnect(c.h, &c.iface, &params, None);
        if r != ERROR_SUCCESS.0 {
            return Err(format!("Could not connect ({r})"));
        }
    }
    // Connecting is asynchronous: wait up to ~10 s for it to land.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let n = now();
        if n.connected && n.ssid == ssid {
            return Ok(());
        }
    }
    Err("Didn't connect — check the password".into())
}

pub fn disconnect() -> bool {
    let Some(c) = open() else { return false };
    unsafe { WlanDisconnect(c.h, &c.iface, None) == ERROR_SUCCESS.0 }
}

pub fn forget(ssid: &str) -> bool {
    let Some(c) = open() else { return false };
    unsafe { WlanDeleteProfile(c.h, &c.iface, &HSTRING::from(ssid), None) == ERROR_SUCCESS.0 }
}

#[allow(dead_code)]
fn _profile_name(n: &WLAN_AVAILABLE_NETWORK) -> String {
    wide(&n.strProfileName)
}

#[cfg(test)]
mod tests {
    use super::profile_xml;

    #[test]
    fn profile_escapes_and_validates() {
        let x = profile_xml("Café & <Home>", "wpa2", Some("pa\"ss word")).unwrap();
        assert!(x.contains("<name>Café &amp; &lt;Home&gt;</name>"));
        assert!(x.contains("<hex>436166C3A9202620"));
        assert!(x.contains("WPA2PSK") && x.contains("pa&quot;ss word"));
        assert!(profile_xml("x", "wpa2", Some("short")).is_none());
        assert!(profile_xml("x", "wpa3", Some("longenough")).unwrap().contains("WPA3SAE"));
        assert!(profile_xml("x", "open", None).unwrap().contains("<encryption>none"));
        assert!(profile_xml("x", "enterprise", Some("whatever1")).is_none());
    }
}
