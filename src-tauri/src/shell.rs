//! Shell work that needs a single-threaded COM apartment: app icons, reading
//! .lnk shortcuts, importing the Windows taskbar pins, launching apps, and
//! sending the Start / Quick Settings / Notification keys.

use crate::config::Pin;
use base64::Engine;
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{LazyLock, Mutex, OnceLock};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW, HBITMAP,
    HGDIOBJ, ReleaseDC,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_A,
    VK_CONTROL, VK_D, VK_ESCAPE, VK_LWIN, VK_N, VK_S, VK_SHIFT,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, IShellLinkW, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
    ShellExecuteW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, Interface, PCWSTR, w};

type Job = Box<dyn FnOnce() + Send>;
static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

/// Starts the STA worker thread. Idempotent.
pub fn start() {
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("ledge-shell".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
                for job in rx {
                    job();
                }
            })
            .expect("spawn shell worker");
        Mutex::new(tx)
    });
}

/// Runs `f` on the STA worker and waits for its result.
pub fn run<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    start();
    let (tx, rx) = mpsc::channel();
    let job: Job = Box::new(move || {
        let _ = tx.send(f());
    });
    WORKER.get()?.lock().ok()?.send(job).ok()?;
    rx.recv().ok()
}

// ---------- icons ----------

static ICONS: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// PNG data URL for a file, shortcut or `shell:AppsFolder\<AUMID>` item.
pub fn icon(parse_name: &str, size: i32) -> Option<String> {
    let key = format!("{size}|{}", parse_name.to_lowercase());
    if let Some(hit) = ICONS.lock().ok()?.get(&key) {
        return hit.clone();
    }
    let out = unsafe { icon_uncached(parse_name, size) };
    if let Ok(mut m) = ICONS.lock() {
        m.insert(key, out.clone());
    }
    out
}

unsafe fn icon_uncached(parse_name: &str, size: i32) -> Option<String> {
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(parse_name), None).ok()?;
        let hbm = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let png = bitmap_to_png(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        png.map(|bytes| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }
}

/// A bitmap's pixels as 32-bit BGRA, top-down.
unsafe fn bitmap_bgra(hbm: HBITMAP) -> Option<(i32, i32, Vec<u8>)> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbm.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let lines = GetDIBits(hdc, hbm, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);
        (lines != 0).then_some((w, h, px))
    }
}

fn rgba_png(w: i32, h: i32, px: &[u8]) -> Option<String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(px).ok()?;
    }
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

/// A bitmap (e.g. a window snapshot) as an opaque PNG data URL.
pub unsafe fn bitmap_data_url(hbm: HBITMAP) -> Option<String> {
    unsafe {
        let (w, h, mut px) = bitmap_bgra(hbm)?;
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        rgba_png(w, h, &px)
    }
}

/// An icon handle (e.g. a tray icon) as a PNG data URL. Icons carry straight
/// alpha; old ones have none and use their mask for transparency instead.
pub unsafe fn hicon_data_url(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(hicon, &mut ii).ok()?;
        let color = if ii.hbmColor.is_invalid() { None } else { bitmap_bgra(ii.hbmColor) };
        let mask = if ii.hbmMask.is_invalid() { None } else { bitmap_bgra(ii.hbmMask) };
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
        let (w, h, mut px) = color?;
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            p.swap(0, 2);
            if !has_alpha {
                // Mask: white = transparent.
                let clear = mask.as_ref().filter(|m| m.0 == w && m.1 >= h).is_some_and(|m| m.2[i * 4] > 127);
                p[3] = if clear { 0 } else { 255 };
            }
        }
        rgba_png(w, h, &px)
    }
}

unsafe fn bitmap_to_png(hbm: HBITMAP) -> Option<Vec<u8>> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbm.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
            return None;
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let lines = GetDIBits(hdc, hbm, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);
        if lines == 0 {
            return None;
        }
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2); // BGRA -> RGBA
            if !has_alpha {
                p[3] = 255;
            } else if p[3] > 0 && p[3] < 255 {
                // Shell bitmaps are premultiplied; PNG wants straight alpha.
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().ok()?;
            writer.write_image_data(&px).ok()?;
        }
        Some(out)
    }
}

