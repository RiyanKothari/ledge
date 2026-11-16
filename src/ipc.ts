import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

// ---------- types mirrored from src-tauri ----------

export type WinInfo = {
  hwnd: number;
  title: string;
  exe: string;
  aumid: string;
  key: string;
  minimized: boolean;
  active: boolean;
  seq: number;
};

export type Pin = { name: string; path: string; aumid?: string | null; target?: string | null };

export type Widgets = {
  ai: boolean;
  system: boolean;
  media: boolean;
  calendar: boolean;
  weather: boolean;
  whatsapp: boolean;
  portfolio: boolean;
  banners: boolean;
  phone: boolean;
  battery: boolean;
};

export type Config = {
  pins: Pin[] | null;
  city: string;
  latitude: number | null;
  longitude: number | null;
  placeLabel: string;
  fahrenheit: boolean;
  clock24: boolean;
  icsUrls: string[];
  widgets: Widgets;
  hideNativeTaskbar: boolean;
  startWithWindows: boolean;
  claudeLiveLimits: boolean;
  portfolioPath: string;
  dockAutohide: boolean;
  dockOrder: string[];
  configVersion: number;
  dnd: boolean;
  bannerSeconds: number;
  claudeWeb: boolean;
  appearance: Appearance;
  trayOrder: string[];
  wakeWord: boolean;
  onboarded: boolean;
  bannerPosition: "left" | "center" | "right";
};

export type DockStyle = "black" | "white" | "liquid" | "ios" | "midnight" | "custom";
export type Appearance = {
  style: DockStyle;
  opacity: number;
  tint: string;
  blur: number;
  accent: string;
  iconSize: "s" | "m" | "l";
  hoverZoom: boolean;
  clockStyle: "flip" | "simple";
  accentFromMusic: boolean;
  tiles: "glass" | "flat";
  motion: "full" | "reduced";
};

/** The frost's tint for Windows (0xAABBGGRR): the style's colour, light enough to stay glass. */
export function frostColor(a: Appearance | undefined): number {
  const m = /^#?([0-9a-f]{6})$/i.exec(a?.tint ?? "");
  const n = m ? parseInt(m[1], 16) : 0x18181c;
  const alpha = Math.round(8 + Math.min(1, Math.max(0, a?.opacity ?? 0.4)) * 96);
  const r = n >> 16;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return ((alpha << 24) | (b << 16) | (g << 8) | r) >>> 0;
}

export type TrayIcon = { id: string; tip: string; icon: string; exe: string; closable: boolean };

export type DeviceBattery = { name: string; level: number };
export type PhoneState = { batteries: DeviceBattery[]; wallpaper: string | null };

/** Every dock item, in the default left-to-right order. */
export const DOCK_ITEMS = ["claude", "apps", "media", "event", "whatsapp", "phone", "portfolio", "system", "battery", "status", "clock"] as const;
export type DockItem = (typeof DOCK_ITEMS)[number];

export const DOCK_LABELS: Record<DockItem, string> = {
  claude: "Claude usage",
  apps: "Apps",
  media: "Now playing",
  event: "Next event",
  whatsapp: "WhatsApp",
  phone: "Phone",
  portfolio: "RennSquare",
  system: "CPU & memory",
  battery: "Battery",
  status: "Wi-Fi, Bluetooth, volume",
  clock: "Clock",
};

/** The saved order, with unknown ids dropped and new items slotted in at their default spot. */
export function dockOrder(saved: string[] | undefined): DockItem[] {
  const known = (saved ?? []).filter((x): x is DockItem => (DOCK_ITEMS as readonly string[]).includes(x));
  const out = [...new Set(known)];
  DOCK_ITEMS.forEach((id, i) => {
    if (out.includes(id)) return;
    // after the item that precedes it by default, else at the start
    const prev = DOCK_ITEMS.slice(0, i).reverse().find((p) => out.includes(p));
    out.splice(prev ? out.indexOf(prev) + 1 : 0, 0, id);
  });
  return out;
}

export type Volume = { level: number; muted: boolean };
export type ProcMem = { name: string; exe: string; mb: number };

