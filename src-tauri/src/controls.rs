#![allow(non_snake_case)] // COM interface method names mirror the Windows SDK
//! Ledge's own Control Center engine — no Windows flyouts:
//! radios (Wi-Fi, Bluetooth, flight mode), paired Bluetooth devices with
//! connect/disconnect for audio devices, screen brightness, audio output
//! devices, energy saver and night light. Published on the "controls" channel.

use crate::{hub, wifi};
use serde::Serialize;
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice, BluetoothMajorClass};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, IDeviceTopology, IMMDeviceEnumerator, MMDeviceEnumerator, eAll, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoSetProxyBlanket,
    CoTaskMemFree, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, STGM_READ,
};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, PowerGetActiveScheme, PowerReadDCValueIndex, PowerSetActiveScheme, PowerWriteDCValueIndex,
    SYSTEM_POWER_STATUS,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_BINARY, RRF_RT_REG_BINARY, RegGetValueW, RegSetKeyValueW};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE, WbemLocator,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::core::{BSTR, GUID, HRESULT, PCWSTR, w};

// ---------- state ----------

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    /// "audio" | "phone" | "input" | "computer" | "other"
    pub kind: String,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Controls {
    pub wifi: wifi::WifiNow,
    /// None when there is no such radio.
    pub wifi_on: Option<bool>,
    pub bluetooth_on: Option<bool>,
    pub flight: bool,
    pub bt_devices: Vec<BtDevice>,
    pub brightness: Option<u8>,
    pub energy_saver: bool,
    pub on_battery: bool,
    pub night_light: Option<bool>,
    pub outputs: Vec<Output>,
}

// ---------- radios ----------

fn radios() -> Vec<Radio> {
    let run = || -> windows::core::Result<Vec<Radio>> {
        let _ = Radio::RequestAccessAsync()?.join();
        Ok(Radio::GetRadiosAsync()?.join()?.into_iter().collect())
    };
    run().unwrap_or_default()
}

fn radio_on(list: &[Radio], kind: RadioKind) -> Option<bool> {
    let mut found = None;
    for r in list.iter().filter(|r| r.Kind().ok() == Some(kind)) {
        let on = r.State().ok() == Some(RadioState::On);
        found = Some(found.unwrap_or(false) || on);
    }
    found
}

pub fn set_radio(kind: &str, on: bool) -> Result<(), String> {
    let kind = match kind {
        "wifi" => RadioKind::WiFi,
        "bluetooth" => RadioKind::Bluetooth,
        _ => return Err("unknown radio".into()),
    };
    let state = if on { RadioState::On } else { RadioState::Off };
    let mut any = false;
    for r in radios().iter().filter(|r| r.Kind().ok() == Some(kind)) {
        any = true;
        r.SetStateAsync(state).and_then(|op| op.join()).map_err(|e| e.message().to_string())?;
    }
    if any { Ok(()) } else { Err("No such radio on this PC".into()) }
}

/// Radios that were on when flight mode started, restored when it ends.
static BEFORE_FLIGHT: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn set_flight(on: bool) -> Result<(), String> {
    let list = radios();
    if on {
        let mut before = BEFORE_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        before.clear();
        for r in &list {
            if r.State().ok() == Some(RadioState::On) {
                before.push(r.Name().map(|n| n.to_string()).unwrap_or_default());
                let _ = r.SetStateAsync(RadioState::Off).and_then(|op| op.join());
            }
        }
    } else {
        let before = std::mem::take(&mut *BEFORE_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()));
        for r in &list {
            let name = r.Name().map(|n| n.to_string()).unwrap_or_default();
            if before.is_empty() || before.contains(&name) {
                let _ = r.SetStateAsync(RadioState::On).and_then(|op| op.join());
            }
        }
    }
    Ok(())
}

// ---------- bluetooth devices ----------

