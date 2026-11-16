# Ledge

A floating glass dock that replaces the Windows 11 taskbar — and every Windows flyout it used to open. Your apps, live
widgets (music with lyrics, calendar, battery, notifications, weather, Claude usage…), Ledge's own Control Center,
window previews, a calendar, and voice commands ("Hey Ledge, open WhatsApp").

## Install it (2 minutes)

**You need:** Windows 11, 64-bit.

1. **Download** `Ledge_0.8.1_x64-setup.exe` from the [latest release](https://github.com/RiyanKothari/ledge/releases/latest)
   (under *Assets*).
2. **Run it.** The installer isn't code-signed yet, so Windows may show *"Windows protected your PC"* — click
   **More info → Run anyway**. It installs just for you (no admin needed) in a few seconds.
3. **Ledge opens a short welcome tour.** It walks you through:
   - **Your taskbar** — Ledge hides the Windows taskbar and becomes yours. Quitting Ledge always brings the old one back.
   - **Make it yours** — pick a dock style (Black, White, Liquid, iOS, Midnight) and a colour.
   - **Voice** — try a command, and choose whether Ledge listens for "Hey Ledge" (off by default; it keeps the mic on).
   - **Connect things** (optional) — your city for weather, a calendar, Claude usage.
4. That's it. **Touch the bottom edge of the screen** and the dock slides up.

### Getting around

| Do this | And you get |
| --- | --- |
| Touch the bottom edge, or press **Alt + `** | The dock |
| Click the **clock** | Notifications, calendar (⤢ for the full month), apps running in the background |
| Click **Wi-Fi / volume** | Control Center: Wi-Fi, Bluetooth, brightness, sound, Do not disturb |
| Hover an open app | Previews of its windows — click one to switch |
| Drag apps or widgets | Reorder them. **Right-click a widget** to hide or move it |
| Press **Ctrl + Alt + L** or the 🎤, or say **"Hey Ledge"** | Voice: "open Chrome", "pause", "next song", "volume 40", "show my calendar" |
| Right-click the dock | Settings, Task Manager, and **Quit** (restores the Windows taskbar) |

Settings (right-click the dock → Ledge settings) can show the welcome tour again.

### Uninstall, or get the Windows taskbar back

- **Quit:** right-click the dock → *Quit Ledge* — the Windows taskbar comes straight back.
- **Uninstall:** Windows Settings → Apps → Installed apps → Ledge → Uninstall. The uninstaller restores the taskbar and
  stops Ledge starting at sign-in.
- **If the taskbar is ever missing:** open Task Manager (Ctrl + Shift + Esc) → find *Windows Explorer* → **Restart**. Or
  run `%LOCALAPPDATA%\Ledge\ledge.exe --restore-taskbar`.

### Found a bug?

Open an issue on this repo (what you did, what happened, a screenshot if you can) or tell Riyan.

## What's new in 0.8

- A **welcome tour** on first launch that sets everything up, and a friendlier installer that cleanly upgrades and
  uninstalls (it always puts the Windows taskbar back).
- "Hey Ledge" is off until you turn it on; SongSeam shortcuts only show when SongSeam is installed.

## What's new in 0.7

- **Voice**: Ctrl + Alt + L, the dock's mic, or "Hey Ledge" — recognised by Windows on your PC, nothing sent anywhere.
- A frosted dock (text behind it no longer shows through), glass panels, a glowing listening orb.

## What's new in 0.6

- **Natural glass**: a tinted pane with a sheen and a lit rim, smooth round corners; widgets, the Wi-Fi cluster and the
  clock are glass tiles too.
- **SongSeam in the music player**: time-synced lyrics (from LRCLIB, like SongSeam; only title, artist and length are
  sent) and one-tap Glow / Mood DJ / lyrics overlay, which open SongSeam.
- **Previews show the last picture of minimized windows** (Ledge keeps a small picture of the window in front).
- **Window previews fixed**: hover an open app to see a picture of each of its windows (refreshed while you look).
- **Music panel**: blurred artwork, wave seek bar, shuffle and repeat, output devices behind the cast button.
- Glassier Control Center; sliders and bars follow your accent colour; running apps show a dot instead of a bar; new
  Start logo.
- Fixed: Wi-Fi and Bluetooth lists were squashed by Settings styles; banners could open blank.

## What's new in 0.5

- **Dock styles**: Black, White, Liquid, iOS and Midnight (Settings → Appearance), plus any accent colour. **Liquid** is
  real refraction: the wallpaper is bent through the glass toward the rim, with a faint colour fringe — not a blur.
- **Window previews**: hover an open app on the dock to see live thumbnails of its windows; click one to switch,
  middle-click or × to close it.
- **Right-click any widget** to hide it or move it; hidden widgets come back from the dock's right-click menu.
- **Banners**: choose how long they stay (1–15 s); clicking one brings the app forward (WhatsApp from your phone opens
  WhatsApp Desktop).
- **Quick toggles** redesigned as pills and rounds in your accent colour; a new Start mark; a new Settings window.
- Lighter when idle: Control Center and system readings slow down while the dock is tucked away.

## What's new in 0.4

- **Claude numbers that match Claude.** Connect claude.ai once (Claude panel or Settings → Claude): you sign in inside a
  Ledge window, and Ledge reads the same 5-hour and weekly usage that claude.ai → Settings → Usage shows. Without it the
  bars are labelled as estimates (Claude Code's logs can't see chats in the Claude app).
- **The dock comes up over Chrome and other maximized apps.** Touch the bottom edge, or press **Alt + `** anywhere.
- **Do not disturb**, in the clock panel and Settings. With it off, banners slide away after 4 seconds.
- **Background apps** (the old tray "notch"): their icons live in the clock panel. Click, double-click or right-click them
  exactly as in the Windows tray.
- **Full calendar**: the expand button on the clock panel's calendar opens a month view with categories (renewals, payouts,
  events, all day, due dates, Claude limit resets) and lets you add your own events, once, monthly or yearly.
