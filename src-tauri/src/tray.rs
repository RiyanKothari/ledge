//! The notification area ("system tray"): icons of apps running in the
//! background, shown in Ledge's clock panel.
//!
//! Apps add their icons with Shell_NotifyIcon, which sends them to the first
//! window of class "Shell_TrayWnd". Ledge keeps a hidden window of that class
//! above Explorer's, copies every icon it sees, and passes every message on to
//! Explorer unchanged. Explorer's own tray therefore stays complete, and nothing
//! is lost if Ledge stops. Clicking an icon in Ledge sends the app the same
//! callback message Explorer would.

use crate::hub;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Duration;
use tauri::AppHandle;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetCursorPos,
    GetMessageW, GetWindowThreadProcessId, HICON, HWND_BROADCAST, HWND_TOPMOST, InSendMessage, IsWindow, MSG,
    PostMessageW, RegisterClassExW, RegisterWindowMessageW, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SendMessageTimeoutW,
    SendNotifyMessageW, SetTimer, SetWindowPos, TranslateMessage, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSEXW,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{GUID, PCWSTR, w};

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_STATE: u32 = 0x8;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x1;
const NIN_SELECT: u32 = WM_USER;

/// NOTIFYICONDATA as the shell sends it between processes: handles are 32-bit.
#[repr(C)]
#[derive(Clone, Copy)]
struct Nid32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    hicon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: GUID,
    balloon_icon: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrayData {
    signature: u32,
    message: u32,
    nid: Nid32,
}

#[derive(Clone)]
struct Item {
    hwnd: isize,
    uid: u32,
    guid: Option<GUID>,
    callback: u32,
    version: u32,
    tip: String,
    icon: Option<String>,
    hidden: bool,
    pid: u32,
    exe: String,
}

impl Item {
    fn id(&self) -> String {
        match self.guid {
            Some(g) => format!("{g:?}"),
            None => format!("{:x}:{}", self.hwnd, self.uid),
        }
    }
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrayIcon {
    pub id: String,
    pub tip: String,
    pub icon: String,
    pub exe: String,
    /// Ledge can quit this app (not Windows itself or Explorer).
    pub closable: bool,
}

/// Windows' own processes (Explorer hosts several tray icons) are never quit.
fn closable(exe: &str, pid: u32) -> bool {
    let e = exe.to_lowercase();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()).to_lowercase();
    pid != 0 && pid != std::process::id() && !e.is_empty() && !e.starts_with(&windir) && !e.ends_with("\\explorer.exe")
}

static ITEMS: Mutex<Vec<Item>> = Mutex::new(Vec::new());
static DIRTY: AtomicBool = AtomicBool::new(true);
static OURS: AtomicIsize = AtomicIsize::new(0);