fn bt_devices() -> Vec<BtDevice> {
    let mut out: Vec<BtDevice> = Vec::new();
    let mut classic = || -> windows::core::Result<()> {
        let sel = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
        for info in DeviceInformation::FindAllAsyncAqsFilter(&sel)?.join()? {
            let Ok(d) = BluetoothDevice::FromIdAsync(&info.Id()?)?.join() else { continue };
            let kind = match d.ClassOfDevice().and_then(|c| c.MajorClass()) {
                Ok(BluetoothMajorClass::AudioVideo) => "audio",
                Ok(BluetoothMajorClass::Phone) => "phone",
                Ok(BluetoothMajorClass::Peripheral) => "input",
                Ok(BluetoothMajorClass::Computer) => "computer",
                _ => "other",
            };
            out.push(BtDevice {
                name: d.Name()?.to_string(),
                connected: d.ConnectionStatus()? == BluetoothConnectionStatus::Connected,
                kind: kind.into(),
            });
        }
        Ok(())
    };
    let _ = classic();
    let le = |out: &mut Vec<BtDevice>| -> windows::core::Result<()> {
        let sel = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?;
        for info in DeviceInformation::FindAllAsyncAqsFilter(&sel)?.join()? {
            let Ok(d) = BluetoothLEDevice::FromIdAsync(&info.Id()?)?.join() else { continue };
            let name = d.Name()?.to_string();
            let connected = d.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
            match out.iter_mut().find(|o| o.name == name) {
                Some(o) => o.connected |= connected,
                None => out.push(BtDevice { name, connected, kind: "other".into() }),
            }
        }
        Ok(())
    };
    let _ = le(&mut out);
    out.retain(|d| !d.name.is_empty());
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };
const PKEY_DEVICEINTERFACE_FRIENDLY_NAME: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x026e516e_b814_414b_83cd_856d6fef4822), pid: 2 };
const KSPROPSETID_BTAUDIO: GUID = GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);
const KSPROPERTY_ONESHOT_RECONNECT: u32 = 0;
const KSPROPERTY_ONESHOT_DISCONNECT: u32 = 1;
const KSPROPERTY_TYPE_GET: u32 = 1;

/// Connects or disconnects a paired Bluetooth *audio* device the way the
/// Windows audio stack does it: a one-shot reconnect/disconnect on the
/// device's kernel-streaming filter. Phones and keyboards connect on their own.
pub fn bt_audio(name: &str, connect: bool) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.message().to_string())?;
        let all = e.EnumAudioEndpoints(eAll, DEVICE_STATE(0xF)).map_err(|e| e.message().to_string())?;
        let want = name.to_lowercase();
        let mut done = std::collections::HashSet::new();
        for i in 0..all.GetCount().unwrap_or(0) {
            let Ok(dev) = all.Item(i) else { continue };
            let Ok(store) = dev.OpenPropertyStore(STGM_READ) else { continue };
            let iface = store.GetValue(&PKEY_DEVICEINTERFACE_FRIENDLY_NAME).map(|v| v.to_string()).unwrap_or_default();
            let full = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).map(|v| v.to_string()).unwrap_or_default();
            if iface.to_lowercase() != want && !full.to_lowercase().contains(&format!("({want})")) {
                continue;
            }
            let Ok(topo) = dev.Activate::<IDeviceTopology>(CLSCTX_ALL, None) else { continue };
            let Ok(conn) = topo.GetConnector(0) else { continue };
            let Ok(filter_id) = conn.GetDeviceIdConnectedTo() else { continue };
            let id = filter_id.to_string().unwrap_or_default();
            CoTaskMemFree(Some(filter_id.0 as *const c_void));
            if !done.insert(id.clone()) {
                continue; // headset = speaker + mic endpoints on one filter
            }
            let Ok(filter) = e.GetDevice(&windows::core::HSTRING::from(id.as_str())) else { continue };
            let Ok(ks) = filter.Activate::<IKsControl>(CLSCTX_ALL, None) else { continue };
            let prop = KSIDENTIFIER {
                Anonymous: KSIDENTIFIER_0 {
                    Anonymous: KSIDENTIFIER_0_0 {
                        Set: KSPROPSETID_BTAUDIO,
                        Id: if connect { KSPROPERTY_ONESHOT_RECONNECT } else { KSPROPERTY_ONESHOT_DISCONNECT },
                        Flags: KSPROPERTY_TYPE_GET,
                    },
                },
            };
            let mut ret = 0u32;
            let _ = ks.KsProperty(&prop, size_of::<KSIDENTIFIER>() as u32, std::ptr::null_mut(), 0, &mut ret);
        }
        if done.is_empty() { Err("Only audio devices can be connected from here".into()) } else { Ok(()) }
    }
}

