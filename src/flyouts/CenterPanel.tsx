import { Menu } from "@tauri-apps/api/menu";
import { useMemo, useRef, useState } from "react";
import { Sortable } from "../components/Sortable";
import { ChatTile, ChevronLeft, ChevronRight, Close, Moon, WeatherGlyph } from "../components/glyphs";
import {
  call,
  openCentered,
  type TrayIcon,
  fmtAgo,
  fmtTime,
  useChannel,
  useConfig,
  useIcon,
  useNow,
  type CalEvent,
  type CalendarState,
  type Notif,
  type NotifCenter,
  type Weather,
} from "../ipc";
import { Section } from "./common";

/** Split-flap clock: each digit is a card with a hinge line. */
export function FlipClock({ h24 }: { h24: boolean }) {
  const now = useNow(1000);
  const d = new Date(now);
  let h = d.getHours();
  const pm = h >= 12;
  if (!h24) h = h % 12 || 12;
  const digits = `${String(h).padStart(2, "0")}${String(d.getMinutes()).padStart(2, "0")}`;
  return (
    <div className="flip">
      {digits.split("").map((ch, i) => (
        <span key={i} className={`flip-card ${i === 2 ? "gap" : ""}`}>
          <span key={ch} className="flip-digit">{ch}</span>
        </span>
      ))}
      {!h24 && <span className="flip-ampm">{pm ? "PM" : "AM"}</span>}
    </div>
  );
}

export function NotifCard({ n, now, onOpen, onDismiss }: { n: Notif; now: number; onOpen: () => void; onDismiss: () => void }) {
  const appIcon = useIcon(n.whatsapp || n.aumid.includes("YourPhone") ? null : `shell:AppsFolder\\${n.aumid}`);
  return (
    <div className="ncard" onClick={onOpen}>
      <div className="ncard-icon">
        {n.image ? <img src={n.image} alt="" /> : n.whatsapp ? <ChatTile size={34} /> : appIcon ? <img src={appIcon} alt="" /> : <span className="app-fallback">{n.app.charAt(0)}</span>}
      </div>
      <div className="ncard-body">
        <div className="ncard-meta">
          <span>{n.app}</span>
          <span>{fmtAgo(n.at, now)}</span>
        </div>
        <b>{n.title}</b>
        {n.body && <p>{n.body}</p>}
      </div>
      <button
        className="ncard-x"
        aria-label="Dismiss"
        onClick={(e) => {
          e.stopPropagation();
          onDismiss();
        }}
      >
        <Close size={11} />
      </button>
    </div>
  );
}

export const KIND_COLOR: Record<string, string> = {
  renewal: "#b77cff",
  payout: "#3ddc84",
  event: "#3b9dff",
  allday: "#7c7cff",
  due: "#ff9f2e",
  limit: "#5fd4ff",
};

/**
 * Background apps' tray icons, in a row you can slide (wheel or drag the
 * scrollbar) and rearrange (drag an icon). Click = what a click in the Windows
 * tray does; right-click = Ledge's menu with Quit, plus the app's own menu.
 */
