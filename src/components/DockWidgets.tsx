import { useEffect, useRef, useState } from "react";
import {
  appLabel,
  call,
  fmtINR,
  fmtPct,
  fmtSpan,
  fmtTokens,
  openFlyout,
  useAccent,
  useFrame,
  useNow,
  type AiUsage,
  type Controls,
  type NotifCenter,
  type NowPlaying,
  type PlanLimits,
  type Portfolio,
  type SystemStats,
  type Weather,
} from "../ipc";
import { Bluetooth, Bolt, ChatTile, ClaudeTile, FundTile, Next, Offline, Pause, Play, Prev, Speaker, WeatherGlyph, WifiLevel, Ethernet, Plane } from "./glyphs";

export const tone = (pct: number) => (pct >= 85 ? "red" : pct >= 60 ? "amber" : "green");

export function Bar({ pct, tone: t, live, thin }: { pct: number; tone?: string; live?: boolean; thin?: boolean }) {
  const p = Math.max(0, Math.min(100, pct));
  return (
    <span className={`meter ${thin ? "thin" : ""}`}>
      <span className={`fill ${t ?? tone(p)} ${live ? "live" : ""}`} style={{ width: `${Math.max(p, p > 0 ? 3 : 0)}%` }} />
    </span>
  );
}

export function Ring({ pct, size = 34, stroke = 3.4, color, children }: { pct: number; size?: number; stroke?: number; color?: string; children?: React.ReactNode }) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  return (
    <div className="ring" style={{ width: size, height: size }}>
      <svg width={size} height={size}>
        <circle cx={size / 2} cy={size / 2} r={r} className="ring-track" strokeWidth={stroke} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          className={`ring-arc ${color ? "" : tone(pct)}`}
          style={color ? { stroke: color } : undefined}
          strokeWidth={stroke}
          strokeDasharray={c}
          strokeDashoffset={c * (1 - Math.min(100, Math.max(0, pct)) / 100)}
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
        />
      </svg>
      <div className="ring-center">{children}</div>
    </div>
  );
}

// ---------- Claude ----------

export type ClaudeView = {
  sessionPct: number;
  weekPct: number;
  resetsAt: number | null;
  weekResetsAt: number | null;
  live: boolean;
  /**
   * "plan" = Anthropic's own numbers (CLI sign-in); "calibrated" = your real
   * limit, learned from the last time Anthropic refused a request; "local" =
   * compared with your own busiest windows.
   */
  source: "plan" | "calibrated" | "local";
  limited: boolean;
  /** The weekly number is a real share of the limit (not a comparison with your busiest week). */
  weekExact: boolean;
  /** Where "plan" numbers come from: "claude.ai" (matches Claude's Usage page) or "claude-code". */
  via: string;
};

export function claudeView(ai: AiUsage | null, plan: PlanLimits | null, now: number): ClaudeView | null {
  const c = ai?.claude;
  if (!c?.available && !plan?.fiveHour) return null;
  const live = !!c?.lastActivity && now - c.lastActivity < 3 * 60_000;
  if (plan?.enabled && plan.fiveHour) {
    return {
      sessionPct: plan.fiveHour.usedPercent,
      weekPct: plan.sevenDay?.usedPercent ?? 0,
      resetsAt: plan.fiveHour.resetsAt,
      weekResetsAt: plan.sevenDay?.resetsAt ?? null,
      live,
      source: "plan",
      limited: plan.fiveHour.usedPercent >= 100,
      weekExact: !!plan.sevenDay,
      via: plan.source,
    };
  }
  if (c?.fiveHourLimit) {
    const limited = !!c.limitedUntil && c.limitedUntil > now;
    const five = limited && c.limitedKind?.includes("five") ? 100 : (c.sessionTokens / c.fiveHourLimit) * 100;
    const week = c.weekLimit ? (c.weekTokens / c.weekLimit) * 100 : c.peakWeekTokens ? (c.weekTokens / c.peakWeekTokens) * 100 : 0;
    return {
      sessionPct: Math.min(100, five),
      weekPct: Math.min(100, limited && c.limitedKind?.includes("seven") ? 100 : week),
      resetsAt: limited ? c.limitedUntil : c.sessionResetsAt,
      weekResetsAt: null,
      live,
      source: "calibrated",
      limited,
      weekExact: !!c.weekLimit,
      via: "",
    };
  }
  return {
    sessionPct: c && c.peakSessionTokens ? (c.sessionTokens / c.peakSessionTokens) * 100 : 0,
    weekPct: c && c.peakWeekTokens ? (c.weekTokens / c.peakWeekTokens) * 100 : 0,
    resetsAt: c?.sessionResetsAt ?? null,
    weekResetsAt: null,
    live,
    source: "local",
    limited: false,
    weekExact: false,
    via: "",
  };
}

