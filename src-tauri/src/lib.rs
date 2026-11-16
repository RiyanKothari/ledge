//! Ledge — a smart dock that replaces the Windows 11 taskbar.

#[cfg(not(windows))]
compile_error!("Ledge replaces the Windows taskbar and only builds on Windows.");

mod ai;
mod appbar;
mod calendar;
mod claude_web;
mod config;
mod controls;
mod flyout;
mod hub;
mod media;
mod notify;
mod phone;
mod portfolio;
mod preview;
mod shell;
mod system;
mod taskbar;
mod tray;
mod voice;
mod weather;
mod wifi;
mod winwatch;

/// The app handle, for code that runs outside Tauri's callbacks (window procs).
pub(crate) static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

use config::Config;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

// ---------- commands ----------

#[tauri::command]
fn get_config() -> Config {
    config::get()
}

#[tauri::command]
fn save_config(app: AppHandle, mut cfg: Config) -> Res<Config> {
    let old = config::get();
    if old.city.trim() != cfg.city.trim() {
        // New city: geocode again on the next weather pass.
        cfg.latitude = None;
        cfg.longitude = None;
        cfg.place_label.clear();
    }
    cfg.banner_seconds = cfg.banner_seconds.clamp(1, 30);
    config::save(&cfg)?;
    if old.wake_word != cfg.wake_word {
        voice::set_wake(&app, cfg.wake_word);
    }
    if old.dock_autohide != cfg.dock_autohide {
        appbar::set_autohide(cfg.dock_autohide);
    }
    if old.start_with_windows != cfg.start_with_windows {
        apply_autostart(cfg.start_with_windows);
    }
    match (old.hide_native_taskbar, cfg.hide_native_taskbar) {
        (false, true) => taskbar::hide(),
        (true, false) => taskbar::restore(),
        _ => {}
    }
    let _ = app.emit("config", &cfg);
    Ok(cfg)
}

/// Latest value of a live channel: windows, system, media, ai, weather, calendar.
#[tauri::command]
fn channel(name: String) -> Option<Value> {
    hub::last(&name)
}

#[tauri::command]
fn window_click(hwnds: Vec<i64>) {
    winwatch::click_group(&hwnds);
}

#[tauri::command]
fn window_focus(hwnd: i64) {
    winwatch::focus(hwnd);
}

#[tauri::command]
fn window_close(hwnd: i64) {
    winwatch::close(hwnd);
}

#[tauri::command]
fn window_minimize(hwnd: i64) {
    winwatch::minimize(hwnd);
}

#[tauri::command]
async fn launch(path: String) -> Res<bool> {
    blocking(move || shell::run(move || shell::launch(&path)).unwrap_or(false)).await
}

/// App icon as a PNG data URL. Tries `primary`, then `fallback`.
#[tauri::command]
async fn icon(primary: String, fallback: Option<String>, size: Option<i32>) -> Res<Option<String>> {
    let size = size.unwrap_or(64).clamp(16, 256);
    blocking(move || {
        shell::run(move || {
            shell::icon(&primary, size).or_else(|| fallback.as_deref().and_then(|f| shell::icon(f, size)))
        })
        .flatten()
    })
    .await
}

#[tauri::command]
fn shell_action(action: String) -> bool {
    shell::shell_action(&action)
}

#[tauri::command]
async fn set_volume(level: Option<f32>, toggle_mute: Option<bool>) -> Res<Option<system::Volume>> {
    blocking(move || system::set_volume(level, toggle_mute.unwrap_or(false))).await
}

#[tauri::command]
async fn media_control(action: String, position_ms: Option<i64>) -> Res<bool> {
    blocking(move || media::control(&action, position_ms)).await
}

// ---------- control center ----------

#[tauri::command]
async fn set_radio(kind: String, on: bool) -> Res<()> {
    blocking(move || controls::set_radio(&kind, on)).await?
}

#[tauri::command]
async fn set_flight(on: bool) -> Res<()> {
    blocking(move || controls::set_flight(on)).await?
}

#[tauri::command]
async fn wifi_scan() -> Res<Vec<wifi::Network>> {
    blocking(wifi::scan).await
}

#[tauri::command]
async fn wifi_connect(ssid: String, password: Option<String>) -> Res<()> {
    blocking(move || wifi::connect(&ssid, password.as_deref().filter(|p| !p.is_empty()))).await?
}

#[tauri::command]
async fn wifi_disconnect() -> Res<bool> {
    blocking(wifi::disconnect).await
}

#[tauri::command]
async fn wifi_forget(ssid: String) -> Res<bool> {
    blocking(move || wifi::forget(&ssid)).await
}

#[tauri::command]
async fn bt_audio(name: String, connect: bool) -> Res<()> {
    blocking(move || controls::bt_audio(&name, connect)).await?
}

#[tauri::command]
async fn set_brightness(pct: u8) -> Res<()> {
    blocking(move || controls::set_brightness(pct)).await?
}

#[tauri::command]
async fn set_output(id: String) -> Res<()> {
    blocking(move || controls::set_output(&id)).await?
}