// ---------- audio outputs ----------

#[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows_core::IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, fmt: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, fmt: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, a: *mut c_void, b: *mut c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, a: *mut i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, key: *const c_void, v: *mut c_void) -> HRESULT;
    fn SetPropertyValue(&self, id: PCWSTR, key: *const c_void, v: *mut c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: u32) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn outputs() -> Vec<Output> {
    unsafe {
        let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return vec![] };
        let default = e
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .and_then(|d| d.GetId())
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                s
            })
            .unwrap_or_default();
        let Ok(list) = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return vec![] };
        (0..list.GetCount().unwrap_or(0))
            .filter_map(|i| {
                let d = list.Item(i).ok()?;
                let p = d.GetId().ok()?;
                let id = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const c_void));
                let name = d.OpenPropertyStore(STGM_READ).ok()?.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok()?.to_string();
                Some(Output { default: id == default, id, name })
            })
            .collect()
    }
}

pub fn set_output(id: &str) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let pc: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(|e| e.message().to_string())?;
        let wid = windows::core::HSTRING::from(id);
        for role in 0..3 {
            pc.SetDefaultEndpoint(PCWSTR(wid.as_ptr()), role).ok().map_err(|e| e.message().to_string())?;
        }
        Ok(())
    }
}

// ---------- brightness (WMI, built-in panels) ----------

fn wmi() -> Option<IWbemServices> {
    unsafe {
        let loc: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let empty = BSTR::new();
        let svc = loc.ConnectServer(&BSTR::from("ROOT\\WMI"), &empty, &empty, &empty, 0, &empty, None).ok()?;
        let _ = CoSetProxyBlanket(&svc, 10, 0, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
        Some(svc)
    }
}

fn first(svc: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
    unsafe {
        let it = svc
            .ExecQuery(&BSTR::from("WQL"), &BSTR::from(query), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)
            .ok()?;
        let mut objs = [None];
        let mut n = 0u32;
        let _ = it.Next(WBEM_INFINITE, &mut objs, &mut n);
        if n == 0 { None } else { objs[0].take() }
    }
}

fn brightness(svc: &IWbemServices) -> Option<u8> {
    let obj = first(svc, "SELECT CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE")?;
    let mut v = VARIANT::default();
    unsafe { obj.Get(w!("CurrentBrightness"), 0, &mut v, None, None).ok()? };
    i32::try_from(&v).ok().map(|b| b.clamp(0, 100) as u8)
}

pub fn set_brightness(pct: u8) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let svc = wmi().ok_or("This display's brightness can't be set from Windows")?;
    let target = first(&svc, "SELECT * FROM WmiMonitorBrightnessMethods WHERE Active=TRUE").ok_or("No built-in display")?;
    unsafe {
        let mut path = VARIANT::default();
        target.Get(w!("__PATH"), 0, &mut path, None, None).map_err(|e| e.message().to_string())?;
        let path = BSTR::try_from(&path).map_err(|e| e.message().to_string())?;
        let mut class = None;
        svc.GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
            .map_err(|e| e.message().to_string())?;
        let class = class.ok_or("WMI class missing")?;
        let mut sig = None;
        class.GetMethod(w!("WmiSetBrightness"), 0, &mut sig, std::ptr::null_mut()).map_err(|e| e.message().to_string())?;
        let input = sig.ok_or("WMI method missing")?.SpawnInstance(0).map_err(|e| e.message().to_string())?;
        input.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0).map_err(|e| e.message().to_string())?;
        input.Put(w!("Brightness"), 0, &VARIANT::from(pct.min(100)), 0).map_err(|e| e.message().to_string())?;
        svc.ExecMethod(&path, &BSTR::from("WmiSetBrightness"), WBEM_GENERIC_FLAG_TYPE(0), None, &input, None, None)
            .map_err(|e| e.message().to_string())
    }
}