export function TrayGrid() {
  const icons = useChannel<TrayIcon[]>("tray") ?? [];
  const [cfg, saveCfg] = useConfig();
  const [gone, setGone] = useState<Set<string>>(new Set());
  const strip = useRef<HTMLDivElement>(null);
  const order = cfg?.trayOrder ?? [];
  const rank = (exe: string) => {
    const i = order.indexOf(exe.toLowerCase());
    return i < 0 ? 1e6 : i;
  };
  const list = icons.filter((t) => !gone.has(t.id)).sort((a, b) => rank(a.exe) - rank(b.exe));
  const byId = new Map(list.map((t) => [t.id, t]));
  if (!list.length) return null;

  const name = (t: TrayIcon) => t.tip.split(/\r?\n/)[0] || (t.exe.split(/[\\/]/).pop() ?? "").replace(/\.exe$/i, "");
  const menu = async (t: TrayIcon) => {
    const items: NonNullable<Parameters<typeof Menu.new>[0]>["items"] = [
      { id: "h", text: name(t).slice(0, 60), enabled: false },
      { item: "Separator" },
      { id: "open", text: "Open", action: () => call.trayClick(t.id, "left") },
      { id: "app", text: "App menu…", action: () => call.trayClick(t.id, "right") },
    ];
    if (t.closable) {
      items.push({ item: "Separator" });
      items.push({
        id: "quit",
        text: `Quit ${name(t).slice(0, 40)}`,
        action: () => {
          // Gone from the strip at once; Ledge ends the app if it doesn't close itself.
          setGone((g) => new Set(g).add(t.id));
          call.trayQuit(t.id).catch(() => setGone((g) => {
            const n = new Set(g);
            n.delete(t.id);
            return n;
          }));
        },
      });
    }
    const m = await Menu.new({ items });
    await m.popup();
  };

  return (
    <Section title={`Running in the background · ${list.length}`}>
      <div
        className="tray-strip"
        ref={strip}
        onWheel={(e) => {
          if (strip.current) strip.current.scrollLeft += e.deltaY + e.deltaX;
        }}
      >
        <Sortable
          ids={list.map((t) => t.id)}
          className="tray-row"
          onCommit={(ids) => {
            if (!cfg) return;
            const exes = ids.map((id) => byId.get(id)?.exe.toLowerCase()).filter((x): x is string => !!x);
            // Keep apps that aren't running now in their old place at the end.
            saveCfg({ ...cfg, trayOrder: [...exes, ...order.filter((x) => !exes.includes(x))] });
          }}
          render={(id) => {
            const t = byId.get(id);
            if (!t) return null;
            return (
              <button
                className="tray-icon"
                title={t.tip || name(t)}
                onClick={() => call.trayClick(t.id, "left")}
                onDoubleClick={() => call.trayClick(t.id, "double")}
                onContextMenu={(e) => {
                  e.preventDefault();
                  menu(t);
                }}
              >
                <img src={t.icon} alt="" draggable={false} />
              </button>
            );
          }}
        />
      </div>
    </Section>
  );
}

export function ExpandGlyph({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" />
    </svg>
  );
}

const dayKey = (t: number) => {
  const d = new Date(t);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
};