function resetLabel(at: number | null, now: number, long = false): string {
  if (!at || at <= now) return "";
  const ms = at - now;
  if (ms < 24 * 3600_000) return fmtSpan(ms);
  const d = new Date(at);
  return long ? d.toLocaleDateString([], { weekday: "short", hour: "numeric", minute: "2-digit" }) : `${d.toLocaleDateString([], { weekday: "short" })} ${d.getDate()}`;
}

export function ClaudeCard({ ai, plan }: { ai: AiUsage | null; plan: PlanLimits | null }) {
  const now = useNow(1000);
  const v = claudeView(ai, plan, now);
  const ref = useRef<HTMLButtonElement>(null);
  if (!v) return null;
  const c = ai?.claude;
  const exact = v.source !== "local";
  const rows: [string, number, string, string, boolean][] = [
    ["5h", v.sessionPct, resetLabel(v.resetsAt, now), exact ? `${Math.round(v.sessionPct)}%` : fmtTokens(c?.sessionTokens ?? 0), true],
    ["Week", v.weekPct, resetLabel(v.weekResetsAt, now), v.weekExact ? `${Math.round(v.weekPct)}%` : fmtTokens(c?.weekTokens ?? 0), false],
  ];
  return (
    <button
      ref={ref}
      className={`card claude ${v.live ? "is-live" : ""} ${v.limited ? "limited" : ""}`}
      onClick={() => openFlyout("claude", ref.current, 340, 420)}
      title={
        v.source === "plan"
          ? v.via === "claude.ai"
            ? "Exact — the same numbers as claude.ai → Settings → Usage"
            : "Claude plan limits, live from Anthropic"
          : "Estimate from Claude Code's logs. Connect claude.ai (click → Connect) for the exact numbers."
      }
    >
      <div className="tile-wrap">
        <ClaudeTile />
        {v.live && <span className="pulse" />}
      </div>
      <div className="claude-grid">
        {rows.map(([label, pct, reset, val, first]) => (
          <div className="claude-row" key={label}>
            <span className="lbl">{label}</span>
            <Bar pct={pct} live={first && v.live} tone={first ? undefined : "blue"} />
            <span className="dim">{reset}</span>
            <span className="val">{val}</span>
          </div>
        ))}
      </div>
    </button>
  );
}

// ---------- now playing ----------

export function nowPosition(m: NowPlaying, now: number) {
  if (!m.durationMs) return 0;
  const p = m.playing && m.updatedAt ? m.positionMs + (now - m.updatedAt) : m.positionMs;
  return Math.max(0, Math.min(m.durationMs, p));
}

export function EqBars({ on, color }: { on: boolean; color: string }) {
  return (
    <span className={`eq ${on ? "on" : ""}`} style={{ color }}>
      <i /><i /><i /><i />
    </span>
  );
}

export function MediaCard({ m }: { m: NowPlaying | null }) {
  const accent = useAccent(m?.art);
  const t = useFrame(!!m?.playing);
  const ref = useRef<HTMLDivElement>(null);
  if (!m) return null;
  const pct = m.durationMs ? (nowPosition(m, t) / m.durationMs) * 100 : 0;
  return (
    <div
      ref={ref}
      className={`card media ${m.playing ? "playing" : ""}`}
      style={{ "--accent": accent } as React.CSSProperties}
      onClick={() => openFlyout("media", ref.current, 420, 300)}
      title={`${m.title}${m.artist ? ` — ${m.artist}` : ""}`}
    >
      <div className="media-glow" />
      {m.art ? <img className="art" src={m.art} alt="" draggable={false} /> : <div className="art art-empty" />}
      <div className="track">
        <span className="t1">
          <EqBars on={m.playing} color={accent} />
          {m.title}
        </span>
        <span className="t2">{m.artist || appLabel(m.sourceApp)}</span>
      </div>
      <div className="controls" onClick={(e) => e.stopPropagation()}>
        <button onClick={() => call.media("previous")} aria-label="Previous"><Prev size={12} /></button>
        <button className="play" onClick={() => call.media("toggle")} aria-label={m.playing ? "Pause" : "Play"}>
          {m.playing ? <Pause size={13} /> : <Play size={13} />}
        </button>
        <button onClick={() => call.media("next")} aria-label="Next"><Next size={12} /></button>
      </div>
      {m.durationMs > 0 && (
        <div className="media-progress">
          <span style={{ width: `${pct}%` }} />
        </div>
      )}
    </div>
  );
}

// ---------- WhatsApp ----------