// ---------- shortcuts and pins ----------

/// (target exe, AUMID) of a .lnk file.
pub fn read_lnk(path: &Path) -> (Option<String>, Option<String>) {
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else {
            return (None, None);
        };
        let Ok(file) = link.cast::<IPersistFile>() else { return (None, None) };
        if file.Load(&HSTRING::from(path.as_os_str()), STGM_READ).is_err() {
            return (None, None);
        }
        let mut buf = [0u16; 1024];
        let target = link
            .GetPath(&mut buf, std::ptr::null_mut(), 0)
            .ok()
            .map(|_| String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]))
            .filter(|s| !s.is_empty());
        let aumid = link
            .cast::<IPropertyStore>()
            .ok()
            .and_then(|store| store.GetValue(&PKEY_AppUserModel_ID).ok())
            .map(|v| v.to_string())
            .filter(|s| !s.is_empty());
        (target, aumid)
    }
}

/// The user's current Windows taskbar pins, oldest first (≈ pin order).
pub fn import_taskbar_pins() -> Vec<Pin> {
    let Some(appdata) = dirs::config_dir() else { return Vec::new() };
    let dir = appdata.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
                .map(|p| (p.metadata().and_then(|m| m.created()).unwrap_or(std::time::UNIX_EPOCH), p))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut pins: Vec<Pin> = files
        .into_iter()
        .map(|(_, p)| {
            let (target, aumid) = read_lnk(&p);
            Pin {
                name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p.to_string_lossy().into_owned(),
                aumid,
                target,
            }
        })
        .collect();
    if pins.is_empty() {
        let explorer = r"C:\Windows\explorer.exe".to_string();
        pins.push(Pin { name: "File Explorer".into(), path: explorer.clone(), aumid: None, target: Some(explorer) });
    }
    pins
}

// ---------- launching ----------

pub fn launch(path: &str) -> bool {
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

fn launch_with_args(file: &str, args: &str) -> bool {
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(file), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Folders where Windows and browsers keep app shortcuts (Desktop may be in OneDrive).
fn shortcut_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = dirs::desktop_dir() {
        v.push(d);
    }
    if let Ok(p) = std::env::var("PUBLIC") {
        v.push(Path::new(&p).join("Desktop"));
    }
    if let Some(d) = dirs::data_dir() {
        v.push(d.join("Microsoft").join("Windows").join("Start Menu").join("Programs"));
        v.push(d.join("Microsoft").join("Internet Explorer").join("Quick Launch").join("User Pinned").join("TaskBar"));
    }
    if let Ok(p) = std::env::var("ProgramData") {
        v.push(Path::new(&p).join("Microsoft").join("Windows").join("Start Menu").join("Programs"));
    }
    v
}

/// A shortcut (.lnk) whose app ID is `aumid` — e.g. a Chrome profile's desktop shortcut.
/// Scanning takes about a second, so answers are remembered.
pub fn shortcut_for(aumid: &str) -> Option<std::path::PathBuf> {
    static FOUND: LazyLock<Mutex<HashMap<String, Option<std::path::PathBuf>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    let key = aumid.to_ascii_lowercase();
    if let Some(hit) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if hit.as_ref().is_none_or(|p| p.exists()) {
            return hit.clone();
        }
    }
    let hit = shortcut_scan(aumid);
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).insert(key, hit.clone());
    hit
}

fn shortcut_scan(aumid: &str) -> Option<std::path::PathBuf> {
    fn scan(dir: &Path, depth: u32, aumid: &str) -> Option<std::path::PathBuf> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth > 0 {
                    if let Some(hit) = scan(&p, depth - 1, aumid) {
                        return Some(hit);
                    }
                }
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                if read_lnk(&p).1.is_some_and(|a| a.eq_ignore_ascii_case(aumid)) {
                    return Some(p);
                }
            }
        }
        None
    }
    shortcut_dirs().iter().find_map(|d| scan(d, 2, aumid))
}