export type SystemStats = {
  cpu: number;
  ram: number;
  ramUsedGb: number;
  ramTotalGb: number;
  battery: { percent: number; charging: boolean } | null;
  netDown: number;
  netUp: number;
  netKind: "wifi" | "ethernet" | "none";
  volume: Volume | null;
  topRam: ProcMem[];
};

export type NowPlaying = {
  title: string;
  artist: string;
  sourceApp: string;
  playing: boolean;
  art: string | null;
  positionMs: number;
  durationMs: number;
  updatedAt: number;
  canSeek: boolean;
  shuffle: boolean | null;
  repeat: "none" | "track" | "list" | null;
};

export type LimitWindow = { usedPercent: number; windowMinutes: number; resetsAt: number | null };

export type ClaudeUsage = {
  available: boolean;
  sessionTokens: number;
  sessionStartedAt: number | null;
  sessionResetsAt: number | null;
  peakSessionTokens: number;
  weekTokens: number;
  peakWeekTokens: number;
  burnPerMin: number;
  lastActivity: number | null;
  topModel: string;
  topModelTokens: number;
  daily: number[];
  fiveHourLimit: number | null;
  weekLimit: number | null;
  calibratedAt: number | null;
  limitedUntil: number | null;
  limitedKind: string | null;
};

export type AiUsage = {
  claude: ClaudeUsage;
  codex: {
    available: boolean;
    primary: LimitWindow | null;
    secondary: LimitWindow | null;
    planType: string | null;
    asOf: number;
  };
};

export type PlanWindow = { usedPercent: number; resetsAt: number | null };
export type PlanLimits = {
  enabled: boolean;
  fiveHour: PlanWindow | null;
  sevenDay: PlanWindow | null;
  fetchedAt: number | null;
  error: string | null;
  source: string;
};

export type Weather = {
  configured: boolean;
  place: string;
  temp: number | null;
  high: number | null;
  low: number | null;
  code: number | null;
  isDay: boolean;
  unit: string;
  error: string | null;
};

export type CalKind = "event" | "allday" | "renewal" | "payout" | "due" | "limit";
export type CalEvent = { title: string; start: number; end: number; allDay: boolean; kind: CalKind; localId: string | null };
export type CalendarState = { configured: boolean; events: CalEvent[]; month: CalEvent[]; error: string | null };
export type LocalEvent = { id: string; title: string; start: number; end: number; allDay: boolean; kind: string; repeat: string };

export type WifiNow = { present: boolean; connected: boolean; ssid: string; signal: number };
export type Network = { ssid: string; signal: number; secure: boolean; saved: boolean; connected: boolean; auth: string };
export type BtDevice = { name: string; connected: boolean; kind: "audio" | "phone" | "input" | "computer" | "other" };
export type Output = { id: string; name: string; default: boolean };

export type Controls = {
  wifi: WifiNow;
  wifiOn: boolean | null;
  bluetoothOn: boolean | null;
  flight: boolean;
  btDevices: BtDevice[];
  brightness: number | null;
  energySaver: boolean;
  onBattery: boolean;
  nightLight: boolean | null;
  outputs: Output[];
};

export type Notif = {
  id: number;
  order: number;
  aumid: string;
  app: string;
  title: string;
  body: string;
  at: number;
  image: string | null;
  whatsapp: boolean;
  /** Ledge's own banners: "charging". */
  kind?: string;
  level?: number;
};

export type NotifCenter = {
  available: boolean;
  items: Notif[];
  whatsapp: { count: number; sender: string; text: string; at: number; image: string | null; open: string };
};

export type Fund = { id: string; name: string; value: number; dayPnl: number; dayReturn: number; positions: number };
export type Portfolio = {
  configured: boolean;
  error: string | null;
  pricesAsOf: string;
  checkedAt: string;
  niftyDay: number | null;
  funds: Fund[];
  totalValue: number;
  totalDayPnl: number;
  totalDayReturn: number;
  badAlerts: number;
  warnAlerts: number;
  alerts: { level: string; what: string; detail: string }[];
};

export type Geometry = {
  x: number;
  y: number;
  width: number;
  height: number;
  monitorWidth: number;
  monitorHeight: number;
  scale: number;
};

export type AppEntry = { name: string; id: string };

