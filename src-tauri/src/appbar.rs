//! Registers the dock as a real Windows AppBar on the primary monitor's bottom
//! edge, so maximized windows stop above it exactly like the native taskbar.
//! The window is subclassed to follow shell notifications: other appbars moving,
//! full-screen apps (the dock steps aside), display/DPI changes and Explorer restarts.

use crate::taskbar;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowRect, IsHungAppWindow, IsWindowVisible, IsZoomed, SPI_SETWORKAREA,
    SW_MAXIMIZE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetForegroundWindow, WM_SETTINGCHANGE,
};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABM_ACTIVATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABM_WINDOWPOSCHANGED,
    ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA, DefSubclassProc, SHAppBarMessage, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, RegisterWindowMessageW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_ACTIVATE, WM_APP,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_WINDOWPOSCHANGED, WS_EX_TOOLWINDOW,
};
use windows::core::w;

/// The floating dock: a pill PILL_DIP tall, GAP_BOTTOM above the screen
/// edge. The reserved strip also leaves GAP_TOP above the pill, so maximized
/// windows never touch it.
pub const PILL_DIP: i32 = 54;
pub const GAP_BOTTOM: i32 = 8;
pub const GAP_TOP: i32 = 6;
pub const SIDE_MARGIN: i32 = 12;
const CALLBACK: u32 = WM_APP + 0x51;

static BAR: AtomicIsize = AtomicIsize::new(0);
/// A plain window under the dock that only blurs what's behind it ("frost").
/// It's a hair smaller than the pill, so its (unavoidably square-ish) region
/// edge hides under the dock's smooth, anti-aliased rim.
static FROST: AtomicIsize = AtomicIsize::new(0);
static FROST_SIZE: std::sync::Mutex<(i32, i32)> = std::sync::Mutex::new((0, 0));
/// The frost's tint, 0xAABBGGRR — set by the dock page from the chosen style.
static FROST_COLOR: AtomicU32 = AtomicU32::new(0x26FFFFFF);
/// 4 = acrylic frost; 3 = the light, clear blur Liquid uses.
static FROST_STATE: AtomicU32 = AtomicU32::new(4);

/// Retints the frost behind the dock (live while you try styles in Settings).
pub fn set_frost_color(color: u32, state: u32) {
    let state = if state == 3 { 3 } else { 4 };
    let same = FROST_COLOR.swap(color, Ordering::SeqCst) == color && FROST_STATE.swap(state, Ordering::SeqCst) == state;
    if same {
        return;
    }
    if let Some(f) = frost() {
        crate::flyout::blur_behind(f, state, color);
    }
}

fn frost() -> Option<HWND> {
    let raw = FROST.load(Ordering::SeqCst);
    (raw != 0).then(|| HWND(raw as *mut _))
}

unsafe extern "system" fn frost_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(h, msg, wp, lp) }
}

/// Creates the frost window and makes the dock its owned window, so the dock
/// always stays directly above it.
fn create_frost(dock: HWND) {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GWLP_HWNDPARENT, RegisterClassExW, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let class = w!("LedgeDockFrost");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(frost_proc),
            hInstance: inst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(h) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOPMOST,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(inst.into()),
            None,
        ) else {
            return;
        };
        crate::flyout::blur_behind(h, FROST_STATE.load(Ordering::SeqCst), FROST_COLOR.load(Ordering::SeqCst));
        // Windows' own rounded corners (the blur ignores window regions). No border
        // line: that thin grey outline is what showed past the pill's ends. Its soft
        // shadow stays, as the dock's drop shadow.
        {
            use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const _, size_of_val(&pref) as u32);
            let none = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &none as *const _ as *const _, size_of_val(&none) as u32);
        }
        FROST.store(h.0 as isize, Ordering::SeqCst);
        SetWindowLongPtrW(dock, GWLP_HWNDPARENT, h.0 as isize);
    }
}

/// The dock is a full pill (end radius = half its height). Windows can only round
/// this blur's corners a little (8 DIPs), so it's inset 6 DIPs all round, where it
/// nests inside the circular ends; CSS draws an even glass rim over that band.