function MonthGrid({ events, selected, onSelect }: { events: CalEvent[]; selected: Date; onSelect: (d: Date) => void }) {
  const [month, setMonth] = useState(() => new Date(selected.getFullYear(), selected.getMonth(), 1));
  const today = dayKey(Date.now());
  const busy = useMemo(() => {
    const s = new Map<string, string>();
    for (const e of events) if (!s.has(dayKey(e.start))) s.set(dayKey(e.start), KIND_COLOR[e.kind] ?? KIND_COLOR.event);
    return s;
  }, [events]);
  const first = new Date(month);
  const offset = (first.getDay() + 6) % 7; // Monday first
  const cells = Array.from({ length: 42 }, (_, i) => new Date(month.getFullYear(), month.getMonth(), i - offset + 1));
  return (
    <div className="cal">
      <div className="cal-head">
        <b>{month.toLocaleDateString([], { month: "long", year: "numeric" })}</b>
        <div>
          <button className="icon-round sm" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))}><ChevronLeft size={13} /></button>
          <button className="icon-round sm" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))}><ChevronRight size={13} /></button>
          <button className="icon-round sm" title="Open the full calendar" onClick={() => openCentered("calendar", 1000, 660)}><ExpandGlyph /></button>
        </div>
      </div>
      <div className="cal-grid">
        {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map((d) => (
          <span key={d} className="cal-dow">{d}</span>
        ))}
        {cells.map((d) => {
          const k = dayKey(d.getTime());
          return (
            <button
              key={k}
              className={`cal-day ${d.getMonth() !== month.getMonth() ? "out" : ""} ${k === today ? "today" : ""} ${k === dayKey(selected.getTime()) ? "sel" : ""}`}
              onClick={() => onSelect(d)}
            >
              {d.getDate()}
              {busy.has(k) && <i style={{ background: busy.get(k) }} />}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function CenterPanel() {
  const [cfg, saveCfg] = useConfig();
  const n = useChannel<NotifCenter>("notifications");
  const cal = useChannel<CalendarState>("calendar");
  const w = useChannel<Weather>("weather");
  const now = useNow(30_000);
  const [sel, setSel] = useState(() => new Date());
  const [all, setAll] = useState(false);
  const h24 = !!cfg?.clock24;
  const events = cal?.month?.length ? cal.month : (cal?.events ?? []);
  const dayEvents = events.filter((e) => dayKey(e.start) === dayKey(sel.getTime()) || (e.start < sel.getTime() && e.end > sel.getTime()));
  const items = n?.items ?? [];
  const shown = all ? items : items.slice(0, 3);

  return (
    <div className="center">
      <div className="center-hero">
        <FlipClock h24={h24} />
        <div className="hero-side">
          <button
            className={`dnd ${cfg?.dnd ? "on" : ""}`}
            title={cfg?.dnd ? "Do not disturb is on — banners are paused" : "Do not disturb"}
            onClick={() => cfg && saveCfg({ ...cfg, dnd: !cfg.dnd })}
          >
            <Moon size={13} /> {cfg?.dnd ? "Do not disturb · on" : "Do not disturb"}
          </button>
          <span className="hero-date">{new Date(now).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" })}</span>
          {w?.configured && w.temp != null && (
            <span className="hero-weather">
              <WeatherGlyph code={w.code} isDay={w.isDay} size={18} /> {Math.round(w.temp)}° · {w.place}
              {w.high != null && w.low != null && <em> H {Math.round(w.high)}° L {Math.round(w.low)}°</em>}
            </span>
          )}
        </div>
      </div>

      <Section
        title={`Notifications${items.length ? ` · ${items.length}` : ""}`}
        action={items.length > 0 && <button className="link" onClick={() => call.dismiss(null)}>Clear all</button>}
      >
        {!n?.available ? (
          <p className="empty">Notifications aren't readable on this PC.</p>
        ) : items.length === 0 ? (
          <p className="empty">You're all caught up.</p>
        ) : (
          <div className="nstack">
            {shown.map((x) => (
              <NotifCard
                key={x.id}
                n={x}
                now={now}
                onOpen={() => {
                  call.openNotif(x.aumid, x.whatsapp);
                  call.flyoutClose();
                }}
                onDismiss={() => call.dismiss(x.id)}
              />
            ))}
            {items.length > 3 && (
              <button className="link center" onClick={() => setAll(!all)}>
                {all ? "Show fewer" : `Show ${items.length - 3} more`}
              </button>
            )}
          </div>
        )}
      </Section>

      <TrayGrid />

      <MonthGrid events={events} selected={sel} onSelect={setSel} />

      <Section title={sel.toDateString() === new Date().toDateString() ? "Today" : sel.toLocaleDateString([], { weekday: "long", day: "numeric", month: "short" })}>
        {!cal?.configured ? (
          <button className="link" onClick={() => call.openSettings("apps")}>Connect Google, Outlook or iCloud calendar…</button>
        ) : dayEvents.length === 0 ? (
          <p className="empty">Nothing scheduled.</p>
        ) : (
          <ul className="agenda">
            {dayEvents.map((e, i) => {
              const live = !e.allDay && e.start <= Date.now() && e.end > Date.now();
              return (
                <li key={i} className={live ? "live" : ""}>
                  <span className="agenda-time">{e.allDay ? "All day" : fmtTime(e.start, h24)}</span>
                  <span className="agenda-bar" />
                  <span className="agenda-title">
                    {e.title || "(no title)"}
                    {!e.allDay && <small>{fmtTime(e.start, h24)} – {fmtTime(e.end, h24)}</small>}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
        {cal?.error && <p className="empty warn">{cal.error}</p>}
      </Section>
    </div>
  );
}