export type FlyoutKind = "control" | "center" | "media" | "claude" | "portfolio" | "whatsapp" | "system" | "launcher" | "phone" | "calendar";

// ---------- hooks ----------

/** Latest value of a live channel pushed from Rust, seeded with the cached value. */
export function useChannel<T>(name: string): T | null {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    let alive = true;
    let pushed = false;
    const off = listen<T>(name, (e) => {
      pushed = true;
      if (alive) setValue(e.payload);
    });
    invoke<T | null>("channel", { name }).then((v) => {
      if (alive && !pushed && v != null) setValue(v);
    });
    return () => {
      alive = false;
      off.then((f) => f());
    };
  }, [name]);
  return value;
}

export function useConfig(): [Config | null, (c: Config) => Promise<void>] {
  const [cfg, setCfg] = useState<Config | null>(null);
  useEffect(() => {
    let alive = true;
    invoke<Config>("get_config").then((c) => alive && setCfg(c));
    const off = listen<Config>("config", (e) => alive && setCfg(e.payload));
    const offPreview = listen<Config>("config-preview", (e) => alive && setCfg(e.payload));
    return () => {
      alive = false;
      off.then((f) => f());
      offPreview.then((f) => f());
    };
  }, []);
  const save = async (c: Config) => {
    setCfg(await invoke<Config>("save_config", { cfg: c }));
  };
  return [cfg, save];
}

/** Applies the dock look (Settings → Appearance) as CSS variables. */
export function useAppearance(a: Appearance | undefined, tone = false, musicAccent?: string | null) {
  useEffect(() => {
    const d = document.documentElement.dataset;
    d.icons = a?.iconSize ?? "m";
    d.zoom = a?.hoverZoom === false ? "off" : "on";
    d.tiles = a?.tiles ?? "glass";
    d.motion = a?.motion ?? "full";
    d.style = a?.style ?? "black";
  }, [a?.iconSize, a?.hoverZoom, a?.tiles, a?.motion, a?.style]);
  useEffect(() => {
    const r = document.documentElement.style;
    const m = /^#?([0-9a-f]{6})$/i.exec(a?.tint ?? "");
    const n = m ? parseInt(m[1], 16) : 0x16161c;
    if (tone) {
      // A light tint that covers enough of the glass needs dark text.
      const lum = (0.2126 * (n >> 16) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255;
      const light = a?.style === "white" || (a?.style !== "liquid" && lum > 0.6 && (a?.opacity ?? 0) > 0.35);
      document.documentElement.dataset.tone = light ? "light" : "dark";
    }
    r.setProperty("--tint-rgb", `${n >> 16}, ${(n >> 8) & 255}, ${n & 255}`);
    r.setProperty("--tint-a", String(Math.min(1, Math.max(0, a?.opacity ?? 0.55))));
    const accent = a?.accentFromMusic && musicAccent ? musicAccent : a?.accent;
    if (accent) r.setProperty("--accent", accent);
    else r.removeProperty("--accent");
  }, [a?.tint, a?.opacity, a?.accent, a?.style, a?.accentFromMusic, musicAccent, tone]);
}

/** Re-renders every `ms`, aligned to the wall clock. */
export function useNow(ms = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    let t: number;
    const tick = () => {
      setNow(Date.now());
      t = window.setTimeout(tick, ms - (Date.now() % ms) + 5);
    };
    t = window.setTimeout(tick, ms - (Date.now() % ms) + 5);
    return () => clearTimeout(t);
  }, [ms]);
  return now;
}

/** Re-renders every animation frame while `on` (for smooth progress bars). */
export function useFrame(on: boolean): number {
  const [t, setT] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    let raf = 0;
    let last = 0;
    const loop = (ts: number) => {
      if (ts - last > 250) {
        last = ts;
        setT(Date.now());
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [on]);
  return t;
}

const icons = new Map<string, Promise<string | null>>();

export function useIcon(primary: string | undefined | null, fallback?: string | null): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (!primary) return;
    const key = `${primary}|${fallback ?? ""}`;
    let p = icons.get(key);
    if (!p) {
      p = invoke<string | null>("icon", { primary, fallback: fallback ?? null, size: 64 }).catch(() => null);
      icons.set(key, p);
    }
    let alive = true;
    p.then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [primary, fallback]);
  return src;
}