// ---------- energy saver ----------

const ENERGY_SAVER_SUBGROUP: GUID = GUID::from_u128(0xde830923_a562_41af_a086_e3a2c6bad2da);
const ENERGY_SAVER_THRESHOLD: GUID = GUID::from_u128(0xe69653ca_cf7f_4f05_aa73_cb833fa90ad4);

fn power_status() -> (bool, bool) {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return (false, false);
    }
    (s.SystemStatusFlag == 1, s.ACLineStatus == 0)
}

/// Energy saver turns on at a battery threshold; 100 % means "now".
/// The user's own threshold is saved first and put back when it's turned off.
pub fn set_energy_saver(on: bool) -> Result<(), String> {
    let saved = crate::config::dir().join("energy-saver-threshold.txt");
    unsafe {
        let mut scheme: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut scheme).is_err() || scheme.is_null() {
            return Err("No active power plan".into());
        }
        let mut current = 0u32;
        PowerReadDCValueIndex(None, Some(scheme), Some(&ENERGY_SAVER_SUBGROUP), Some(&ENERGY_SAVER_THRESHOLD), &mut current);
        let value = if on {
            if current != 100 && !saved.exists() {
                let _ = std::fs::create_dir_all(crate::config::dir());
                let _ = std::fs::write(&saved, current.to_string());
            }
            100
        } else {
            let v = std::fs::read_to_string(&saved).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(20u32);
            let _ = std::fs::remove_file(&saved);
            if v == 100 { 20 } else { v }
        };
        let r = PowerWriteDCValueIndex(None, scheme, Some(&ENERGY_SAVER_SUBGROUP), Some(&ENERGY_SAVER_THRESHOLD), value);
        let _ = PowerSetActiveScheme(None, Some(scheme));
        let _ = LocalFree(Some(HLOCAL(scheme as *mut c_void)));
        if r != 0 { Err(format!("Windows refused ({r})")) } else { Ok(()) }
    }
}

// ---------- night light ----------

const NIGHT_KEY: PCWSTR = w!(
    "Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate"
);

fn read_night() -> Option<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, NIGHT_KEY, w!("Data"), RRF_RT_REG_BINARY, None, None, Some(&mut size)).is_err() {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if RegGetValueW(HKEY_CURRENT_USER, NIGHT_KEY, w!("Data"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut size))
            .is_err()
        {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}

/// Night-light state from the CloudStore blob: byte 18 is 0x15 when on, 0x13 when off.
pub fn night_state(b: &[u8]) -> Option<bool> {
    if b.len() < 25 || b[..4] != [0x43, 0x42, 0x01, 0x00] {
        return None;
    }
    match b[18] {
        0x15 => Some(true),
        0x13 => Some(false),
        _ => None,
    }
}

/// The same edit Windows makes: flip the state byte, add/remove the two
/// "active" bytes after the header, and bump the change counter so the
/// shell notices. Returns None for any blob that doesn't look as expected.
pub fn night_toggle(b: &[u8], on: bool) -> Option<Vec<u8>> {
    let cur = night_state(b)?;
    if cur == on {
        return Some(b.to_vec());
    }
    let mut out = Vec::with_capacity(b.len() + 2);
    out.extend_from_slice(&b[..23]);
    if on {
        out.extend_from_slice(&[0x10, 0x00]);
        out.extend_from_slice(&b[23..]);
        out[18] = 0x15;
    } else {
        if b[23..25] != [0x10, 0x00] {
            return None;
        }
        out.extend_from_slice(&b[25..]);
        out[18] = 0x13;
    }
    for i in 10..15 {
        if out[i] != 0xff {
            out[i] += 1;
            break;
        }
    }
    Some(out)
}

pub fn set_night_light(on: bool) -> Result<(), String> {
    let cur = read_night().ok_or("Night light isn't set up on this PC")?;
    let backup = crate::config::dir().join("nightlight-backup.bin");
    if !backup.exists() {
        let _ = std::fs::create_dir_all(crate::config::dir());
        std::fs::write(&backup, &cur).map_err(|e| e.to_string())?;
    }
    let next = night_toggle(&cur, on).ok_or("Unrecognised night light format; left untouched")?;
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, NIGHT_KEY, w!("Data"), REG_BINARY.0, Some(next.as_ptr() as *const c_void), next.len() as u32)
            .ok()
            .map_err(|e| e.message().to_string())
    }
}

