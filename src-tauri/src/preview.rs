//! Window previews above the dock: hovering an app shows a picture of each of
//! its windows so you can pick one (click), or close it (×, middle-click).
//!
//! Pictures are taken with PrintWindow(PW_RENDERFULLCONTENT), which works for
//! GPU-drawn apps like Chrome, scaled down and refreshed about once a second
//! while the preview is open. (DWM thumbnails would be live, but they are drawn
//! under the web view that renders the cards, so they never showed.)

use crate::appbar;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, HALFTONE, HGDIOBJ, ReleaseDC, SRCCOPY,
    SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HWND_TOPMOST, IsIconic, IsWindow, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow,
};

/// Card layout in DIPs.
const SHOT_H: f64 = 132.0;
const TITLE_H: f64 = 30.0;
const PAD: f64 = 8.0;
const GAP: f64 = 8.0;
const EDGE: f64 = 10.0;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub hwnd: i64,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub minimized: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Shot {
    hwnd: i64,
    img: String,
}

static OPEN: AtomicBool = AtomicBool::new(false);
static HOVER: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);

pub fn is_open() -> bool {
    OPEN.load(Ordering::SeqCst)
}

fn h(raw: i64) -> HWND {
    HWND(raw as isize as *mut _)
}

/// The visible frame (without the invisible resize borders) and the full window rect.
fn frames(w: HWND) -> Option<(RECT, RECT)> {
    unsafe {
        let mut full = RECT::default();
        GetWindowRect(w, &mut full).ok()?;
        let mut vis = RECT::default();
        if DwmGetWindowAttribute(w, DWMWA_EXTENDED_FRAME_BOUNDS, &mut vis as *mut _ as *mut _, size_of::<RECT>() as u32).is_err() {
            vis = full;
        }
        (vis.right > vis.left && vis.bottom > vis.top).then_some((vis, full))
    }
}

/// A scaled-down picture of the window as a PNG data URL.
fn snapshot(w: HWND, max_w: i32, max_h: i32) -> Option<String> {
    unsafe {
        if IsIconic(w).as_bool() {
            return None;
        }
        let (vis, full) = frames(w)?;
        let (fw, fh) = (full.right - full.left, full.bottom - full.top);
        let (vw, vh) = (vis.right - vis.left, vis.bottom - vis.top);
        if fw <= 0 || fh <= 0 || fw > 8000 || fh > 8000 {
            return None;
        }
        let scale = (max_w as f64 / vw as f64).min(max_h as f64 / vh as f64).min(1.0);
        let (tw, th) = (((vw as f64 * scale).round() as i32).max(1), ((vh as f64 * scale).round() as i32).max(1));
        let screen = GetDC(None);
        let big_dc = CreateCompatibleDC(Some(screen));
        let big = CreateCompatibleBitmap(screen, fw, fh);
        let old_big = SelectObject(big_dc, HGDIOBJ(big.0));
        let ok = PrintWindow(w, big_dc, PRINT_WINDOW_FLAGS(2)).as_bool(); // PW_RENDERFULLCONTENT
        let small_dc = CreateCompatibleDC(Some(screen));
        let small = CreateCompatibleBitmap(screen, tw, th);
        let old_small = SelectObject(small_dc, HGDIOBJ(small.0));
        if ok {
            SetStretchBltMode(small_dc, HALFTONE);
            let _ = SetBrushOrgEx(small_dc, 0, 0, None);
            let _ = StretchBlt(small_dc, 0, 0, tw, th, Some(big_dc), vis.left - full.left, vis.top - full.top, vw, vh, SRCCOPY);
        }
        SelectObject(big_dc, old_big);
        SelectObject(small_dc, old_small);
        let _ = DeleteObject(HGDIOBJ(big.0));
        let _ = DeleteDC(big_dc);
        let _ = DeleteDC(small_dc);
        ReleaseDC(None, screen);
        let url = if ok { crate::shell::bitmap_data_url(small) } else { None };
        let _ = DeleteObject(HGDIOBJ(small.0));
        url
    }
}

fn win(app: &AppHandle) -> Option<(tauri::WebviewWindow, HWND)> {
    let w = app.get_webview_window("preview")?;
    let hw = HWND(w.hwnd().ok()?.0);
    Some((w, hw))
}