export function WhatsAppCard({ n }: { n: NotifCenter | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  const [bump, setBump] = useState(false);
  const count = n?.whatsapp.count ?? 0;
  const prev = useRef(count);
  useEffect(() => {
    if (count > prev.current) {
      setBump(true);
      const t = setTimeout(() => setBump(false), 900);
      prev.current = count;
      return () => clearTimeout(t);
    }
    prev.current = count;
  }, [count]);
  if (!n?.available) return null;
  const w = n.whatsapp;
  return (
    <button ref={ref} className={`card wa ${bump ? "bump" : ""}`} onClick={() => openFlyout("whatsapp", ref.current, 360, 420)} title="WhatsApp">
      <ChatTile />
      <div className="stat">
        <span className="big">{count}</span>
        <span className="t2">{count ? w.sender : "All caught up"}</span>
      </div>
    </button>
  );
}

// ---------- RennSquare ----------

export function PortfolioCard({ p }: { p: Portfolio | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  if (!p?.configured || p.error || !p.funds.length) return null;
  const up = p.totalDayPnl >= 0;
  const max = Math.max(...p.funds.map((f) => Math.abs(f.dayReturn)), 0.0001);
  return (
    <button ref={ref} className="card fund" onClick={() => openFlyout("portfolio", ref.current, 380, 520)} title="RennSquare funds">
      <FundTile />
      <div className="stat">
        <span className="big">{fmtINR(p.totalValue)}</span>
        <span className={`delta ${up ? "up" : "down"}`}>{up ? "▲" : "▼"} {fmtPct(p.totalDayReturn)}</span>
      </div>
      <div className="mini-bars" aria-hidden>
        {p.funds.slice(0, 12).map((f) => (
          <i key={f.id} className={f.dayReturn >= 0 ? "up" : "down"} style={{ height: `${18 + (Math.abs(f.dayReturn) / max) * 82}%` }} />
        ))}
      </div>
    </button>
  );
}

// ---------- system ----------

export function SystemCard({ s }: { s: SystemStats | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  if (!s) return null;
  return (
    <button ref={ref} className="card sys" onClick={() => openFlyout("system", ref.current, 330, 400)}>
      <Ring pct={s.cpu} size={32}>
        <span className="ring-num">{s.cpu}</span>
      </Ring>
      <div className="sys-rows">
        <div className="sys-row">
          <span className="lbl">CPU</span>
          <Bar pct={s.cpu} tone="blue" />
          <span className="val">{s.cpu}%</span>
        </div>
        <div className="sys-row">
          <span className="lbl">RAM</span>
          <Bar pct={s.ram} tone="blue" />
          <span className="val">{s.ram}%</span>
        </div>
      </div>
    </button>
  );
}

export function BatteryRing({ s }: { s: SystemStats | null }) {
  const b = s?.battery;
  if (!b) return null;
  const color = b.charging ? "var(--green)" : b.percent <= 15 ? "var(--red)" : b.percent <= 30 ? "var(--amber)" : "var(--green)";
  return (
    <div className="card batt-card" title={`Battery ${b.percent}%${b.charging ? " · charging" : ""}`}>
      <Ring pct={b.percent} size={34} stroke={3.6} color={color}>
        <span className="ring-num">
          {b.charging && <Bolt size={8} />}
          {b.percent}
        </span>
      </Ring>
    </div>
  );
}

// ---------- status cluster → Control Center ----------

export function StatusCluster({ c, s }: { c: Controls | null; s: SystemStats | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  const vol = s?.volume;
  const onWheel = (e: React.WheelEvent) => {
    if (!vol) return;
    const next = Math.max(0, Math.min(1, Math.round((vol.level + (e.deltaY < 0 ? 0.02 : -0.02)) * 50) / 50));
    call.volume(next);
  };
  const wifi = c?.flight ? (
    <Plane size={15} />
  ) : s?.netKind === "ethernet" ? (
    <Ethernet size={15} />
  ) : c?.wifi.connected ? (
    <WifiLevel signal={c.wifi.signal} size={15} />
  ) : (
    <Offline size={15} />
  );
  return (
    <button ref={ref} className="card status" onClick={() => openFlyout("control", ref.current, 360, 470)} onWheel={onWheel} title="Control Center">
      <span className={`st ${c?.wifi.connected ? "on" : ""}`}>{wifi}</span>
      {c?.bluetoothOn && (
        <span className={`st ${c.btDevices.some((d) => d.connected) ? "on" : ""}`}>
          <Bluetooth size={14} />
        </span>
      )}
      {vol && <Speaker size={15} level={vol.level} muted={vol.muted} />}
    </button>
  );
}

// ---------- clock tile ----------

export function ClockTile({ h24, w }: { h24: boolean; w: Weather | null }) {
  const now = useNow(1000);
  const ref = useRef<HTMLButtonElement>(null);
  const d = new Date(now);
  let h = d.getHours();
  if (!h24) h = h % 12 || 12;
  const hh = String(h).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return (
    <button ref={ref} className="clock-tile flipdock" onClick={() => openFlyout("center", ref.current, 384, 640)} title={d.toLocaleDateString([], { dateStyle: "full" })}>
      <span className="fd">
        <span className="fd-card">
          <span key={hh} className="fd-num">{hh}</span>
        </span>
        <span className="fd-card">
          <span key={mm} className="fd-num">{mm}</span>
        </span>
      </span>
      <span className="fd-side">
        <small>{d.toLocaleDateString([], { weekday: "short" }).toUpperCase()}</small>
        <b>{d.getDate()}</b>
        {w?.temp != null && <em title={w.place}>{Math.round(w.temp)}°</em>}
      </span>
    </button>
  );
}

export function WeatherMini({ w }: { w: Weather | null }) {
  if (!w?.configured || w.temp == null) return null;
  return (
    <div className="card weather" title={`${w.place}\nHigh ${Math.round(w.high ?? 0)}° · Low ${Math.round(w.low ?? 0)}°`}>
      <WeatherGlyph code={w.code} isDay={w.isDay} size={24} />
      <span className="temp">{Math.round(w.temp)}°</span>
    </div>
  );
}

// ---------- next event ----------

export function NextEventCard({ cal, h24 }: { cal: import("../ipc").CalendarState | null; h24: boolean }) {
  const now = useNow(15_000);
  const ref = useRef<HTMLButtonElement>(null);
  if (!cal?.configured) return null;
  const next = cal.events.find((e) => e.end > now && (e.allDay ? new Date(e.start).toDateString() === new Date(now).toDateString() || e.start > now : true));
  if (!next) return null;
  const d = new Date(next.start);
  const soon = !next.allDay && next.start > now && next.start - now < 15 * 60_000;
  const live = !next.allDay && next.start <= now;
  const when = next.allDay
    ? "All day"
    : live
      ? `Now · ends ${new Date(next.end).toLocaleTimeString([], { hour: h24 ? "2-digit" : "numeric", minute: "2-digit", hour12: !h24 })}`
      : `${d.toLocaleTimeString([], { hour: h24 ? "2-digit" : "numeric", minute: "2-digit", hour12: !h24 })} · in ${fmtSpan(next.start - now)}`;
  return (
    <button ref={ref} className={`card event ${soon ? "soon" : ""} ${live ? "live" : ""}`} onClick={() => openFlyout("center", ref.current, 384, 640)}>
      <span className="datetile">
        <span className="dow">{d.toLocaleDateString([], { weekday: "short" }).toUpperCase()}</span>
        <span className="dom">{d.getDate()}</span>
      </span>
      <span className="track">
        <span className="t1">{next.title || "(no title)"}</span>
        <span className="t2">{when}</span>
      </span>
    </button>
  );
}


// ---------- phone ----------

export function connectedPhone(c: Controls | null) {
  return c?.btDevices.find((d) => d.kind === "phone" && d.connected) ?? null;
}

export function batteryOf(phone: import("../ipc").PhoneState | null, name: string | undefined) {
  if (!phone || !name) return null;
  return phone.batteries.find((b) => b.name === name)?.level ?? null;
}

export function PhoneCard({ phone, c, n }: { phone: import("../ipc").PhoneState | null; c: Controls | null; n: NotifCenter | null }) {
  const ref = useRef<HTMLButtonElement>(null);
  const dev = connectedPhone(c);
  if (!phone || (!dev && !phone.wallpaper)) return null;
  const level = batteryOf(phone, dev?.name);
  const unread = (n?.items ?? []).filter((x) => x.aumid.includes("YourPhone")).length;
  const short = (dev?.name ?? "Phone").replace(/^.*?'s\s+/, "");
  return (
    <button ref={ref} className={`card phone ${dev ? "" : "off"}`} onClick={() => openFlyout("phone", ref.current, 320, 520)} title={dev ? `${dev.name} · connected` : "Phone not connected"}>
      <span className="mini-phone" style={phone.wallpaper ? { backgroundImage: `url("${phone.wallpaper}")` } : undefined}>
        {unread > 0 && <i className="badge-dot">{unread > 9 ? "9+" : unread}</i>}
      </span>
      <span className="stat">
        <span className="big">
          {level != null ? `${level}%` : dev ? "—" : "Off"}
        </span>
        <span className="t2">{short}</span>
      </span>
      {level != null && (
        <span className="phone-batt" aria-hidden>
          <i style={{ height: `${level}%` }} className={level <= 20 ? "low" : ""} />
        </span>
      )}
    </button>
  );
}
