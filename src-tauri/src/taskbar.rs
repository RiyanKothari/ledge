//! Hides the Windows taskbar while Ledge runs and always puts it back.
//!
//! Before touching anything, the taskbar's original auto-hide state is written
//! to %LOCALAPPDATA%\Ledge\taskbar-state.json. It is restored on normal exit,
//! from the panic hook, on the next launch after a crash, and by
//! `ledge --restore-taskbar`. Restarting Explorer also brings the taskbar back.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA, SHAppBarMessage};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, IsWindowVisible, SW_HIDE, SW_SHOW, SW_SHOWNA, SetForegroundWindow, ShowWindow,
};
use windows::core::{PCWSTR, w};

static HIDDEN: AtomicBool = AtomicBool::new(false);
static PEEKING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
struct Saved {
    /// ABM_GETSTATE bits before Ledge changed them.
    state: u32,
}

fn state_path() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Ledge").join("taskbar-state.json")
}

/// Explorer's taskbar. Ledge has a hidden "Shell_TrayWnd" of its own (the tray
/// host in tray.rs), so the search skips this process's windows.
pub fn explorer_tray() -> Option<HWND> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        let me = GetCurrentProcessId();
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_TrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid != me {
                return Some(h);
            }
            after = Some(h);
        }
        None
    }
}

fn tray() -> Option<HWND> {
    explorer_tray()
}

/// The main taskbar plus the taskbars on other monitors.
fn all_trays() -> Vec<HWND> {
    let mut v: Vec<HWND> = tray().into_iter().collect();
    unsafe {
        let mut after: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            v.push(h);
            after = Some(h);
        }
    }
    v
}

fn appbar_state(tray: HWND) -> u32 {
    let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: tray, ..Default::default() };
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32 }
}

fn set_appbar_state(tray: HWND, state: u32) {
    let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: tray, ..Default::default() };
    abd.lParam = windows::Win32::Foundation::LPARAM(state as isize);
    unsafe { SHAppBarMessage(ABM_SETSTATE, &mut abd) };
}

/// Auto-hide releases the taskbar's reserved strip; SW_HIDE stops it popping up.
pub fn hide() {
    let Some(t) = tray() else { return };
    let path = state_path();
    if !path.exists() {
        let saved = Saved { state: appbar_state(t) };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, serde_json::to_string(&saved).unwrap_or_default()).is_err() {
            return; // can't promise a restore, so don't hide
        }
    }
    set_appbar_state(t, ABS_AUTOHIDE);
    for h in all_trays() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
    HIDDEN.store(true, Ordering::SeqCst);
}

/// Explorer re-shows the taskbar on some events; call this periodically.
pub fn reassert() {
    if !HIDDEN.load(Ordering::SeqCst) || PEEKING.load(Ordering::SeqCst) {
        return;
    }
    for t in all_trays() {
        unsafe {
            if IsWindowVisible(t).as_bool() {
                let _ = ShowWindow(t, SW_HIDE);
            }
        }
    }
}

/// Shows the real taskbar for a moment (system tray access) or hides it again.
pub fn peek(on: bool) {
    if !HIDDEN.load(Ordering::SeqCst) {
        return;
    }
    PEEKING.store(on, Ordering::SeqCst);
    if let Some(t) = tray() {
        unsafe {
            if on {
                let _ = ShowWindow(t, SW_SHOW);
                let _ = SetForegroundWindow(t);
            } else {
                let _ = ShowWindow(t, SW_HIDE);
            }
        }
    }
}

pub fn is_peeking() -> bool {
    PEEKING.load(Ordering::SeqCst)
}

/// Puts the taskbar back exactly as it was. Safe to call any number of times.
pub fn restore() {
    let path = state_path();
    let saved: Option<Saved> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok());
    if saved.is_none() && !HIDDEN.load(Ordering::SeqCst) {
        return;
    }
    if let Some(t) = tray() {
        if let Some(s) = &saved {
            set_appbar_state(t, s.state);
        }
    }
    for t in all_trays() {
        unsafe {
            let _ = ShowWindow(t, SW_SHOWNA);
        }
    }
    let _ = std::fs::remove_file(&path);
    HIDDEN.store(false, Ordering::SeqCst);
    PEEKING.store(false, Ordering::SeqCst);
}

/// Last-resort: make the taskbar visible even with no saved state.
pub fn show_now() {
    for t in all_trays() {
        unsafe {
            let _ = ShowWindow(t, SW_SHOWNA);
        }
    }
}

/// A state file at startup means the last run never restored: do it now.
pub fn recover_after_crash() {
    if state_path().exists() {
        restore();
    }
}