fn place_frost(left: i32, top: i32, w: i32, h: i32, scale: f64) {
    let Some(f) = frost() else { return };
    // An 8 DIP rounded rectangle inset 6 DIPs all round nests inside the pill's round ends.
    let (iy, ix) = ((6.0 * scale).round() as i32, (6.0 * scale).round() as i32);
    let (fw, fh) = ((w - 2 * ix).max(1), (h - 2 * iy).max(1));
    *FROST_SIZE.lock().unwrap_or_else(|e| e.into_inner()) = (fw, fh);
    unsafe {
        let _ = SetWindowPos(f, Some(HWND_TOPMOST), left + ix, top + iy, fw, fh, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

fn hide_frost() {
    if let Some(f) = frost() {
        unsafe {
            let _ = ShowWindow(f, SW_HIDE);
        }
    }
}
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static LAST_RECT: std::sync::Mutex<Option<(i32, i32, i32, i32)>> = std::sync::Mutex::new(None);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Width the UI asked for, in DIPs (0 = full width).
static WANT_W: AtomicI32 = AtomicI32::new(0);
/// Where the dock window actually is, physical px: (left, top, right, bottom).
static DOCK: std::sync::Mutex<(i32, i32, i32, i32)> = std::sync::Mutex::new((0, 0, 0, 0));

/// A real full-screen app (a game, a video, a slideshow) is in front.
/// Windows' own "full-screen app" notice also fires for plain maximized windows
/// once auto-hide frees the whole screen, so the foreground window is checked.
pub fn is_fullscreen_app() -> bool {
    is_foreground_fullscreen(monitor().0)
}

// ---------- auto-hide ----------
//
// Auto-hide reserves no screen space. The dock waits below the screen edge;
// touching the bottom edge slides it up, and it slides away again shortly
// after the pointer leaves — unless a panel is open or the UI asked to hold
// it (dragging, menus).

static AUTOHIDE: AtomicBool = AtomicBool::new(false);
/// 0.0 = fully hidden, 1.0 = fully shown (drives the slide).
static SHOWN: std::sync::Mutex<f64> = std::sync::Mutex::new(1.0);
static HOLD: AtomicBool = AtomicBool::new(false);
/// Set by the hotkey: show the dock now, wherever the pointer is.
static PEEK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
static FORCE_HIDE: AtomicBool = AtomicBool::new(false);
/// Whether the dock's web view is painting. Off while parked off-screen, so its
/// animations (live bars, the music equalizer) cost nothing.
static RENDERING: AtomicBool = AtomicBool::new(true);

fn set_rendering(on: bool) {
    if RENDERING.swap(on, Ordering::SeqCst) == on {
        return;
    }
    if let Some(app) = crate::APP.get() {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("bar") {
            crate::flyout::render(&w, on);
        }
    }
}

/// The shortcut that starts a voice command ("" when none was free).
pub static VOICE_KEY: std::sync::Mutex<&'static str> = std::sync::Mutex::new("");

/// Alt+` shows the dock over anything (and hides it again).
pub fn toggle_peek() {
    if shown() > 0.5 {
        *PEEK.lock().unwrap_or_else(|e| e.into_inner()) = None;
        FORCE_HIDE.store(true, Ordering::SeqCst);
    } else {
        *PEEK.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    }
}

pub fn spawn_hotkey() {
    std::thread::Builder::new()
        .name("ledge-hotkey".into())
        .spawn(|| unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, RegisterHotKey};
            use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
            // VK_OEM_3 is the ` key on US/UK/Indian layouts.
            use windows::Win32::UI::Input::KeyboardAndMouse::MOD_CONTROL;
            let peek = RegisterHotKey(None, 1, MOD_ALT | MOD_NOREPEAT, 0xC0).is_ok();
            use windows::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT;
            // Talk to Ledge. Ctrl+Alt+Space is taken by the Claude app, so take
            // the first free one of these and remember which.
            let mut voice = false;
            for (mods, vk, label) in [
                (MOD_CONTROL | MOD_ALT, 0x4C_u32, "Ctrl + Alt + L"),
                (MOD_CONTROL | MOD_SHIFT, 0x20, "Ctrl + Shift + Space"),
                (MOD_CONTROL | MOD_ALT, 0x20, "Ctrl + Alt + Space"),
            ] {
                if RegisterHotKey(None, 2, mods | MOD_NOREPEAT, vk).is_ok() {
                    *VOICE_KEY.lock().unwrap_or_else(|e| e.into_inner()) = label;
                    voice = true;
                    break;
                }
            }
            if let Some(app) = crate::APP.get() {
                crate::hub::publish(app, "hotkeys", &serde_json::json!({ "peek": peek, "voice": voice }));
            }
            if !peek && !voice {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_HOTKEY {
                    if msg.wParam.0 == 2 {
                        if let Some(app) = crate::APP.get() {
                            crate::voice::listen(app);
                        }
                    } else {
                        toggle_peek();
                    }
                }
            }
        })
        .expect("spawn hotkey");
}

pub fn set_hold(on: bool) {
    HOLD.store(on, Ordering::SeqCst);
}

/// Sets the starting mode before the bar is installed (no repositioning).
pub fn init_autohide(on: bool) {
    AUTOHIDE.store(on, Ordering::SeqCst);
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = if on { 0.0 } else { 1.0 };
}

pub fn autohide() -> bool {
    AUTOHIDE.load(Ordering::SeqCst)
}

/// Switches auto-hide on or off: re-reserves (or frees) the strip and lets
/// maximized windows grow or shrink to match.
pub fn set_autohide(on: bool) {
    AUTOHIDE.store(on, Ordering::SeqCst);
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = if on { 0.0 } else { 1.0 };
    if let Some(h) = bar() {
        position(h, true);
    }
}

/// Something of Ledge is on screen (the dock, or a panel): pollers can run fast.
pub fn on_screen() -> bool {
    !autohide() || shown() > 0.0 || crate::flyout::is_open()
}

fn shown() -> f64 {
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_foreground_fullscreen(mon: RECT) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let cls = {
            let mut buf = [0u16; 64];
            let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(fg, &mut buf);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        };
        if matches!(cls.as_str(), "Progman" | "WorkerW") {
            return false; // the desktop itself
        }
        // Maximized windows (Chrome, Explorer...) cover the screen when nothing is
        // reserved, but they still have a title bar: they're not full screen.
        if IsZoomed(fg).as_bool() {
            return false;
        }
        let style = GetWindowLongPtrW(fg, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) as u32;
        let caption = windows::Win32::UI::WindowsAndMessaging::WS_CAPTION.0;
        if style & caption == caption {
            return false;
        }
        r.left <= mon.left && r.top <= mon.top && r.right >= mon.right && r.bottom >= mon.bottom
    }
}

