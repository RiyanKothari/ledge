//! Tracks open app windows the way the taskbar does, without polling:
//! WinEvent hooks mark the list dirty, a 150 ms timer re-reads it, and the
//! UI only hears about real changes. Also does activate / minimize / close.

use crate::{hub, taskbar};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::AppHandle;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, MAX_PATH, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, EnumChildWindows, EnumWindows, GA_ROOT, GW_OWNER,
    GWL_EXSTYLE, GetAncestor, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, MSG,
    OBJID_WINDOW, PostMessageW, SW_MINIMIZE, SW_RESTORE, SetForegroundWindow, SetTimer, ShowWindowAsync,
    TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_CLOSE, WM_TIMER, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WinInfo {
    pub hwnd: i64,
    pub title: String,
    pub exe: String,
    pub aumid: String,
    /// Grouping key: AUMID when the app has one, else the exe path (lower-case).
    pub key: String,
    pub minimized: bool,
    pub active: bool,
    /// First-seen order, so buttons don't jump around as focus changes.
    pub seq: u64,
}

static DIRTY: AtomicBool = AtomicBool::new(true);
static LAST_FG: AtomicIsize = AtomicIsize::new(0);
static SEQ: AtomicU64 = AtomicU64::new(1);
/// hwnd -> (pid, exe, aumid, seq). Cleared of dead windows on each pass.
static IDENTITY: LazyLock<Mutex<HashMap<isize, (u32, String, String, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn own_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title_of(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

pub(crate) fn exe_of(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; MAX_PATH as usize * 2];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

fn aumid_of(h: HWND) -> String {
    unsafe {
        SHGetPropertyStoreForWindow::<IPropertyStore>(h)
            .and_then(|s| s.GetValue(&PKEY_AppUserModel_ID))
            .map(|v| v.to_string())
            .unwrap_or_default()
    }
}

fn is_cloaked(h: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, size_of::<u32>() as u32).is_ok()
            && cloaked != 0
    }
}

/// Same rules the taskbar uses to decide what gets a button.
fn is_app_window(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || is_cloaked(h) {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let app = ex & WS_EX_APPWINDOW.0 != 0;
        if !app && (ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0) {
            return false;
        }
        let owner = GetWindow(h, GW_OWNER).unwrap_or_default();
        if !app && !owner.is_invalid() {
            return false;
        }
        // Our own dock is a tool window (excluded above); our Settings window
        // is a normal window and gets a button like any other app.
        if GetWindowTextLengthW(h) == 0 {
            return false;
        }
        if pid_of(h) == own_pid() && !title_of(h).starts_with("Ledge settings") {
            return false; // the dock's own panels and banners
        }
        !matches!(
            class_of(h).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
        )
    }
}

/// UWP apps live in ApplicationFrameHost; the real process owns a child window.
fn uwp_child_pid(frame: HWND) -> Option<u32> {
    struct Find {
        frame_pid: u32,
        found: u32,
    }
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
        let f = unsafe { &mut *(lp.0 as *mut Find) };
        let pid = pid_of(h);
        if pid != f.frame_pid {
            f.found = pid;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut f = Find { frame_pid: pid_of(frame), found: 0 };
    unsafe {
        let _ = EnumChildWindows(Some(frame), Some(cb), LPARAM(&mut f as *mut Find as isize));
    }
    (f.found != 0).then_some(f.found)
}

pub fn enumerate() -> Vec<WinInfo> {
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lp.0 as *mut Vec<HWND>) };
        if is_app_window(h) {
            v.push(h);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<HWND> as isize));
    }
    let fg = unsafe { GetForegroundWindow() };
    if !fg.is_invalid() && pid_of(fg) != own_pid() {
        LAST_FG.store(fg.0 as isize, Ordering::SeqCst);
    }
    let active = LAST_FG.load(Ordering::SeqCst);

    let mut ids = IDENTITY.lock().unwrap_or_else(|e| e.into_inner());
    ids.retain(|&k, _| unsafe { IsWindow(Some(HWND(k as *mut _))).as_bool() });
    let mut out: Vec<WinInfo> = hwnds
        .into_iter()
        .map(|h| {
            let pid = pid_of(h);
            let entry = ids.entry(h.0 as isize).or_insert_with(|| (0, String::new(), String::new(), 0));
            if entry.0 != pid || entry.1.is_empty() {
                let mut exe = exe_of(pid);
                if exe.to_lowercase().ends_with("applicationframehost.exe") {
                    if let Some(child) = uwp_child_pid(h) {
                        exe = exe_of(child);
                    }
                }
                let seq = if entry.3 == 0 { SEQ.fetch_add(1, Ordering::SeqCst) } else { entry.3 };
                *entry = (pid, exe, aumid_of(h), seq);
            }
            let (_, exe, aumid, seq) = entry.clone();
            let key = if aumid.is_empty() { exe.to_lowercase() } else { aumid.to_lowercase() };
            WinInfo {
                hwnd: h.0 as i64,
                title: title_of(h),
                exe,
                aumid,
                key,
                minimized: unsafe { IsIconic(h).as_bool() },
                active: h.0 as isize == active,
                seq,
            }
        })
        .collect();
    out.sort_by_key(|w| w.seq);
    out
}

