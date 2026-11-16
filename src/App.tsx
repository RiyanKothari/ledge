import { invoke } from "@tauri-apps/api/core";
import { Menu } from "@tauri-apps/api/menu";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Apps } from "./components/Apps";
import {
  BatteryRing,
  ClaudeCard,
  ClockTile,
  MediaCard,
  NextEventCard,
  PhoneCard,
  PortfolioCard,
  StatusCluster,
  SystemCard,
  WhatsAppCard,
} from "./components/DockWidgets";
import { StartGlyph } from "./components/glyphs";
import { Sortable } from "./components/Sortable";
import {
  call,
  DOCK_LABELS,
  openFlyout,
  type Config,
  type Widgets,
  useChannel,
  useConfig,
  dockOrder,
  type AiUsage,
  type DockItem,
  type PhoneState,
  type CalendarState,
  type Controls,
  type Geometry,
  type NotifCenter,
  type NowPlaying,
  type PlanLimits,
  type Portfolio,
  type SystemStats,
  type Weather,
  type WinInfo,
  useAppearance,
  useAccent,
  frostColor,
} from "./ipc";

/** Dock item -> the Widgets switch that hides it (status, clock and apps always stay). */
const WIDGET_OF: Partial<Record<DockItem, keyof Widgets>> = {
  claude: "ai",
  media: "media",
  event: "calendar",
  whatsapp: "whatsapp",
  phone: "phone",
  portfolio: "portfolio",
  system: "system",
  battery: "battery",
};

type MenuItems = NonNullable<Parameters<typeof Menu.new>[0]>["items"];

function baseItems(): NonNullable<MenuItems> {
  return [
    { id: "settings", text: "Ledge settings", action: () => call.openSettings() },
      { id: "tm", text: "Task Manager", action: () => call.shell("taskManager") },
      { id: "desk", text: "Show desktop", action: () => call.shell("desktop") },
      { id: "bg", text: "Background apps (tray icons)", action: () => openFlyout("center", document.querySelector(".clock-tile"), 384, 640) },
      { item: "Separator" },
      { id: "quit", text: "Quit Ledge (restores the Windows taskbar)", action: () => call.quit() },
  ];
}

/** Hidden widgets, as a submenu that brings each one back. */
function restoreItems(cfg: Config, saveCfg: (c: Config) => Promise<void>): NonNullable<MenuItems> {
  const hidden = (Object.entries(WIDGET_OF) as [DockItem, keyof Widgets][]).filter(([, k]) => !cfg.widgets[k]);
  if (!hidden.length) return [];
  return [
    {
      text: "Show hidden widgets",
      items: [
        ...hidden.map(([id, k]) => ({ id: `show-${id}`, text: DOCK_LABELS[id], action: () => saveCfg({ ...cfg, widgets: { ...cfg.widgets, [k]: true } }) })),
        { item: "Separator" as const },
        { id: "show-all", text: "Show all", action: () => saveCfg({ ...cfg, widgets: { ...cfg.widgets, ...Object.fromEntries(hidden.map(([, k]) => [k, true])) } }) },
      ],
    },
  ];
}

async function barMenu(e: React.MouseEvent, cfg: Config | null, saveCfg: (c: Config) => Promise<void>) {
  e.preventDefault();
  const menu = await Menu.new({ items: [...(cfg ? restoreItems(cfg, saveCfg) : []), ...(cfg && restoreItems(cfg, saveCfg).length ? [{ item: "Separator" as const }] : []), ...baseItems()] });
  await menu.popup();
}

/** Right-click on a widget: hide it or move it, plus the dock's own menu. */
async function widgetMenu(e: React.MouseEvent, id: DockItem, order: DockItem[], cfg: Config, saveCfg: (c: Config) => Promise<void>) {
  e.preventDefault();
  e.stopPropagation();
  const key = WIDGET_OF[id];
  const i = order.indexOf(id);
  const move = (to: number) => {
    const next = [...order];
    next.splice(i, 1);
    next.splice(to, 0, id);
    saveCfg({ ...cfg, dockOrder: next });
  };
  const items: NonNullable<MenuItems> = [];
  const el = e.currentTarget.firstElementChild ?? e.currentTarget;
  if (id === "whatsapp") items.push({ id: "wa-list", text: "Recent messages", action: () => openFlyout("whatsapp", el, 360, 420) }, { item: "Separator" });
  if (key) items.push({ id: "hide", text: `Hide ${DOCK_LABELS[id]}`, action: () => saveCfg({ ...cfg, widgets: { ...cfg.widgets, [key]: false } }) });
  if (i > 0) items.push({ id: "left", text: "Move left", action: () => move(i - 1) });
  if (i < order.length - 1) items.push({ id: "right", text: "Move right", action: () => move(i + 1) });
  items.push({ item: "Separator" }, ...restoreItems(cfg, saveCfg), ...baseItems());
  const menu = await Menu.new({ items });
  await menu.popup();
}


/** Optional widgets, in the order they step aside when the screen is too narrow. */
const DROP_ORDER = ["event", "battery", "system", "phone", "whatsapp", "claude", "media", "portfolio"] as const;