/// Watches the pointer and slides the dock in and out. Cheap: one
/// GetCursorPos every 30 ms, window moves only while animating.
pub fn spawn_autohide() {
    std::thread::Builder::new()
        .name("ledge-autohide".into())
        .spawn(|| {
            let mut edge_since: Option<std::time::Instant> = None;
            let mut away_since: Option<std::time::Instant> = None;
            let mut tick = 0u32;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(30));
                tick = tick.wrapping_add(1);
                if tick % 20 == 0 && on_screen() {
                    update_covered();
                }
                if !autohide() {
                    set_rendering(true);
                    continue;
                }
                let Some(h) = bar() else { continue };
                let (mon, scale) = monitor();
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                }
                let on_monitor = pt.x >= mon.left && pt.x < mon.right;
                let at_edge = on_monitor && pt.y >= mon.bottom - 2;
                let (l, _, r, _) = *DOCK.lock().unwrap_or_else(|e| e.into_inner());
                let pill_top = mon.bottom - px(GAP_BOTTOM + PILL_DIP, scale);
                let over_dock = pt.x >= l - px(16, scale) && pt.x <= r + px(16, scale) && pt.y >= pill_top - px(18, scale);
                let peeking = PEEK
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(3000));
                let held = HOLD.load(Ordering::SeqCst) || crate::flyout::is_open() || crate::preview::is_open();
                let keep = held || peeking;
                let cur = shown();
                let force_hide = FORCE_HIDE.swap(false, Ordering::SeqCst) && !held;
                let target = if force_hide {
                    0.0
                } else if peeking && cur == 0.0 {
                    1.0
                } else if cur > 0.0 {
                    // Visible: stay while the pointer is on it or something holds it.
                    if over_dock || keep {
                        away_since = None;
                        1.0
                    } else {
                        let since = *away_since.get_or_insert_with(std::time::Instant::now);
                        if since.elapsed() > std::time::Duration::from_millis(650) { 0.0 } else { 1.0 }
                    }
                } else if at_edge && !is_foreground_fullscreen(mon) {
                    // Hidden: a short dwell on the edge avoids flicker when flinging the mouse.
                    let since = *edge_since.get_or_insert_with(std::time::Instant::now);
                    if since.elapsed() > std::time::Duration::from_millis(70) { 1.0 } else { 0.0 }
                } else {
                    edge_since = None;
                    0.0
                };
                if target != cur {
                    edge_since = None;
                    away_since = None;
                    if target > 0.0 {
                        update_covered();
                        set_rendering(true);
                    }
                    slide(h, cur, target);
                    if target == 0.0 {
                        set_rendering(false);
                    }
                } else if cur == 0.0 {
                    set_rendering(false);
                }
            }
        })
        .expect("spawn autohide");
}