/// Shows cards for `windows` (hwnd, title) centred over `anchor_x` (DIPs from the dock's left edge).
pub fn show(app: &AppHandle, windows: Vec<(i64, String)>, anchor_x: f64, icon: Option<String>) {
    let Some((w, dest)) = win(app) else { return };
    let seq = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let g = appbar::geometry();
    let s = g.scale;
    let max_cards = (((g.monitor_width - 2.0 * EDGE) / (170.0 + GAP)).floor() as usize).clamp(1, 8);
    let mut cards = Vec::new();
    let mut x = EDGE;
    for (raw, title) in windows.into_iter().take(max_cards) {
        if unsafe { !IsWindow(Some(h(raw))).as_bool() } {
            continue;
        }
        let minimized = unsafe { IsIconic(h(raw)).as_bool() };
        let aspect = frames(h(raw))
            .filter(|_| !minimized)
            .map(|(v, _)| (v.right - v.left) as f64 / (v.bottom - v.top) as f64)
            .unwrap_or(16.0 / 10.0);
        let sw = (SHOT_H * aspect).clamp(150.0, 250.0);
        let cw = sw + 2.0 * PAD;
        cards.push(Card { hwnd: raw, title, x, y: EDGE, w: cw, h: TITLE_H + SHOT_H + PAD, minimized });
        x += cw + GAP;
    }
    if cards.is_empty() {
        hide(app);
        return;
    }
    let width = x - GAP + EDGE;
    let height = 2.0 * EDGE + TITLE_H + SHOT_H + PAD;
    let (pw, ph) = ((width * s).round() as i32, (height * s).round() as i32);
    let margin = (12.0 * s) as i32;
    let cx = g.dock_left + (anchor_x * s) as i32;
    let px = (cx - pw / 2).clamp(g.mon_left + margin, g.mon_right - margin - pw);
    let py = g.dock_top - (10.0 * s) as i32 - ph;
    OPEN.store(true, Ordering::SeqCst);
    unsafe {
        let _ = SetWindowPos(dest, Some(HWND_TOPMOST), px, py, pw, ph, SWP_NOACTIVATE);
        let _ = ShowWindow(dest, SW_SHOWNOACTIVATE);
    }
    let _ = w.emit("preview", serde_json::json!({ "cards": cards, "icon": icon }));

    // Pictures, off the UI thread, refreshed while this preview stays open.
    let app = app.clone();
    let list: Vec<i64> = cards.iter().map(|c| c.hwnd).collect();
    let (max_w, max_h) = ((250.0 * s) as i32, (SHOT_H * s) as i32);
    std::thread::spawn(move || {
        let mut first = true;
        while OPEN.load(Ordering::SeqCst) && GEN.load(Ordering::SeqCst) == seq {
            for &raw in &list {
                if GEN.load(Ordering::SeqCst) != seq {
                    return;
                }
                if let Some(img) = snapshot(h(raw), max_w, max_h) {
                    let _ = app.emit_to("preview", "preview-shot", Shot { hwnd: raw, img });
                }
            }
            std::thread::sleep(Duration::from_millis(if first { 400 } else { 1200 }));
            first = false;
        }
    });
}

pub fn hide(app: &AppHandle) {
    GEN.fetch_add(1, Ordering::SeqCst);
    if !OPEN.swap(false, Ordering::SeqCst) {
        return;
    }
    HOVER.store(false, Ordering::SeqCst);
    if let Some((w, hw)) = win(app) {
        unsafe {
            let _ = ShowWindow(hw, SW_HIDE);
        }
        let _ = w.emit("preview-hidden", ());
    }
}

/// The pointer left the app button or the preview: hide shortly, unless it
/// moves onto the preview (or another app) in the meantime.
pub fn hide_soon(app: &AppHandle) {
    let seq = GEN.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        if GEN.load(Ordering::SeqCst) == seq && !HOVER.load(Ordering::SeqCst) {
            hide(&app);
        }
    });
}

pub fn set_hover(on: bool) {
    HOVER.store(on, Ordering::SeqCst);
}