/** Average colour of an image (album art) for accent tinting. */
export function useAccent(src: string | null | undefined, fallback = "#60a5fa"): string {
  const [c, setC] = useState(fallback);
  useEffect(() => {
    if (!src) return setC(fallback);
    const img = new Image();
    img.onload = () => {
      const cv = document.createElement("canvas");
      cv.width = cv.height = 16;
      const g = cv.getContext("2d");
      if (!g) return;
      g.drawImage(img, 0, 0, 16, 16);
      const d = g.getImageData(0, 0, 16, 16).data;
      let r = 0, gg = 0, b = 0, n = 0;
      for (let i = 0; i < d.length; i += 4) {
        const mx = Math.max(d[i], d[i + 1], d[i + 2]);
        const mn = Math.min(d[i], d[i + 1], d[i + 2]);
        const w = 1 + (mx - mn) / 32; // favour saturated pixels
        r += d[i] * w; gg += d[i + 1] * w; b += d[i + 2] * w; n += w;
      }
      r /= n; gg /= n; b /= n;
      const sat = (Math.max(r, gg, b) - Math.min(r, gg, b)) / Math.max(r, gg, b, 1);
      if (sat < 0.22) return setC(fallback); // grey covers: keep a lively accent
      // Lift dark covers so the accent reads on glass.
      const lum = 0.2126 * r + 0.7152 * gg + 0.0722 * b;
      const k = lum < 110 ? 110 / Math.max(lum, 1) : 1;
      const cl = (x: number) => Math.round(Math.min(255, x * k));
      setC(`rgb(${cl(r)}, ${cl(gg)}, ${cl(b)})`);
    };
    img.src = src;
  }, [src, fallback]);
  return c;
}

/** Opens a glass panel above the element that was clicked. */
export function openFlyout(kind: FlyoutKind, el: Element | null, width: number, height: number, arg?: string) {
  const r = el?.getBoundingClientRect();
  const anchorX = r ? r.left + r.width / 2 : 0;
  return invoke("flyout_open", { kind, arg: arg ?? null, anchorX, width, height });
}

/** Opens a panel centred on the screen (anchorX < 0). */
export function openCentered(kind: FlyoutKind, width: number, height: number, arg?: string) {
  return invoke("flyout_open", { kind, arg: arg ?? null, anchorX: -1, width, height });
}

export const call = {
  trayClick: (id: string, button: "left" | "right" | "double") => invoke<boolean>("tray_click", { id, button }),
  trayQuit: (id: string) => invoke("tray_quit", { id }),
  claudeConnect: () => invoke("claude_connect"),
  claudeRefresh: () => invoke("claude_refresh"),
  eventAdd: (event: LocalEvent) => invoke("event_add", { event }),
  eventRemove: (id: string) => invoke("event_remove", { id }),
  dockPeek: () => invoke("dock_peek"),
  listen: () => invoke("voice_listen"),
  shell: (action: "start" | "search" | "quickSettings" | "notifications" | "desktop" | "taskManager") =>
    invoke("shell_action", { action }),
  openSettings: () => invoke("open_settings"),
  quit: () => invoke("quit"),
  peek: (on: boolean) => invoke<boolean>("taskbar_peek", { on }),
  media: (action: "toggle" | "next" | "previous" | "seek" | "shuffle" | "repeat", positionMs?: number) =>
    invoke("media_control", { action, positionMs: positionMs ?? null }),
  volume: (level: number | null, toggleMute = false) => invoke<Volume | null>("set_volume", { level, toggleMute }),
  launch: (path: string) => invoke<boolean>("launch", { path }),
  click: (hwnds: number[]) => invoke("window_click", { hwnds }),
  focus: (hwnd: number) => invoke("window_focus", { hwnd }),
  close: (hwnd: number) => invoke("window_close", { hwnd }),
  radio: (kind: "wifi" | "bluetooth", on: boolean) => invoke("set_radio", { kind, on }),
  flight: (on: boolean) => invoke("set_flight", { on }),
  wifiScan: () => invoke<Network[]>("wifi_scan"),
  wifiConnect: (ssid: string, password?: string) => invoke("wifi_connect", { ssid, password: password ?? null }),
  wifiDisconnect: () => invoke("wifi_disconnect"),
  wifiForget: (ssid: string) => invoke("wifi_forget", { ssid }),
  btAudio: (name: string, connect: boolean) => invoke("bt_audio", { name, connect }),
  brightness: (pct: number) => invoke("set_brightness", { pct: Math.round(pct) }),
  output: (id: string) => invoke("set_output", { id }),
  energySaver: (on: boolean) => invoke("set_energy_saver", { on }),
  nightLight: (on: boolean) => invoke("set_night_light", { on }),
  flyoutClose: () => invoke("flyout_close"),
  flyoutResize: (height: number) => invoke("flyout_resize", { height }),
  toast: (show: boolean, height?: number) => invoke("toast_show", { show, height: height ?? null }),
  dismiss: (id: number | null) => invoke("notif_dismiss", { id }),
  openNotif: (aumid: string, whatsapp = false) => invoke("notif_open", { aumid, whatsapp }),
  apps: () => invoke<AppEntry[]>("apps_list"),
  hold: (on: boolean) => invoke("dock_hold", { on }),
};