/// Eased slide between hidden (0) and shown (1), ~180 ms.
fn slide(hwnd: HWND, from: f64, to: f64) {
    const FRAMES: u32 = 12;
    for i in 1..=FRAMES {
        let t = i as f64 / FRAMES as f64;
        let e = if to > from { 1.0 - (1.0 - t).powi(3) } else { t * t * t };
        *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = from + (to - from) * e;
        place(hwnd);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = to;
    place(hwnd);
}

#[derive(serde::Serialize, Clone, Copy, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    /// Dock window rect relative to the monitor, in DIPs.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub monitor_width: f64,
    pub monitor_height: f64,
    pub scale: f64,
    /// Monitor rect, physical px (for placing flyouts and banners).
    pub mon_left: i32,
    pub mon_top: i32,
    pub mon_right: i32,
    pub mon_bottom: i32,
    pub dock_left: i32,
    pub dock_top: i32,
    pub dock_right: i32,
    pub dock_bottom: i32,
}

pub fn geometry() -> Geometry {
    let (mon, scale) = monitor();
    let (l, t, r, b) = *DOCK.lock().unwrap_or_else(|e| e.into_inner());
    Geometry {
        x: (l - mon.left) as f64 / scale,
        y: (t - mon.top) as f64 / scale,
        width: (r - l) as f64 / scale,
        height: (b - t) as f64 / scale,
        monitor_width: (mon.right - mon.left) as f64 / scale,
        monitor_height: (mon.bottom - mon.top) as f64 / scale,
        scale,
        mon_left: mon.left,
        mon_top: mon.top,
        mon_right: mon.right,
        mon_bottom: mon.bottom,
        dock_left: l,
        dock_top: t,
        dock_right: r,
        dock_bottom: b,
    }
}

/// Primary monitor rect (physical) and its scale factor.
pub fn monitor() -> (RECT, f64) {
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        (mi.rcMonitor, x as f64 / 96.0)
    }
}

fn px(dip: i32, scale: f64) -> i32 {
    (dip as f64 * scale).round() as i32
}

/// The UI measured its content: resize the pill (keeps it centred).
pub fn set_width(dip: i32) {
    WANT_W.store(dip.max(0), Ordering::SeqCst);
    if let Some(h) = bar() {
        place(h);
    }
}

/// Something (an app window) sits behind the dock.
static COVERED: AtomicBool = AtomicBool::new(false);

