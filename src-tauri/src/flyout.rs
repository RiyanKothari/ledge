//! Ledge's own pop-up surfaces — real acrylic glass windows, never the
//! Windows flyouts:
//! * "flyout": one panel above the dock (control center, calendar, media,
//!   Claude, portfolio, WhatsApp, system, app launcher). Hides when it loses focus.
//! * "toast": the notification banner, top-right like macOS. Never takes focus.

use crate::appbar;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowLongPtrW,
    SW_HIDE, SetWindowPos, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

const GAP: i32 = 10;
const TOAST_W: i32 = 368;
const TOAST_H: i32 = 84;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Open {
    kind: String,
    arg: Option<String>,
}

/// (kind, when it was hidden) — clicking the widget whose flyout just closed
/// on blur should leave it closed, not reopen it.
static CLOSED: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static OPEN_KIND: Mutex<Option<String>> = Mutex::new(None);

/// Tells WebView2 whether the window is on screen. Hidden web views stop
/// painting and throttle their timers, which keeps Ledge light while idle.
pub fn render(w: &WebviewWindow, on: bool) {
    let _ = w.with_webview(move |pw| unsafe {
        let _ = pw.controller().SetIsVisible(on);
    });
}

fn hwnd(w: &WebviewWindow) -> Option<HWND> {
    w.hwnd().ok().map(|h| HWND(h.0))
}

fn glass(w: &WebviewWindow, no_activate: bool) {
    let Some(h) = hwnd(w) else { return };
    unsafe {
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const _, size_of_val(&pref) as u32);
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let mut add = WS_EX_TOOLWINDOW.0 as isize;
        if no_activate {
            add |= WS_EX_NOACTIVATE.0 as isize;
        }
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | add);
    }
    see_through(h);
}

/// Clear, see-through frosted glass: the classic acrylic blur-behind with a
/// light tint, instead of Windows 11's dense flyout backdrop (which reads as
/// solid grey). Rounded by DWMWCP_ROUND above.
fn see_through(h: HWND) {
    acrylic(h, 0x4C1A1414);
}

/// SetWindowCompositionAttribute acrylic blur-behind; `color` is 0xAABBGGRR.
pub fn acrylic(h: HWND, color: u32) {
    use windows::Win32::Graphics::Dwm::{DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::core::{s, w};
    #[repr(C)]
    struct Accent {
        state: u32,
        flags: u32,
        color: u32,
        animation: u32,
    }
    #[repr(C)]
    struct Data {
        attribute: u32,
        data: *mut Accent,
        size: usize,
    }
    type SetWca = unsafe extern "system" fn(HWND, *mut Data) -> windows::core::BOOL;
    unsafe {
        let none = DWMSBT_NONE;
        let _ = DwmSetWindowAttribute(h, DWMWA_SYSTEMBACKDROP_TYPE, &none as *const _ as *const _, size_of_val(&none) as u32);
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
        let Some(f) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else { return };
        let f: SetWca = std::mem::transmute(f);
        // ACCENT_ENABLE_ACRYLICBLURBEHIND, tint 0xAABBGGRR: a dark, mostly clear pane.
        let mut accent = Accent { state: 4, flags: 2, color, animation: 0 };
        let mut data = Data { attribute: 19, data: &mut accent, size: size_of::<Accent>() };
        let _ = f(h, &mut data);
    }
}

fn builder<'a>(app: &'a AppHandle, label: &'a str, title: &'a str) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .focused(false)
        .theme(Some(tauri::Theme::Dark))
}

/// Creates both windows hidden, so opening one later is instant.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let f = builder(app, "flyout", "Ledge panel").inner_size(360.0, 420.0).build()?;
    glass(&f, false);
    let t = builder(app, "toast", "Ledge notification").inner_size(TOAST_W as f64, TOAST_H as f64).build()?;
    glass(&t, true);
    let p = builder(app, "preview", "Ledge previews").inner_size(400.0, 200.0).build()?;
    glass(&p, true);
    // The voice pill draws its own glass (a rounded pill), so no window backdrop.
    let v = builder(app, "voice", "Ledge voice").inner_size(460.0, 320.0).shadow(false).build()?;
    if let Some(h) = hwnd(&v) {
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize);
        }
    }
    // The flyout stops painting while hidden; the banner and preview pages are
    // static, and toggling their web view's visibility left them blank when
    // they were shown with Win32.
    render(&f, false);
    let _ = (&t, &p);
    Ok(())
}