export default function Dock() {
  const [cfg, saveCfg] = useConfig();
  const windows = useChannel<WinInfo[]>("windows") ?? [];
  const system = useChannel<SystemStats>("system");
  const media = useChannel<NowPlaying>("media");
  const ai = useChannel<AiUsage>("ai");
  const plan = useChannel<PlanLimits>("ai-plan");
  const weather = useChannel<Weather>("weather");
  const controls = useChannel<Controls>("controls");
  const notifs = useChannel<NotifCenter>("notifications");
  const portfolio = useChannel<Portfolio>("portfolio");
  const calendar = useChannel<CalendarState>("calendar");
  const phone = useChannel<PhoneState>("phone");
  const style = cfg?.appearance?.style ?? "black";
  const [voiceKey, setVoiceKey] = useState("");
  useEffect(() => {
    invoke<string>("voice_hotkey").then(setVoiceKey);
  }, []);
  const musicAccent = useAccent(cfg?.appearance?.accentFromMusic ? media?.art : null, "");
  useAppearance(cfg?.appearance, true, musicAccent || null);
  // The frost behind the dock takes the style's tint (live while previewing in Settings).
  const frost = frostColor(cfg?.appearance);
  useEffect(() => {
    if (cfg) invoke("dock_frost", { color: frost });
  }, [frost, !!cfg]);
  const dockRef = useRef<HTMLDivElement>(null);
  const startRef = useRef<HTMLButtonElement>(null);
  const [dropped, setDropped] = useState(0);
  const [maxW, setMaxW] = useState(1900);
  const sent = useRef(0);

  useEffect(() => {
    invoke<Geometry>("dock_geometry").then((g) => setMaxW(g.monitorWidth - 24));
  }, []);
  // New windows can make room tight again: re-try showing everything.
  useEffect(() => setDropped(0), [windows.length, maxW]);
  // Widgets dropped during a crowded moment (startup, many windows) come back once there's room.
  useEffect(() => {
    const t = setInterval(() => setDropped((d) => (d > 0 ? 0 : d)), 20_000);
    return () => clearInterval(t);
  }, []);

  // Size the window to the content, dropping optional widgets that don't fit.
  useLayoutEffect(() => {
    const el = dockRef.current;
    if (!el) return;
    const measure = () => {
      const natural = Math.ceil(el.scrollWidth);
      if (natural > maxW && dropped < DROP_ORDER.length) {
        setDropped((d) => d + 1);
        return;
      }
      const want = Math.min(natural, maxW);
      if (want !== sent.current) {
        sent.current = want;
        invoke("dock_resize", { width: want });
      }
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  if (!cfg) return <div className="dock-shell" />;
  const w = cfg.widgets;
  const gone = new Set<string>(DROP_ORDER.slice(0, dropped));
  const on: Record<DockItem, boolean> = {
    claude: w.ai,
    apps: true,
    media: w.media,
    event: w.calendar,
    whatsapp: w.whatsapp,
    phone: w.phone,
    portfolio: w.portfolio,
    system: w.system,
    battery: w.battery ?? true,
    status: true,
    clock: true,
  };
  const order = dockOrder(cfg.dockOrder);

  const render = (id: string) => {
    const el = widget(id);
    if (!el || id === "apps") return el;
    return (
      <span style={{ display: "contents" }} onContextMenu={(e) => widgetMenu(e, id as DockItem, order, cfg, saveCfg)}>
        {el}
      </span>
    );
  };

  const widget = (id: string) => {
    if (!on[id as DockItem] || gone.has(id)) return null;
    switch (id as DockItem) {
      case "claude":
        return <ClaudeCard ai={ai} plan={plan} />;
      case "apps":
        return (
          <div className="apps-group">
            <button
              ref={startRef}
              className="start"
              title="Apps"
              onClick={() => openFlyout("launcher", startRef.current, 560, 580)}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                call.shell("start");
              }}
            >
              <StartGlyph />
            </button>
            <button className="mic" title={`Talk to Ledge${voiceKey ? ` (${voiceKey})` : ""} — or just say “Hey Ledge”`} aria-label="Voice command" onClick={() => call.listen()}>
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                <rect x="9" y="3" width="6" height="11" rx="3" />
                <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
              </svg>
            </button>
            <Apps windows={windows} cfg={cfg} saveCfg={saveCfg} />
          </div>
        );
      case "media":
        return media ? <MediaCard m={media} /> : null;
      case "event":
        return <NextEventCard cal={calendar} h24={cfg.clock24} />;
      case "whatsapp":
        return <WhatsAppCard n={notifs} />;
      case "phone":
        return <PhoneCard phone={phone} c={controls} n={notifs} />;
      case "portfolio":
        return <PortfolioCard p={portfolio} />;
      case "system":
        return <SystemCard s={system} />;
      case "battery":
        return <BatteryRing s={system} />;
      case "status":
        return <StatusCluster c={controls} s={system} />;
      case "clock":
        return <ClockTile h24={cfg.clock24} w={w.weather ? weather : null} simple={cfg.appearance.clockStyle === "simple"} />;
    }
  };

  return (
    <div className={`dock-shell s-${style}`} onContextMenu={(e) => barMenu(e, cfg, saveCfg)}>
      <div className="glass" />
      <div className="dock" ref={dockRef}>
        {/* Every item can be dragged to a new place; the order is saved. */}
        <Sortable ids={order} onCommit={(ids) => saveCfg({ ...cfg, dockOrder: ids })} render={render} className="dock-row" />
      </div>
    </div>
  );
}