/// Is any app window under the dock's rect? (The liquid style refracts the
/// wallpaper only when it's actually what's behind the glass.)
fn covered_now() -> bool {
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId, IsIconic};
    struct Ctx {
        rect: RECT,
        me: u32,
        hit: bool,
    }
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let c = &mut *(lp.0 as *mut Ctx);
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                return BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == c.me {
                return BOOL(1);
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
            if cloaked != 0 {
                return BOOL(1);
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(h, &mut buf);
            let cls = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                return BOOL(1);
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, size_of::<RECT>() as u32).is_err() {
                return BOOL(1);
            }
            let d = c.rect;
            if r.left < d.right && r.right > d.left && r.top < d.bottom && r.bottom > d.top {
                c.hit = true;
                return BOOL(0);
            }
            BOOL(1)
        }
    }
    let (l, t, r, b) = *DOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ctx = Ctx {
        rect: RECT { left: l, top: t, right: r, bottom: b },
        me: unsafe { windows::Win32::System::Threading::GetCurrentProcessId() },
        hit: false,
    };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.hit
}

/// Re-checks what's behind the dock and tells the UI when it changes.
pub fn update_covered() {
    let now = covered_now();
    if COVERED.swap(now, Ordering::SeqCst) != now {
        if let Some(app) = crate::APP.get() {
            use tauri::Emitter;
            let _ = app.emit("dock-covered", now);
        }
    }
}

pub fn is_covered() -> bool {
    COVERED.load(Ordering::SeqCst)
}

/// Moves the dock window to its pill rect inside the reserved strip.
fn place(hwnd: HWND) {
    let (mon, scale) = monitor();
    let full = mon.right - mon.left - 2 * px(SIDE_MARGIN, scale);
    let want = WANT_W.load(Ordering::SeqCst);
    let w = if want <= 0 { full } else { px(want, scale).min(full) };
    let h = px(PILL_DIP, scale);
    let left = mon.left + (mon.right - mon.left - w) / 2;
    let top_shown = mon.bottom - px(GAP_BOTTOM, scale) - h;
    // Hidden = parked just below the screen edge.
    let hidden_top = mon.bottom + 2;
    let s = shown();
    let top = hidden_top + ((top_shown - hidden_top) as f64 * s).round() as i32;
    // Flyouts anchor to where the dock is when fully shown.
    let rect = (left, top_shown, left + w, top_shown + h);
    let changed = {
        let mut d = DOCK.lock().unwrap_or_else(|e| e.into_inner());
        let c = *d != rect;
        *d = rect;
        c
    };
    place_frost(left, top, w, h, scale);
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), left, top, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    if !changed {
        return;
    }
    if let Some(app) = crate::APP.get() {
        use tauri::Emitter;
        let _ = app.emit("dock-geometry", geometry());
    }
}


fn bar() -> Option<HWND> {
    let raw = BAR.load(Ordering::SeqCst);
    (raw != 0).then(|| HWND(raw as *mut _))
}

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: CALLBACK,
        uEdge: ABE_BOTTOM,
        ..Default::default()
    }
}

/// Must run on the window's own (main) thread: subclassing is per-thread.
pub fn install(hwnd: HWND) {
    BAR.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        // Tool window: never in Alt+Tab, never in its own taskbar list.
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
        let _ = SetWindowSubclass(hwnd, Some(subclass), 1, 0);
    }
    create_frost(hwnd);
    register(hwnd);
    position(hwnd, true);
}

fn register(hwnd: HWND) {
    let mut abd = data(hwnd);
    unsafe { SHAppBarMessage(ABM_NEW, &mut abd) };
}

pub fn unregister() {
    if let Some(hwnd) = bar() {
        let mut abd = data(hwnd);
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut abd) };
    }
}


/// Reserve the strip with the shell, then move the window into it.
/// `force` re-applies even when nothing changed (first run, Explorer restart).
fn position(hwnd: HWND, force: bool) {
    let (mon, scale) = monitor();
    // Auto-hide: register the bar but reserve nothing, so windows get the whole screen.
    let h = if autohide() { 0 } else { px(GAP_TOP + PILL_DIP + GAP_BOTTOM, scale) };
    let mut abd = data(hwnd);
    abd.rc = RECT { left: mon.left, right: mon.right, top: mon.bottom - h, bottom: mon.bottom };
    unsafe {
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        abd.rc.top = abd.rc.bottom - h;
        let key = (abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
        {
            let mut last = LAST_RECT.lock().unwrap_or_else(|e| e.into_inner());
            if !force && *last == Some(key) {
                return; // breaks the POSCHANGED -> SETPOS -> POSCHANGED loop
            }
            *last = Some(key);
        }
        SHAppBarMessage(ABM_SETPOS, &mut abd);
    }
    place(hwnd);
    // Backup for the WM_SETTINGCHANGE path below, in case that broadcast is missed.
    fit_soon(1200);
}

/// The shell doesn't resize windows that were already maximized; do it
/// once the new work area has settled.
fn fit_soon(ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        fit_maximized_windows();
    });
}