fn wide(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

/// 32-bit handle from another process, widened the way Windows does (sign-extended).
fn handle(v: u32) -> isize {
    v as i32 as isize
}

fn apply(d: &TrayData) {
    let n = &d.nid;
    let guid = (n.flags & NIF_GUID != 0 && n.guid != GUID::zeroed()).then_some(n.guid);
    let hwnd = handle(n.hwnd);
    let mut items = ITEMS.lock().unwrap_or_else(|e| e.into_inner());
    let pos = items.iter().position(|i| match guid {
        Some(g) => i.guid == Some(g),
        None => i.guid.is_none() && i.hwnd == hwnd && i.uid == n.uid,
    });
    match d.message {
        NIM_ADD | NIM_MODIFY => {
            let i = match pos {
                Some(p) => p,
                None => {
                    let mut pid = 0u32;
                    unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
                    let exe = crate::winwatch::exe_of(pid);
                    items.push(Item {
                        hwnd,
                        uid: n.uid,
                        guid,
                        callback: 0,
                        version: 0,
                        tip: String::new(),
                        icon: None,
                        hidden: false,
                        pid,
                        exe,
                    });
                    items.len() - 1
                }
            };
            let it = &mut items[i];
            it.hwnd = hwnd;
            if n.flags & NIF_MESSAGE != 0 {
                it.callback = n.callback;
            }
            if n.flags & NIF_ICON != 0 {
                // Copy now: apps often destroy the handle as soon as we return.
                it.icon = if n.hicon == 0 { None } else { unsafe { crate::shell::hicon_data_url(HICON(handle(n.hicon) as *mut _)) } };
            }
            if n.flags & NIF_TIP != 0 {
                it.tip = wide(&n.tip);
            }
            if n.flags & NIF_STATE != 0 && n.state_mask & NIS_HIDDEN != 0 {
                it.hidden = n.state & NIS_HIDDEN != 0;
            }
        }
        NIM_DELETE => {
            if let Some(p) = pos {
                items.remove(p);
            }
        }
        NIM_SETVERSION => {
            if let Some(p) = pos {
                items[p].version = n.version;
            }
        }
        _ => return,
    }
    DIRTY.store(true, Ordering::SeqCst);
}

unsafe fn forward(msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(t) = crate::taskbar::explorer_tray() else { return LRESULT(0) };
    unsafe {
        if msg == WM_COPYDATA || InSendMessage().as_bool() {
            let mut r = 0usize;
            let _ = SendMessageTimeoutW(t, msg, wp, lp, SMTO_ABORTIFHUNG, 5000, Some(&mut r));
            LRESULT(r as isize)
        } else {
            let _ = PostMessageW(Some(t), msg, wp, lp);
            LRESULT(0)
        }
    }
}

fn keep_on_top(h: HWND) {
    unsafe {
        let first = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
        if first != h {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE);
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COPYDATA => {
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData == 1 && !cds.lpData.is_null() && cds.cbData as usize >= 8 + 24 {
                    let mut d: TrayData = std::mem::zeroed();
                    let n = (cds.cbData as usize).min(size_of::<TrayData>());
                    std::ptr::copy_nonoverlapping(cds.lpData as *const u8, &mut d as *mut _ as *mut u8, n);
                    apply(&d);
                    let r = forward(msg, wp, lp);
                    // Explorer may already know an icon we're only now seeing.
                    return if r.0 != 0 { r } else { LRESULT(1) };
                }
                // App bars (dwData 0), icon positions (3) and the rest: Explorer's business.
                forward(msg, wp, lp)
            }
            WM_TIMER => {
                keep_on_top(h);
                LRESULT(0)
            }
            m if m == WM_COMMAND || (WM_USER..0xC000).contains(&m) => forward(msg, wp, lp),
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }
}

/// Starts the tray host and its publisher.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ledge-tray".into())
        .spawn(|| unsafe {
            let Ok(inst) = GetModuleHandleW(None) else { return };
            let class = w!("Shell_TrayWnd");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: inst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return;
            }
            let (mon, scale) = crate::appbar::monitor();
            let strip = (48.0 * scale) as i32;
            let Ok(h) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class,
                w!(""),
                WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                mon.left,
                mon.bottom - strip,
                mon.right - mon.left,
                strip,
                None,
                None,
                Some(inst.into()),
                None,
            ) else {
                return;
            };
            OURS.store(h.0 as isize, Ordering::SeqCst);
            keep_on_top(h);
            let _ = SetTimer(Some(h), 1, 500, None);
            // Ask every app to add its icon again — this time Ledge sees it too.
            let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = SendNotifyMessageW(HWND_BROADCAST, msg, WPARAM(0), LPARAM(0));
            let mut m = MSG::default();
            while GetMessageW(&mut m, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        })
        .expect("spawn tray host");

    std::thread::Builder::new()
        .name("ledge-tray-publish".into())
        .spawn(move || {
            let mut last: Vec<TrayIcon> = Vec::new();
            let mut tick = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(600));
                tick += 1;
                if !DIRTY.swap(false, Ordering::SeqCst) && tick % 16 != 0 {
                    continue;
                }
                let list: Vec<TrayIcon> = {
                    let mut items = ITEMS.lock().unwrap_or_else(|e| e.into_inner());
                    // Apps that quit without removing their icon.
                    items.retain(|i| unsafe { IsWindow(Some(HWND(i.hwnd as *mut _))).as_bool() });
                    items
                        .iter()
                        .filter(|i| !i.hidden)
                        .filter_map(|i| {
                            Some(TrayIcon {
                                id: i.id(),
                                tip: i.tip.clone(),
                                icon: i.icon.clone()?,
                                exe: i.exe.clone(),
                                closable: closable(&i.exe, i.pid),
                            })
                        })
                        .collect()
                };
                if list != last {
                    hub::publish(&app, "tray", &list);
                    last = list;
                }
            }
        })
        .expect("spawn tray publisher");
}