/// "Profile19" -> "Profile 19" (browsers drop the space in the app ID).
pub fn spaced_profile(s: &str) -> String {
    match s.find(|c: char| c.is_ascii_digit()) {
        Some(i) if i > 0 && !s[..i].ends_with(' ') => format!("{} {}", &s[..i], &s[i..]),
        _ => s.to_string(),
    }
}

/// (browser exe, profile folder) for a per-profile browser app ID such as
/// "Chrome.UserData.Profile19", when that profile exists on this PC.
pub fn browser_profile(aumid: &str) -> Option<(&'static str, String)> {
    let parts: Vec<&str> = aumid.splitn(3, '.').collect();
    if parts.len() != 3 || !parts[1].eq_ignore_ascii_case("UserData") {
        return None;
    }
    let local = dirs::data_local_dir()?;
    let (exe, data) = match parts[0].to_ascii_lowercase().as_str() {
        "chrome" => ("chrome.exe", local.join("Google").join("Chrome").join("User Data")),
        "msedge" => ("msedge.exe", local.join("Microsoft").join("Edge").join("User Data")),
        "brave" => ("brave.exe", local.join("BraveSoftware").join("Brave-Browser").join("User Data")),
        _ => return None,
    };
    [parts[2].to_string(), spaced_profile(parts[2])].into_iter().find(|d| data.join(d).is_dir()).map(|d| (exe, d))
}

/// Opens an app by ID. Chrome, Edge and Brave give each profile its own ID
/// ("Chrome.UserData.Profile19"): it groups that profile's windows but isn't a
/// Start-menu entry, so `shell:AppsFolder\…` can't open it. For those, use the
/// profile's own shortcut, or start the browser on that profile.
pub fn launch_app_id(aumid: &str) -> bool {
    if launch(&format!("shell:AppsFolder\\{aumid}")) {
        return true;
    }
    if let Some(lnk) = shortcut_for(aumid) {
        return launch(&lnk.to_string_lossy());
    }
    match browser_profile(aumid) {
        Some((exe, dir)) => launch_with_args(exe, &format!("--profile-directory=\"{dir}\"")),
        None => false,
    }
}

// ---------- shell keys ----------

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn chord(keys: &[VIRTUAL_KEY]) {
    let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, false)).collect();
    inputs.extend(keys.iter().rev().map(|&k| key(k, true)));
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

pub fn shell_action(action: &str) -> bool {
    match action {
        "start" => chord(&[VK_LWIN]),
        "search" => chord(&[VK_LWIN, VK_S]),
        "quickSettings" => chord(&[VK_LWIN, VK_A]),
        "notifications" => chord(&[VK_LWIN, VK_N]),
        "desktop" => chord(&[VK_LWIN, VK_D]),
        "taskManager" => chord(&[VK_CONTROL, VK_SHIFT, VK_ESCAPE]),
        _ => return false,
    }
    true
}

// ---------- names, app list, wallpaper ----------

fn take_pwstr(p: windows::core::PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

/// Display name of a shell item (file, shortcut or `shell:AppsFolder\<AUMID>`).
pub fn display_name(parse_name: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{IShellItem, SIGDN_NORMALDISPLAY};
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(parse_name), None).ok()?;
        let name = take_pwstr(item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?);
        (!name.is_empty()).then_some(name)
    }
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// Launch with `shell:AppsFolder\<id>`.
    pub id: String,
}