/** Coalesces rapid slider input into one in-flight call at a time. */
export function throttled<T>(fn: (v: T) => Promise<unknown>) {
  let busy = false;
  let next: { v: T } | null = null;
  const run = async (v: T) => {
    busy = true;
    try {
      await fn(v);
    } catch {
      /* the next state push shows the truth */
    }
    busy = false;
    if (next) {
      const n = next.v;
      next = null;
      run(n);
    }
  };
  return (v: T) => {
    if (busy) next = { v };
    else run(v);
  };
}

// ---------- formatting ----------

/** "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" -> "Spotify"; "chrome.exe" -> "Chrome". */
export function appLabel(id: string): string {
  const known: Record<string, string> = { spotifymusic: "Spotify", msedge: "Edge", chrome: "Chrome", firefox: "Firefox", brave: "Brave", vlc: "VLC", zunemusic: "Media Player" };
  let k = id.split("!")[0].replace(/\.exe$/i, "");
  k = k.split(/[\\.]/).pop() ?? k;
  k = k.replace(/_[a-z0-9]{10,}$/i, "");
  return known[k.toLowerCase()] ?? (k.charAt(0).toUpperCase() + k.slice(1));
}

export function fmtTokens(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 1 : 2)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e4 ? 0 : 1)}K`;
  return `${n}`;
}

export function fmtRate(bytesPerSec: number): string {
  if (bytesPerSec >= 1e6) return `${(bytesPerSec / 1e6).toFixed(1)} MB/s`;
  if (bytesPerSec >= 1e3) return `${Math.round(bytesPerSec / 1e3)} KB/s`;
  return `${bytesPerSec} B/s`;
}

export function fmtSpan(ms: number, seconds = false): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(s / 60);
  if (m < 60) return seconds ? `${m}m ${String(s % 60).padStart(2, "0")}s` : `${Math.max(m, s > 0 ? 1 : 0)}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

export function fmtTime(t: number, h24: boolean): string {
  return new Date(t).toLocaleTimeString([], { hour: h24 ? "2-digit" : "numeric", minute: "2-digit", hour12: !h24 });
}

export function fmtAgo(t: number, now: number): string {
  const s = Math.round((now - t) / 1000);
  if (s < 45) return "now";
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  return new Date(t).toLocaleDateString([], { day: "numeric", month: "short" });
}

export function fmtClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Indian-style money: ₹5.18L, ₹1.2Cr, ₹48,210. */
export function fmtINR(v: number, signed = false): string {
  const sign = v < 0 ? "−" : signed ? "+" : "";
  const a = Math.abs(v);
  let body: string;
  if (a >= 1e7) body = `${(a / 1e7).toFixed(2)}Cr`;
  else if (a >= 1e5) body = `${(a / 1e5).toFixed(2)}L`;
  else body = Math.round(a).toLocaleString("en-IN");
  return `${sign}₹${body}`;
}

export function fmtPct(r: number, digits = 2): string {
  return `${r >= 0 ? "+" : "−"}${Math.abs(r * 100).toFixed(digits)}%`;
}