- **Flip clock** on the dock, the **real WhatsApp icon**, and a **Charging** banner when you plug in.
- **Appearance**: glass opacity, tint, frost and accent colour (Settings → Appearance).
- **Lighter**: the dock stops painting while it's tucked away, and the glass is blurred once instead of every frame.

## What's new in 0.3

- **Auto-hide.** The dock slides away and frees the whole screen; touch the bottom edge and it slides back. It stays up
  while a panel is open or you're dragging. Turn it off in Settings → Dock.
- **Rearrange everything.** Drag any widget, or any pinned app, along the dock. Settings → Dock has the same order with
  up/down buttons.
- **Phone widget.** Your phone's battery (as it reports over Bluetooth), its wallpaper from Phone Link, and a red badge
  for new phone notifications. The panel also shows the battery of other connected Bluetooth devices.
- **Claude limits on the dock.** Both the 5-hour and weekly bars, with reset times. Live plan limits are on by default.
- **The Windows taskbar stays hidden** on every monitor, and is put back whenever Ledge quits.

## What's in 0.2

**The dock** floats above a reserved strip (a real Windows AppBar, so maximized windows stop above it). Its frosted
glass is your actual wallpaper, blurred, so corners stay round and smooth. It sizes itself to its content and drops
optional widgets one at a time when the screen is too narrow.

| Widget | Source |
| --- | --- |
| Claude session + weekly bars (live, shimmering while you work) | Claude Code's logs; optionally Anthropic's plan limits |
| Apps (pins imported from Windows, running dots, window cycling) | Windows |
| Now playing — art, accent colour, progress, controls | Windows media sessions (Spotify, browsers…) |
| Next event | Secret iCal links (Google, Outlook, iCloud) |
| WhatsApp — unread count and last sender | WhatsApp Desktop or WhatsApp mirrored by Phone Link |
| RennSquare — total value, day change, per-fund bars | `RennSquare Investments\data\dashboard\live.json` (read-only) |
| Phone — battery, wallpaper, unread badge | Bluetooth hands-free battery; Phone Link's wallpaper copy |
| CPU / RAM, battery ring | Win32 |
| Status (Wi-Fi strength, Bluetooth, volume) → Control Center | Ledge's own engine |
| Clock tile (time, date, temperature) → notification center + calendar | — |

**Panels** (real acrylic glass, close when you click elsewhere):

- **Control Center** — Wi-Fi (scan, connect with password, disconnect, forget), Bluetooth (paired devices, connect /
  disconnect headphones and speakers), flight mode, energy saver, night light, brightness, volume, sound output, battery,
  CPU/RAM with the apps using the most memory.
- **Notification center** — flip clock, weather, notifications with contact photos, month calendar, the day's agenda.
- **Media** — big artwork, seek bar, controls, output device.
- **Claude** — 5-hour and weekly bars with a live reset countdown, burn rate, most-used model, 7-day chart.
- **RennSquare** — all paper funds, Nifty comparison, urgent alerts.
- **WhatsApp** — unread messages; opens WhatsApp.
- **Apps** — search every installed app (Start button). Right-click Start for Windows' own menu.

**Banners** — new notifications slide in top-right as glass banners, one at a time, like macOS. Turn on Windows' Do not
disturb so its own banners don't show too; notifications still arrive and Ledge still sees them.

## Privacy

Everything is read locally. Network calls: Open-Meteo (the city you type), your iCal links; if you
connect claude.ai, a short-lived hidden page on claude.ai that asks for your usage (your claude.ai session stays in
Ledge's own web view profile; Ledge never sees your password); and — only if you turn on **Live Claude plan limits** —
`api.anthropic.com` with the sign-in Claude Code already saved. Ledge never stores, refreshes or sends that sign-in
anywhere else.

Tray icons: Ledge keeps a hidden window that receives apps' tray icons and passes every message on to Explorer
unchanged, so Explorer's own tray stays complete.

## Run it

```bash
npm install
npm run desktop
```

`npm run desktop:build` makes `src-tauri/target/release/bundle/nsis/Ledge_<version>_x64-setup.exe`. Installed builds start
at sign-in (Settings → Startup), which is what makes Ledge your taskbar. `npm run verify` type-checks, runs the Rust
tests and builds the UI. Hardware checks that only read (or write back the same brightness):
`cd src-tauri && cargo test --lib live_ -- --ignored --nocapture`.

## If the Windows taskbar ever goes missing

Ledge saves the taskbar's state before hiding it and restores it on quit, on a panic, and on the next launch after a
crash. Otherwise run `ledge --restore-taskbar`, or restart Windows Explorer from Task Manager.

## Known limits

- Primary monitor only (the dock and its panels show on your main display).
- Wi-Fi network names need Settings → Privacy → Location → "Let desktop apps access your location".
- Bluetooth connect/disconnect works for audio devices; other devices show their state. Pairing new devices opens Windows Settings.
- Energy saver can only switch on while on battery (that's how Windows' setting works).
- Night light is switched through Windows' own stored setting; Ledge backs it up first to `%APPDATA%\Ledge\nightlight-backup.bin`.
- Calendar: TZID times are read in this PC's time zone; "2nd Monday"-style rules show only the first occurrence.