// ---------- the loop ----------

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-controls".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let svc = wmi();
            let mut bt: Vec<BtDevice> = Vec::new();
            let mut tick: u64 = 0;
            loop {
                let list = radios();
                let wifi_on = radio_on(&list, RadioKind::WiFi);
                let bluetooth_on = radio_on(&list, RadioKind::Bluetooth);
                if tick % 4 == 0 || bluetooth_on == Some(true) && bt.is_empty() {
                    bt = if bluetooth_on == Some(true) { bt_devices() } else { Vec::new() };
                }
                let (energy_saver, on_battery) = power_status();
                let state = Controls {
                    wifi: wifi::now(),
                    wifi_on,
                    bluetooth_on,
                    flight: !list.is_empty() && list.iter().all(|r| r.State().ok() != Some(RadioState::On)),
                    bt_devices: bt.clone(),
                    brightness: svc.as_ref().and_then(brightness),
                    energy_saver,
                    on_battery,
                    night_light: read_night().and_then(|b| night_state(&b)),
                    outputs: outputs(),
                };
                hub::publish(&app, "controls", &state);
                tick += 1;
                // Idle (dock tucked away, no panel): check far less often.
                let fast = crate::appbar::on_screen();
                for _ in 0..if fast { 1 } else { 4 } {
                    std::thread::sleep(Duration::from_millis(1500));
                    if !fast && crate::appbar::on_screen() {
                        break;
                    }
                }
            }
        })
        .expect("spawn controls");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn night_light_round_trip_on_real_blob() {
        // Captured from this PC (night light off).
        let off = hex("434201000A0201002A06C6E8BABA062A2B0E1343420100D00A02C614CDB987A2DAAAD1ED0100000000");
        assert_eq!(night_state(&off), Some(false));
        let on = night_toggle(&off, true).unwrap();
        assert_eq!(on.len(), off.len() + 2);
        assert_eq!(night_state(&on), Some(true));
        assert_eq!(&on[23..25], &[0x10, 0x00]);
        assert_ne!(on[10..15], off[10..15], "change counter bumped");
        let back = night_toggle(&on, false).unwrap();
        assert_eq!(night_state(&back), Some(false));
        assert_eq!(back.len(), off.len());
        assert_eq!(back[15..], off[15..]);
        assert!(night_toggle(&[1, 2, 3], true).is_none());
    }
}

#[cfg(test)]
mod live {
    /// Read-only snapshot of this PC. `cargo test --lib live_ -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_controls_snapshot() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let list = super::radios();
        println!("radios: {:?}", list.iter().map(|r| (r.Name().unwrap_or_default().to_string(), r.State().ok().map(|s| s.0))).collect::<Vec<_>>());
        println!("wifi now: {:?}", crate::wifi::now());
        println!("brightness: {:?}", super::wmi().as_ref().and_then(super::brightness));
        println!("outputs: {:?}", super::outputs());
        println!("bt: {:?}", super::bt_devices());
        println!("power (saver, on battery): {:?}", super::power_status());
        println!("night: {:?}", super::read_night().and_then(|b| super::night_state(&b)));
        let nets = crate::wifi::scan();
        println!("scan: {} networks, first: {:?}", nets.len(), nets.iter().take(3).map(|n| (&n.ssid, n.signal, &n.auth, n.saved)).collect::<Vec<_>>());
    }
}

#[cfg(test)]
mod live_write {
    /// Writes brightness = its current value (no visible change) to prove the WMI method path.
    #[test]
    #[ignore]
    fn live_brightness_roundtrip() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let svc = super::wmi().expect("wmi");
        let before = super::brightness(&svc).expect("read");
        super::set_brightness(before).expect("write");
        std::thread::sleep(std::time::Duration::from_millis(400));
        let after = super::brightness(&super::wmi().unwrap()).unwrap();
        println!("brightness before={before} after={after}");
        assert_eq!(before, after);
    }
}