#[tauri::command]
async fn set_energy_saver(on: bool) -> Res<()> {
    blocking(move || controls::set_energy_saver(on)).await?
}

#[tauri::command]
async fn set_night_light(on: bool) -> Res<()> {
    blocking(move || controls::set_night_light(on)).await?
}

// ---------- surfaces ----------

#[tauri::command]
fn dock_resize(width: i32) {
    appbar::set_width(width);
}

/// The UI is dragging or showing a menu: keep the auto-hiding dock up.
#[tauri::command]
fn dock_hold(on: bool) {
    appbar::set_hold(on);
}

#[tauri::command]
fn dock_geometry() -> appbar::Geometry {
    appbar::geometry()
}

#[tauri::command]
async fn wallpaper() -> Res<shell::Wallpaper> {
    blocking(shell::wallpaper).await
}

#[tauri::command]
async fn apps_list() -> Res<Vec<shell::AppEntry>> {
    blocking(shell::apps_cached).await
}

#[tauri::command]
async fn flyout_open(app: AppHandle, kind: String, arg: Option<String>, anchor_x: f64, width: f64, height: f64) -> Res<()> {
    flyout::open(&app, kind, arg, anchor_x, width, height).map_err(|e| e.to_string())
}

#[tauri::command]
async fn flyout_resize(app: AppHandle, height: f64) -> Res<()> {
    flyout::resize(&app, height).map_err(|e| e.to_string())
}

#[tauri::command]
async fn flyout_close(app: AppHandle) -> Res<()> {
    flyout::close(&app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn toast_show(app: AppHandle, show: bool, height: Option<f64>) -> Res<()> {
    flyout::toast(&app, show, height).map_err(|e| e.to_string())
}

#[tauri::command]
fn notif_dismiss(id: Option<i64>) {
    notify::dismiss(id);
}

#[tauri::command]
async fn notif_open(aumid: String, whatsapp: Option<bool>) -> Res<bool> {
    // WhatsApp mirrored from the phone opens WhatsApp Desktop when it's installed.
    let aumid = if whatsapp.unwrap_or(false) && notify::whatsapp_installed() { notify::WHATSAPP_APP.to_string() } else { aumid };
    blocking(move || {
        if winwatch::focus_app(&aumid) {
            return true;
        }
        let target = notify::open_target(&aumid);
        shell::run(move || shell::launch(&target)).unwrap_or(false)
    })
    .await
}

// ---------- start with Windows ----------

fn apply_autostart(on: bool) {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
    if cfg!(debug_assertions) {
        return; // never register a dev build
    }
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, w!("Ledge"), REG_SZ.0, Some(cmd.as_ptr() as *const _), (cmd.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("Ledge"));
        }
    }
}

#[tauri::command]
fn tray_click(id: String, button: String) -> bool {
    tray::click(&id, &button)
}

#[tauri::command]
fn tray_quit(id: String) -> Res<()> {
    tray::quit(&id)
}

#[tauri::command]
async fn claude_connect(app: AppHandle) -> Res<()> {
    claude_web::open_sign_in(&app)
}

#[tauri::command]
fn claude_refresh(app: AppHandle) {
    claude_web::refresh_now(&app);
}

#[tauri::command]
fn event_add(event: calendar::LocalEvent) -> Res<()> {
    calendar::add_local(event)
}

#[tauri::command]
fn event_remove(id: String) -> Res<()> {
    calendar::remove_local(&id)
}

#[tauri::command]
fn preview_show(app: AppHandle, windows: Vec<(i64, String)>, anchor_x: f64, icon: Option<String>) {
    preview::show(&app, windows, anchor_x, icon);
}

#[tauri::command]
fn preview_hide(app: AppHandle, soon: bool) {
    if soon {
        preview::hide_soon(&app);
    } else {
        preview::hide(&app);
    }
}

#[tauri::command]
fn preview_hover(app: AppHandle, on: bool) {
    preview::set_hover(on);
    if !on {
        preview::hide_soon(&app);
    }
}

#[tauri::command]
fn dock_covered() -> bool {
    appbar::is_covered()
}

#[tauri::command]
fn voice_hotkey() -> String {
    appbar::VOICE_KEY.lock().unwrap_or_else(|e| e.into_inner()).to_string()
}

#[tauri::command]
fn voice_listen(app: AppHandle) {
    voice::listen(&app);
}

#[tauri::command]
fn dock_peek() {
    appbar::toggle_peek();
}

/// Show the real taskbar (for the system tray) or hide it again.
#[tauri::command]
fn taskbar_peek(on: bool) -> bool {
    taskbar::peek(on);
    taskbar::is_peeking()
}

// Window creation must not happen in a sync command on Windows (deadlock).
#[tauri::command]
async fn open_settings(app: AppHandle) -> Res<()> {
    open_settings_window(&app).await
}

/// The first-run welcome tour (also from Settings → General).
#[tauri::command]
async fn open_welcome(app: AppHandle) -> Res<()> {
    open_welcome_window(&app)
}