/// Quits the app behind a tray icon: asks its windows to close, and ends the
/// process if it's still running a few seconds later (most tray apps only hide
/// when their window is closed). The icon disappears as soon as it's gone.
pub fn quit(id: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, WM_CLOSE};
    let it = ITEMS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|i| i.id() == id).cloned().ok_or("That app has already closed")?;
    if !closable(&it.exe, it.pid) {
        return Err("Windows' own icons can't be quit from here".into());
    }
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> windows::core::BOOL {
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == lp.0 as u32 {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        windows::core::BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(it.pid as isize));
    }
    let pid = it.pid;
    std::thread::spawn(move || unsafe {
        if let Ok(p) = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) {
            if WaitForSingleObject(p, 2500) != WAIT_OBJECT_0 {
                let _ = TerminateProcess(p, 0);
                let _ = WaitForSingleObject(p, 2000);
            }
            let _ = CloseHandle(p);
        }
        ITEMS.lock().unwrap_or_else(|e| e.into_inner()).retain(|i| i.pid != pid);
        DIRTY.store(true, Ordering::SeqCst);
    });
    Ok(())
}

fn mkparam(lo: i32, hi: i32) -> usize {
    ((lo as u16 as u32) | ((hi as u16 as u32) << 16)) as usize
}

/// Clicks a tray icon the way Explorer does: "left", "right" or "double".
pub fn click(id: &str, button: &str) -> bool {
    let Some(it) = ITEMS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|i| i.id() == id).cloned() else {
        return false;
    };
    let hwnd = HWND(it.hwnd as *mut _);
    unsafe {
        if it.callback == 0 || !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        // Lets the app bring its window or menu to the front.
        let _ = AllowSetForegroundWindow(it.pid);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let send = |m: u32| {
            let (wp, lp) = if it.version >= 4 {
                (mkparam(pt.x, pt.y), mkparam(m as i32, it.uid as i32) as isize)
            } else {
                (it.uid as usize, m as isize)
            };
            let _ = SendNotifyMessageW(hwnd, it.callback, WPARAM(wp), LPARAM(lp));
        };
        match button {
            "right" => {
                send(WM_RBUTTONDOWN);
                send(WM_RBUTTONUP);
                if it.version >= 3 {
                    send(WM_CONTEXTMENU);
                }
            }
            "double" => {
                send(WM_LBUTTONDOWN);
                send(WM_LBUTTONUP);
                send(WM_LBUTTONDBLCLK);
                send(WM_LBUTTONUP);
            }
            _ => {
                send(WM_LBUTTONDOWN);
                send(WM_LBUTTONUP);
                if it.version >= 3 {
                    send(NIN_SELECT);
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_shell() {
        // The shell's 32-bit NOTIFYICONDATA is 956 bytes; with the header, 964.
        assert_eq!(size_of::<Nid32>(), 956);
        assert_eq!(size_of::<TrayData>(), 964);
        assert_eq!(mkparam(-1, 2), 0x0002_ffff);
    }

    #[test]
    fn tracks_add_modify_delete() {
        let mut d: TrayData = unsafe { std::mem::zeroed() };
        d.nid.hwnd = 0x1234;
        d.nid.uid = 7;
        d.nid.flags = NIF_MESSAGE | NIF_TIP;
        d.nid.callback = 0x8001;
        for (i, c) in "Sync".encode_utf16().enumerate() {
            d.nid.tip[i] = c;
        }
        d.message = NIM_ADD;
        apply(&d);
        d.message = NIM_SETVERSION;
        d.nid.version = 4;
        apply(&d);
        let it = ITEMS.lock().unwrap().iter().find(|i| i.uid == 7).cloned().unwrap();
        assert_eq!((it.tip.as_str(), it.callback, it.version), ("Sync", 0x8001, 4));
        d.message = NIM_MODIFY;
        d.nid.flags = NIF_STATE;
        d.nid.state = NIS_HIDDEN;
        d.nid.state_mask = NIS_HIDDEN;
        apply(&d);
        assert!(ITEMS.lock().unwrap().iter().find(|i| i.uid == 7).unwrap().hidden);
        d.message = NIM_DELETE;
        apply(&d);
        assert!(!ITEMS.lock().unwrap().iter().any(|i| i.uid == 7));
    }
}