/// Makes every maximized window on the primary monitor fill exactly the
/// current work area (keeping its maximized state and frame overhang).
pub fn fit_maximized_windows() {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let (mon, work) = unsafe { *(lp.0 as *const (HMONITOR, RECT)) };
        unsafe {
            if !IsWindowVisible(h).as_bool()
                || !IsZoomed(h).as_bool()
                || IsHungAppWindow(h).as_bool()
                || MonitorFromWindow(h, MONITOR_DEFAULTTONULL) != mon
            {
                return BOOL(1);
            }
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_err() {
                return BOOL(1);
            }
            // Maximized windows hang their resize border off-screen by a few pixels.
            let over = (work.left - r.left).clamp(0, 32);
            let want = RECT {
                left: work.left - over,
                top: work.top - over,
                right: work.right + over,
                bottom: work.bottom + over,
            };
            if r == want {
                return BOOL(1);
            }
            if r.bottom < want.bottom || r.top > want.top || r.left > want.left || r.right < want.right {
                // Growing: Chrome and others clamp a maximized window to the
                // area they cached, so re-maximize to make them recompute.
                let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
                let _ = ShowWindow(h, SW_MAXIMIZE);
            } else {
                // Shrinking: a plain resize keeps the window maximized, no flicker.
                let _ = SetWindowPos(
                    h,
                    None,
                    want.left,
                    want.top,
                    want.right - want.left,
                    want.bottom - want.top,
                    // Not SWP_ASYNCWINDOWPOS: Chrome ignores async moves of a
                    // maximized window. Hung windows are skipped above and this
                    // runs off the UI thread, so the dock can't block.
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
        BOOL(1)
    }
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return;
        }
        let ctx = (mon, mi.rcWork);
        let fg = GetForegroundWindow();
        let _ = EnumWindows(Some(each), LPARAM(&ctx as *const (HMONITOR, RECT) as isize));
        // Re-maximizing activates windows; hand focus back to what had it.
        if !fg.is_invalid() && GetForegroundWindow() != fg {
            let _ = SetForegroundWindow(fg);
        }
    }
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    unsafe {
        match msg {
            CALLBACK => {
                match wp.0 as u32 {
                    ABN_POSCHANGED => position(hwnd, false),
                    ABN_FULLSCREENAPP => {
                        let fs = lp.0 != 0;
                        FULLSCREEN.store(fs, Ordering::SeqCst);
                        if autohide() {
                            // Parked off-screen already; the reveal checks the real
                            // foreground window (maximized Chrome triggers this too).
                        } else if fs {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            hide_frost();
                        } else {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            position(hwnd, true);
                        }
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_ACTIVATE => {
                let mut abd = data(hwnd);
                SHAppBarMessage(ABM_ACTIVATE, &mut abd);
            }
            WM_WINDOWPOSCHANGED => {
                let mut abd = data(hwnd);
                SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut abd);
            }
            // The work area actually changed (our strip, or the taskbar
            // coming back): refit maximized windows. Fitting doesn't touch
            // the work area, so this can't loop.
            WM_SETTINGCHANGE if wp.0 as u32 == SPI_SETWORKAREA.0 => fit_soon(100),
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                position(hwnd, true);
                return r;
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
                // Explorer restarted: the new taskbar is visible and our
                // appbar registration is gone. Re-apply both.
                if crate::config::get().hide_native_taskbar {
                    taskbar::hide();
                }
                register(hwnd);
                position(hwnd, true);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