pub(crate) fn open_welcome_window(app: &AppHandle) -> Res<()> {
    if let Some(w) = app.get_webview_window("welcome") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "welcome", WebviewUrl::App("index.html".into()))
        .title("Welcome to Ledge")
        .inner_size(860.0, 620.0)
        .resizable(false)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) async fn open_settings_window(app: &AppHandle) -> Res<()> {
    let app = app.clone();
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("Ledge settings")
        .inner_size(900.0, 640.0)
        .min_inner_size(760.0, 520.0)
        .center()
        .theme(Some(tauri::Theme::Dark))
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------- startup ----------

fn already_running() -> bool {
    unsafe {
        // The handle is kept for the life of the process on purpose.
        let _ = CreateMutexW(None, true, w!("Local\\LedgeDockSingleInstance"));
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

pub fn restore_taskbar_and_exit() -> ! {
    taskbar::restore();
    taskbar::show_now();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    appbar::fit_maximized_windows();
    std::process::exit(0)
}

pub fn run() {
    if already_running() {
        return;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        taskbar::restore();
        default_hook(info);
    }));
    // After a crash, a saved state file holds the user's ORIGINAL taskbar
    // setting. If we're about to hide the taskbar again, keep that file as is:
    // restoring and re-reading would risk saving our own auto-hide as "original".
    if !config::load().hide_native_taskbar {
        taskbar::recover_after_crash();
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            channel,
            window_click,
            window_focus,
            window_close,
            window_minimize,
            launch,
            icon,
            shell_action,
            set_volume,
            media_control,
            set_radio,
            set_flight,
            wifi_scan,
            wifi_connect,
            wifi_disconnect,
            wifi_forget,
            bt_audio,
            set_brightness,
            set_output,
            set_energy_saver,
            set_night_light,
            dock_resize,
            dock_geometry,
            dock_hold,
            wallpaper,
            apps_list,
            flyout_open,
            flyout_resize,
            flyout_close,
            toast_show,
            notif_dismiss,
            notif_open,
            taskbar_peek,
            tray_click,
            tray_quit,
            claude_connect,
            claude_refresh,
            event_add,
            event_remove,
            dock_peek,
            voice_listen,
            voice_hotkey,
            open_welcome,
            dock_covered,
            preview_show,
            preview_hide,
            preview_hover,
            open_settings,
            quit,
        ])
        .setup(|app| {
            let _ = APP.set(app.handle().clone());
            shell::start();
            apply_autostart(config::get().start_with_windows);
            if config::get().pins.is_none() {
                let pins = shell::run(shell::import_taskbar_pins).unwrap_or_default();
                let _ = config::update(|c| c.pins = Some(pins));
            }
            if config::get().hide_native_taskbar {
                taskbar::hide();
            }
            let bar = app.get_webview_window("bar").ok_or("bar window missing")?;
            let raw = bar.hwnd()?.0 as isize;
            appbar::init_autohide(config::get().dock_autohide);
            appbar::install(HWND(raw as *mut _));
            appbar::spawn_autohide();
            appbar::spawn_hotkey();
            bar.show()?;
            flyout::create(app.handle())?;
            std::thread::spawn(shell::apps_cached); // warm the launcher

            let h = app.handle().clone();
            winwatch::spawn(h.clone());
            system::spawn(h.clone());
            media::spawn(h.clone());
            ai::spawn(h.clone());
            controls::spawn(h.clone());
            notify::spawn(h.clone());
            portfolio::spawn(h.clone());
            phone::spawn(h.clone());
            tray::spawn(h.clone());
            preview::spawn_watcher();
            if !config::get().onboarded {
                let _ = open_welcome_window(&h);
            }
            if config::get().wake_word {
                // After the app list is warm, so "Hey Ledge, open …" knows your apps.
                let hw = h.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                    voice::set_wake(&hw, true);
                });
            }
            tauri::async_runtime::spawn(claude_web::run(h.clone()));
            tauri::async_runtime::spawn(ai::plan_loop(h.clone()));
            tauri::async_runtime::spawn(weather::run(h.clone()));
            tauri::async_runtime::spawn(calendar::run(h));
            Ok(())
        })
        .on_window_event(|w, ev| {
            // Alt+F4 on the dock must not leave you without a taskbar.
            match (w.label(), ev) {
                ("bar" | "flyout" | "toast" | "preview" | "voice", WindowEvent::CloseRequested { api, .. }) => api.prevent_close(),
                // Panels close when you click anywhere else, like the real ones.
                ("flyout", WindowEvent::Focused(false)) => {
                    flyout::mark_closed();
                    let _ = w.hide();
                    if let Some(wv) = w.app_handle().get_webview_window("flyout") {
                        flyout::render(&wv, false);
                    }
                    let _ = w.emit("flyout-closed", ());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Ledge")
        .run(|_app, ev| {
            if let RunEvent::Exit = ev {
                appbar::unregister();
                taskbar::restore();
                // Let maximized windows grow back into the freed strip
                // (the shell needs a moment to publish the new work area).
                std::thread::sleep(std::time::Duration::from_millis(1000));
                appbar::fit_maximized_windows();
            }
        });
}