// ---------- actions ----------

fn hwnd(raw: i64) -> HWND {
    HWND(raw as isize as *mut _)
}

fn bring_forward(h: HWND) {
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindowAsync(h, SW_RESTORE);
        }
        if !SetForegroundWindow(h).as_bool() {
            // Borrow the current foreground thread's input state, then retry.
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let _ = AttachThreadInput(me, fg_thread, true);
            let _ = SetForegroundWindow(h);
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
}

/// Taskbar click semantics for a group of windows of one app:
/// one window -> focus it, or minimize it if it is already in front;
/// several -> cycle through them.
pub fn click_group(raw: &[i64]) {
    if raw.is_empty() {
        return;
    }
    let last = LAST_FG.load(Ordering::SeqCst) as i64;
    if raw.len() == 1 {
        let h = hwnd(raw[0]);
        unsafe {
            if raw[0] == last && !IsIconic(h).as_bool() {
                let _ = ShowWindowAsync(h, SW_MINIMIZE);
                return;
            }
        }
        bring_forward(h);
        return;
    }
    let next = raw.iter().position(|&r| r == last).map(|i| (i + 1) % raw.len()).unwrap_or(0);
    bring_forward(hwnd(raw[next]));
}

/// Brings an already-open window of the app with this AUMID to the front.
/// Win32 apps often use their exe's name as the notification AUMID ("Chrome"),
/// so the exe name is matched too.
pub fn focus_app(aumid: &str) -> bool {
    let want = aumid.to_lowercase();
    if want.is_empty() {
        return false;
    }
    let list = enumerate();
    let stem = |exe: &str| {
        std::path::Path::new(exe).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
    };
    let hit = list
        .iter()
        .find(|w| !w.aumid.is_empty() && w.aumid.to_lowercase() == want)
        .or_else(|| list.iter().find(|w| {
            let s = stem(&w.exe);
            s.len() >= 4 && want.contains(&s)
        }));
    match hit {
        Some(w) => {
            bring_forward(hwnd(w.hwnd));
            true
        }
        None => false,
    }
}

pub fn focus(raw: i64) {
    bring_forward(hwnd(raw));
}

pub fn close(raw: i64) {
    unsafe {
        let _ = PostMessageW(Some(hwnd(raw)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn minimize(raw: i64) {
    unsafe {
        let _ = ShowWindowAsync(hwnd(raw), SW_MINIMIZE);
    }
}

// ---------- the watcher thread ----------

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, h: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if event == EVENT_SYSTEM_FOREGROUND {
        if !h.is_invalid() && pid_of(h) != own_pid() {
            LAST_FG.store(h.0 as isize, Ordering::SeqCst);
        }
        DIRTY.store(true, Ordering::SeqCst);
        return;
    }
    if id_object != OBJID_WINDOW.0 || id_child != 0 || h.is_invalid() {
        return;
    }
    if unsafe { GetAncestor(h, GA_ROOT) } == h {
        // Explorer re-showing its taskbar: hide it again right away, not on the next sweep.
        if event == EVENT_OBJECT_SHOW && matches!(class_of(h).as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            taskbar::reassert();
            return;
        }
        DIRTY.store(true, Ordering::SeqCst);
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-winwatch".into())
        .spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
            for (lo, hi) in [
                (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
                (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
                (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            ] {
                let _ = EVENT_OBJECT_DESTROY; // inside CREATE..HIDE
                SetWinEventHook(lo, hi, None, Some(on_event), 0, 0, flags);
            }
            SetTimer(None, 1, 150, None);
            let mut ticks: u64 = 0;
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_TIMER {
                    ticks += 1;
                    // Titles of background windows can change without events
                    // we listen to; a slow safety re-read every ~3 s covers it.
                    if DIRTY.swap(false, Ordering::SeqCst) || ticks % 20 == 0 {
                        hub::publish(&app, "windows", &enumerate());
                    }
                    if ticks % 4 == 0 {
                        taskbar::reassert();
                    }
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn window watcher");
}