/// Opens (or toggles closed) the panel `kind`, centred over `anchor_x`
/// (DIPs from the dock's left edge), sized `width`×`height` DIPs.
pub fn open(app: &AppHandle, kind: String, arg: Option<String>, anchor_x: f64, width: f64, height: f64) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window("flyout") else { return Ok(()) };
    if let Some((k, at)) = CLOSED.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == kind && at.elapsed() < Duration::from_millis(300) {
            return Ok(());
        }
    }
    let visible = w.is_visible().unwrap_or(false);
    if visible && OPEN_KIND.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(kind.as_str()) {
        return close(app);
    }
    let g = appbar::geometry();
    let s = g.scale;
    let (pw, ph) = ((width * s).round() as i32, (height * s).round() as i32);
    let margin = (12.0 * s) as i32;
    // anchor_x < 0: centred on the screen (the big calendar).
    let cx = if anchor_x < 0.0 { (g.mon_left + g.mon_right) / 2 } else { g.dock_left + (anchor_x * s) as i32 };
    let x = (cx - pw / 2).clamp(g.mon_left + margin, g.mon_right - margin - pw);
    let y = g.dock_top - (GAP as f64 * s) as i32 - ph;
    *OPEN_KIND.lock().unwrap_or_else(|e| e.into_inner()) = Some(kind.clone());
    w.emit("flyout", Open { kind, arg })?;
    // Win32 directly: Tauri's size/position calls drift on borderless glass windows.
    if let Some(h) = hwnd(&w) {
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, pw, ph, SWP_NOACTIVATE);
        }
    }
    render(&w, true);
    w.show()?;
    w.set_focus()?;
    Ok(())
}

/// Content grew or shrank: keep the bottom edge where it is.
pub fn resize(app: &AppHandle, height: f64) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window("flyout") else { return Ok(()) };
    let Some(h) = hwnd(&w) else { return Ok(()) };
    let s = appbar::geometry().scale;
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(h, &mut r).is_err() {
            return Ok(());
        }
        let ph = (height * s).round() as i32;
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), r.left, r.bottom - ph, r.right - r.left, ph, SWP_NOACTIVATE);
    }
    Ok(())
}

pub fn close(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("flyout") {
        if w.is_visible().unwrap_or(false) {
            mark_closed();
            w.hide()?;
            render(&w, false);
            let _ = w.emit("flyout-closed", ());
        }
    }
    Ok(())
}

/// Called when the panel loses focus.
pub fn mark_closed() {
    let kind = OPEN_KIND.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(k) = kind {
        *CLOSED.lock().unwrap_or_else(|e| e.into_inner()) = Some((k, Instant::now()));
    }
}

/// Shows or hides the banner window (top-right), without stealing focus.
pub fn toast(app: &AppHandle, show: bool, height: Option<f64>) -> tauri::Result<()> {
    let Some(w) = app.get_webview_window("toast") else { return Ok(()) };
    if !show {
        // Shown with Win32, so hidden with Win32 (Tauri doesn't know it's visible).
        if let Some(h) = hwnd(&w) {
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
        return Ok(());
    }
    let g = appbar::geometry();
    let s = g.scale;
    let (pw, ph) = ((TOAST_W as f64 * s) as i32, (height.unwrap_or(TOAST_H as f64) * s) as i32);
    let m = (14.0 * s) as i32;
    if let Some(h) = hwnd(&w) {
        unsafe {
            let x = match crate::config::get().banner_position.as_str() {
                "left" => g.mon_left + m,
                "center" => (g.mon_left + g.mon_right - pw) / 2,
                _ => g.mon_right - m - pw,
            };
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, g.mon_top + m, pw, ph, SWP_NOACTIVATE);
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
    }
    Ok(())
}

/// True while a panel is showing (the auto-hiding dock stays up meanwhile).
pub fn is_open() -> bool {
    OPEN_KIND.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}
