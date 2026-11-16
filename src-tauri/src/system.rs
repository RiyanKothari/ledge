//! CPU, memory, battery, network throughput and master volume, straight from
//! Win32 every 1.5 s (no extra crates, no WMI).

use crate::hub;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub level: f32,
    pub muted: bool,
}

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    pub cpu: u8,
    pub ram: u8,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub battery: Option<Battery>,
    /// Bytes per second.
    pub net_down: u64,
    pub net_up: u64,
    /// "wifi", "ethernet" or "none".
    pub net_kind: String,
    pub volume: Option<Volume>,
    /// Apps using the most memory (all processes of one exe summed).
    pub top_ram: Vec<ProcMem>,
}

#[derive(Serialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcMem {
    pub name: String,
    pub exe: String,
    pub mb: u64,
}

/// Working set per executable, biggest first.
fn top_ram(limit: usize) -> Vec<ProcMem> {
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::ProcessStatus::{EnumProcesses, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;
    let mut pids = vec![0u32; 4096];
    let mut needed = 0u32;
    if unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed) }.is_err() {
        return Vec::new();
    }
    pids.truncate(needed as usize / 4);
    let mut by_exe: std::collections::HashMap<String, (String, u64)> = std::collections::HashMap::new();
    for pid in pids.into_iter().filter(|&p| p > 4) {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid) else { continue };
            let mut pmc = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            let mem = GetProcessMemoryInfo(h, &mut pmc, pmc.cb).is_ok().then_some(pmc.WorkingSetSize as u64);
            let mut buf = [0u16; MAX_PATH as usize * 2];
            let mut len = buf.len() as u32;
            let path = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok()
                .then(|| String::from_utf16_lossy(&buf[..len as usize]));
            let _ = CloseHandle(h);
            let (Some(mem), Some(path)) = (mem, path) else { continue };
            let e = by_exe.entry(path.to_lowercase()).or_insert((path, 0));
            e.1 += mem;
        }
    }
    let mut v: Vec<ProcMem> = by_exe
        .into_values()
        .map(|(exe, bytes)| {
            let stem = exe.rsplit('\\').next().unwrap_or(&exe).trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
            let mut c = stem.chars();
            let name = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
            ProcMem { name, exe, mb: bytes / (1024 * 1024) }
        })
        .filter(|p| !matches!(p.name.to_lowercase().as_str(), "memory compression" | "system" | "registry" | "vmmem" | "secure system"))
        .collect();
    v.sort_by(|a, b| b.mb.cmp(&a.mb));
    v.truncate(limit);
    v
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };
    // Kernel time includes idle time.
    Some((ft(idle), ft(kernel) + ft(user)))
}

fn memory() -> (u8, f32, f32) {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return (0, 0.0, 0.0);
    }
    let gb = 1024.0 * 1024.0 * 1024.0;
    let total = m.ullTotalPhys as f32 / gb;
    let used = (m.ullTotalPhys - m.ullAvailPhys) as f32 / gb;
    (m.dwMemoryLoad as u8, used, total)
}

fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    // 128 = no system battery, 255 = unknown.
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { percent: s.BatteryLifePercent, charging: s.ACLineStatus == 1 })
}

/// Total bytes in/out on physical interfaces that are up, plus the link kind.
fn net_totals() -> (u64, u64, String) {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0, "none".into());
        }
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        let (mut down, mut up) = (0u64, 0u64);
        let mut kind = "none";
        for r in rows {
            let hardware = r.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            if !hardware || r.Type == 24 || r.OperStatus != IfOperStatusUp {
                continue;
            }
            down += r.InOctets;
            up += r.OutOctets;
            kind = match (kind, r.Type) {
                (_, 71) => "wifi",
                ("none", 6) => "ethernet",
                (k, _) => k,
            };
        }
        FreeMibTable(table as *const _);
        (down, up, kind.into())
    }
}

fn volume() -> Option<Volume> {
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let v: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        Some(Volume { level: v.GetMasterVolumeLevelScalar().ok()?, muted: v.GetMute().ok()?.as_bool() })
    }
}

pub fn set_volume(level: Option<f32>, toggle_mute: bool) -> Option<Volume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = e.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let v: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        if let Some(l) = level {
            v.SetMasterVolumeLevelScalar(l.clamp(0.0, 1.0), std::ptr::null()).ok()?;
            if v.GetMute().ok()?.as_bool() && l > 0.0 {
                v.SetMute(false, std::ptr::null()).ok()?;
            }
        }
        if toggle_mute {
            let m = v.GetMute().ok()?.as_bool();
            v.SetMute(!m, std::ptr::null()).ok()?;
        }
        Some(Volume { level: v.GetMasterVolumeLevelScalar().ok()?, muted: v.GetMute().ok()?.as_bool() })
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-system".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut prev_cpu = cpu_times();
            let (mut prev_down, mut prev_up, _) = net_totals();
            let mut prev_at = Instant::now();
            let mut tick = 0u64;
            let mut top = Vec::new();
            let mut prev_charging: Option<bool> = None;
            loop {
                // The process list is the costly part: refresh it every ~12 s.
                if tick % 8 == 0 {
                    top = top_ram(6);
                }
                tick += 1;
                std::thread::sleep(Duration::from_millis(if crate::appbar::on_screen() { 1500 } else { 4000 }));
                let now_cpu = cpu_times();
                let cpu = match (prev_cpu, now_cpu) {
                    (Some((i0, t0)), Some((i1, t1))) if t1 > t0 => {
                        let busy = 1.0 - (i1 - i0) as f64 / (t1 - t0) as f64;
                        (busy * 100.0).round().clamp(0.0, 100.0) as u8
                    }
                    _ => 0,
                };
                prev_cpu = now_cpu;
                let (down, up, kind) = net_totals();
                let secs = prev_at.elapsed().as_secs_f64().max(0.001);
                prev_at = Instant::now();
                let rate = |now: u64, before: u64| (now.saturating_sub(before) as f64 / secs) as u64;
                let stats = SystemStats {
                    cpu,
                    ram: memory().0,
                    ram_used_gb: memory().1,
                    ram_total_gb: memory().2,
                    battery: battery(),
                    net_down: rate(down, prev_down),
                    net_up: rate(up, prev_up),
                    net_kind: kind,
                    volume: volume(),
                    top_ram: top.clone(),
                };
                prev_down = down;
                prev_up = up;
                // Plugged in: a "Charging" banner, like a phone.
                let charging = stats.battery.as_ref().map(|b| b.charging);
                if let (Some(false), Some(true)) = (prev_charging, charging) {
                    if !crate::appbar::is_fullscreen_app() {
                        use tauri::Emitter;
                        let pct = stats.battery.as_ref().map(|b| b.percent).unwrap_or(0);
                        let at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        let _ = app.emit(
                            "toast",
                            serde_json::json!({
                                "id": -at, "order": 0, "aumid": "", "app": "Battery", "title": "Charging",
                                "body": format!("{pct}%"), "at": at, "image": null, "whatsapp": false,
                                "kind": "charging", "level": pct
                            }),
                        );
                    }
                }
                prev_charging = charging.or(prev_charging);
                hub::publish(&app, "system", &stats);
            }
        })
        .expect("spawn system monitor");
}