/// Every app in the Start menu's "All apps" list (desktop and Store apps).
pub fn apps() -> Vec<AppEntry> {
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem,
        SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut batch = [None];
            let mut n = 0u32;
            if items.Next(&mut batch, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(take_pwstr).unwrap_or_default();
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).map(take_pwstr).unwrap_or_default();
            let lower = name.to_lowercase();
            if name.is_empty() || id.is_empty() || lower.starts_with("uninstall") || lower.contains("readme") {
                continue;
            }
            out.push(AppEntry { name, id });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name);
    out
}

static APPS: Mutex<Option<(std::time::Instant, Vec<AppEntry>)>> = Mutex::new(None);

/// The app list, built on its own STA thread (it takes seconds) and cached
/// for ten minutes, so the launcher opens instantly and icons aren't queued behind it.
pub fn apps_cached() -> Vec<AppEntry> {
    if let Some((at, list)) = APPS.lock().ok().and_then(|a| a.clone()) {
        if at.elapsed() < std::time::Duration::from_secs(600) {
            return list;
        }
    }
    let list = std::thread::spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        apps()
    })
    .join()
    .unwrap_or_default();
    if let Ok(mut a) = APPS.lock() {
        *a = Some((std::time::Instant::now(), list.clone()));
    }
    list
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    pub data_url: Option<String>,
    /// Windows' WallpaperStyle: "10" fill, "6" fit, "2" stretch, "0" centre/tile, "22" span.
    pub style: String,
    pub key: String,
}

static WALL: LazyLock<Mutex<Option<Wallpaper>>> = LazyLock::new(|| Mutex::new(None));

fn wallpaper_path() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
    let mut buf = [0u16; 1024];
    unsafe {
        SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr() as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).ok()?;
    }
    let p = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]);
    let p = std::path::PathBuf::from(p);
    if p.is_file() {
        return Some(p);
    }
    // Spotlight / slideshow: Windows keeps the current image here.
    let t = dirs::config_dir()?.join(r"Microsoft\Windows\Themes\TranscodedWallpaper");
    t.is_file().then_some(t)
}

fn wallpaper_style() -> String {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let mut buf = [0u16; 16];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), w!("WallpaperStyle"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut size)).is_ok() {
            return String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]);
        }
    }
    "10".into()
}

/// The current desktop wallpaper as a data URL — the dock blurs it to look
/// like frosted glass over the desktop (the strip under the dock is always
/// desktop, because it's reserved).
pub fn wallpaper() -> Wallpaper {
    let path = wallpaper_path();
    let mtime = path.as_ref().and_then(|p| p.metadata().ok()).and_then(|m| m.modified().ok());
    let key = format!("{:?}|{:?}|{}", path, mtime, wallpaper_style());
    if let Some(w) = WALL.lock().ok().and_then(|w| w.clone()) {
        if w.key == key {
            return w;
        }
    }
    let data_url = path.and_then(|p| std::fs::read(p).ok()).filter(|b| b.len() < 40_000_000).map(|b| {
        let mime = if b.starts_with(&[0x89, b'P']) {
            "image/png"
        } else if b.starts_with(b"RIFF") {
            "image/webp"
        } else if b.starts_with(b"BM") {
            "image/bmp"
        } else {
            "image/jpeg"
        };
        format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
    });
    let w = Wallpaper { data_url, style: wallpaper_style(), key };
    if let Ok(mut c) = WALL.lock() {
        *c = Some(w.clone());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this PC's shortcuts and Chrome profiles"]
    fn live_profile_resolution() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        for id in ["Chrome.UserData.Profile19", "Chrome.UserData.Profile16", "Chrome.UserData.Profile13"] {
            println!("{id}: shortcut={:?} profile={:?}", shortcut_for(id), browser_profile(id));
        }
    }

    #[test]
    fn spaces_browser_profiles() {
        assert_eq!(spaced_profile("Profile19"), "Profile 19");
        assert_eq!(spaced_profile("Default"), "Default");
        assert_eq!(spaced_profile("Profile 3"), "Profile 3");
        assert!(browser_profile("Notepad").is_none());
        assert!(browser_profile("Chrome.UserData.NoSuchProfile123").is_none());
    }
}
