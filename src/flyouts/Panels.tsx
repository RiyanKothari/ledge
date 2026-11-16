import { useEffect, useMemo, useRef, useState } from "react";
import { Bar, batteryOf, claudeView, connectedPhone, nowPosition, Ring } from "../components/DockWidgets";
import { ChatTile, ClaudeTile, CodexTile, Cpu, Down, FundTile, Next, Pause, Play, Prev, Search, Speaker, Up } from "../components/glyphs";
import {
  appLabel,
  call,
  fmtAgo,
  fmtClock,
  fmtINR,
  fmtPct,
  fmtRate,
  fmtSpan,
  fmtTokens,
  throttled,
  useAccent,
  useChannel,
  useConfig,
  useFrame,
  useIcon,
  useNow,
  type AiUsage,
  type AppEntry,
  type Controls,
  type NotifCenter,
  type NowPlaying,
  type PlanLimits,
  type PhoneState,
  type Portfolio,
  type SystemStats,
} from "../ipc";
import { NotifCard } from "./CenterPanel";
import { Section, Slider, Spinner } from "./common";

// ---------- media ----------

const setVolume = throttled((v: number) => call.volume(v / 100));

/** Seek bar with a soft wave riding on the played part. Drag or click to seek. */
function WaveSeek({ pct, playing, disabled, onSeek }: { pct: number; playing: boolean; disabled: boolean; onSeek: (p: number, done: boolean) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const at = (x: number) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.max(0, Math.min(100, ((x - r.left) / r.width) * 100));
  };
  const p = Math.max(0, Math.min(100, pct));
  return (
    <div
      ref={ref}
      className={`wave ${playing ? "playing" : ""} ${disabled ? "disabled" : ""}`}
      onPointerDown={(e) => {
        if (disabled) return;
        (e.target as Element).setPointerCapture(e.pointerId);
        onSeek(at(e.clientX), false);
      }}
      onPointerMove={(e) => e.buttons === 1 && !disabled && onSeek(at(e.clientX), false)}
      onPointerUp={(e) => !disabled && onSeek(at(e.clientX), true)}
    >
      <div className="wave-played" style={{ width: `${p}%` }}>
        <svg className="wave-svg" viewBox="0 0 200 24" preserveAspectRatio="none" aria-hidden>
          <path d="M0 24 C 12 10, 25 10, 37 18 S 62 6, 75 14 S 100 4, 112 15 S 137 8, 150 16 S 175 6, 200 13 L 200 24 Z" />
        </svg>
        <svg className="wave-svg back" viewBox="0 0 200 24" preserveAspectRatio="none" aria-hidden>
          <path d="M0 24 C 15 14, 28 8, 42 16 S 70 10, 85 17 S 110 6, 125 15 S 155 10, 170 17 S 190 9, 200 15 L 200 24 Z" />
        </svg>
      </div>
      <div className="wave-track" />
      <div className="wave-fill" style={{ width: `${p}%` }} />
      <div className="wave-thumb" style={{ left: `${p}%` }} />
    </div>
  );
}

const Shuffle = ({ size = 18 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5" />
  </svg>
);
const Repeat = ({ size = 18, one = false }: { size?: number; one?: boolean }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d="m17 2 4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3" />
    {one && <text x="12" y="14.5" fontSize="7" textAnchor="middle" fill="currentColor" stroke="none" fontWeight="700">1</text>}
  </svg>
);
const Cast = ({ size = 20 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d="M5 8.5 11 12l-6 3.5v-7Z" fill="currentColor" />
    <path d="M14 4.6a8 8 0 0 1 0 14.8M14 9a3.5 3.5 0 0 1 0 6" />
  </svg>
);

export function MediaPanel() {
  const m = useChannel<NowPlaying>("media");
  const c = useChannel<Controls>("controls");
  const s = useChannel<SystemStats>("system");
  const accent = useAccent(m?.art);
  const t = useFrame(!!m?.playing);
  const [seek, setSeek] = useState<number | null>(null);
  const [vol, setVol] = useState<number | null>(null);
  const [outputs, setOutputs] = useState(false);
  if (!m) return <p className="empty">Nothing is playing.</p>;
  const pos = seek ?? nowPosition(m, t);
  const pct = m.durationMs ? (pos / m.durationMs) * 100 : 0;
  const volume = vol ?? Math.round((s?.volume?.level ?? 0) * 100);
  const out = c?.outputs.find((o) => o.default)?.name.replace(/\s*\(.*\)$/, "") ?? "This PC";
  return (
    <div className="mp2" style={{ "--accent": accent } as React.CSSProperties}>
      {m.art && <div className="mp2-bg" style={{ backgroundImage: `url("${m.art}")` }} />}
      <div className="mp2-top">
        <span>{appLabel(m.sourceApp)}</span>
        <button className={`mp2-icon ${outputs ? "on" : ""}`} title={`Playing on ${out}`} onClick={() => setOutputs(!outputs)}>
          <Cast />
        </button>
      </div>
      <div className="mp2-title">
        {m.art && <img src={m.art} alt="" />}
        <b>{m.title}</b>
        {m.artist && (
          <>
            <i>•</i>
            <span>{m.artist}</span>
          </>
        )}
      </div>
      {m.durationMs > 0 && (
        <>
          <WaveSeek
            pct={pct}
            playing={m.playing}
            disabled={!m.canSeek}
            onSeek={(p, done) => {
              setSeek((p / 100) * m.durationMs);
              if (done) {
                call.media("seek", Math.round((p / 100) * m.durationMs));
                setTimeout(() => setSeek(null), 800);
              }
            }}
          />
          <div className="mp2-times">
            <span>{fmtClock(pos)}</span>
            <span>{fmtClock(m.durationMs)}</span>
          </div>
        </>
      )}
      <div className="mp2-ctrl">
        <button className={`mp2-side ${m.shuffle ? "on" : ""}`} disabled={m.shuffle == null} onClick={() => call.media("shuffle")} aria-label="Shuffle" title="Shuffle">
          <Shuffle />
          {m.shuffle && <i />}
        </button>
        <button className="mp2-round" onClick={() => call.media("previous")} aria-label="Previous">
          <Prev size={18} />
        </button>
        <button className="mp2-round play" onClick={() => call.media("toggle")} aria-label={m.playing ? "Pause" : "Play"}>
          {m.playing ? <Pause size={20} /> : <Play size={20} />}
        </button>
        <button className="mp2-round" onClick={() => call.media("next")} aria-label="Next">
          <Next size={18} />
        </button>
        <button
          className={`mp2-side ${m.repeat && m.repeat !== "none" ? "on" : ""}`}
          disabled={m.repeat == null}
          onClick={() => call.media("repeat")}
          aria-label="Repeat"
          title={m.repeat === "track" ? "Repeat this song" : m.repeat === "list" ? "Repeat all" : "Repeat off"}
        >
          <Repeat one={m.repeat === "track"} />
          {m.repeat && m.repeat !== "none" && <i />}
        </button>
      </div>
      {outputs && (
        <div className="mp2-out">
          <Slider
            icon={<Speaker size={15} level={volume / 100} muted={!!s?.volume?.muted} />}
            label={out}
            value={volume}
            onChange={(v) => {
              setVol(v);
              setVolume(v);
            }}
            onCommit={() => setTimeout(() => setVol(null), 1500)}
          />
          {c && c.outputs.length > 1 && (
            <div className="chips">
              {c.outputs.map((o) => (
                <button key={o.id} className={`chip ${o.default ? "on" : ""}`} onClick={() => call.output(o.id)}>
                  {o.name.replace(/\s*\(.*\)$/, "")}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function ClaudePanel() {
  const [cfg] = useConfig();
  const ai = useChannel<AiUsage>("ai");
  const plan = useChannel<PlanLimits>("ai-plan");
  const now = useNow(1000);
  const v = claudeView(ai, plan, now);
  const c = ai?.claude;
  const max = Math.max(...(c?.daily ?? [1]), 1);
  const days = (c?.daily ?? []).map((n, i, a) => ({ n, label: new Date(now - (a.length - 1 - i) * 86_400_000).toLocaleDateString([], { weekday: "narrow" }) }));
  const codex = ai?.codex;
  return (
    <div className="cp">
      <div className="cp-head">
        <ClaudeTile size={38} />
        <div>
          <b>Claude</b>
          <span className={`badge ${v && v.source !== "local" ? "ok" : ""}`}>
            {v?.source === "plan" ? (v.via === "claude.ai" ? "Exact · claude.ai" : "Plan limits · live") : "Estimate · Claude Code only"}
          </span>
        </div>
        {v?.live && <span className="live-dot">LIVE</span>}
      </div>
      {!v ? (
        <p className="empty">No Claude Code activity in the last 15 days.</p>
      ) : (
        <>
          <div className="cp-row">
            <div className="cp-row-top">
              <span>Session · 5h</span>
              <b>{v.source !== "local" ? `${Math.round(v.sessionPct)}%` : fmtTokens(c?.sessionTokens ?? 0)}</b>
            </div>
            <Bar pct={v.sessionPct} live={v.live} />
            <small>
              {v.limited ? "Limit reached · " : ""}
              {v.resetsAt && v.resetsAt > now ? `Resets in ${fmtSpan(v.resetsAt - now, true)}` : "No active window"}
              {v.source === "local"
                ? ` · ${Math.round(v.sessionPct)}% of your busiest 5h`
                : c
                  ? ` · ${fmtTokens(c.sessionTokens)} of ${c.fiveHourLimit ? fmtTokens(c.fiveHourLimit) : "?"} tokens`
                  : ""}
            </small>
          </div>
          <div className="cp-row">
            <div className="cp-row-top">
              <span>Weekly</span>
              <b>{v.weekExact ? `${Math.round(v.weekPct)}%` : fmtTokens(c?.weekTokens ?? 0)}</b>
            </div>
            <Bar pct={v.weekPct} tone="blue" />
            <small>
              {v.weekResetsAt && v.weekResetsAt > now
                ? `Resets ${new Date(v.weekResetsAt).toLocaleDateString([], { weekday: "short", hour: "numeric", minute: "2-digit" })}`
                : `${Math.round(v.weekPct)}% of your busiest 7 days`}
              {v.source === "plan" && c ? ` · ${fmtTokens(c.weekTokens)} tokens` : ""}
            </small>
          </div>
          {c && (
            <div className="cp-stats">
              <div><small>Most used</small><b>{c.topModel || "—"}</b><span>{fmtTokens(c.topModelTokens)}</span></div>
              <div><small>Burn rate</small><b>{fmtTokens(c.burnPerMin)}</b><span>tokens / min</span></div>
              <div><small>Last active</small><b>{c.lastActivity ? fmtAgo(c.lastActivity, now) : "—"}</b><span>{c.lastActivity ? new Date(c.lastActivity).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : ""}</span></div>
            </div>
          )}
          <div className="spark">
            {days.map((d, i) => (
              <div key={i} className={`spark-col ${i === days.length - 1 ? "today" : ""}`} title={fmtTokens(d.n)}>
                <i style={{ height: `${Math.max(3, (d.n / max) * 100)}%` }} />
                <span>{d.label}</span>
              </div>
            ))}
          </div>
        </>
      )}
      {v?.source === "calibrated" && c?.calibratedAt && (
        <p className="hint">
          Your 5-hour limit is about {fmtTokens(c.fiveHourLimit ?? 0)} tokens — learned when Claude last stopped you (
          {new Date(c.calibratedAt).toLocaleString([], { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" })}). The weekly bar
          {c.weekLimit ? " uses your weekly limit the same way." : " compares with your busiest 7 days until you hit a weekly limit."}
        </p>
      )}
      {v?.source === "local" && (
        <p className="hint">Percentages appear after the first time Claude tells you a limit was reached; until then bars compare with your busiest windows.</p>
      )}
      {cfg && !cfg.claudeWeb && (
        <div className="cp-connect">
          <p>
            These are estimates: chats in the Claude app and on claude.ai use the same limits but never reach Claude Code's logs.
            Sign in to claude.ai once inside Ledge to show the exact numbers from Settings → Usage.
          </p>
          <button className="primary" onClick={() => call.claudeConnect()}>
            Connect claude.ai
          </button>
        </div>
      )}
      {cfg?.claudeWeb && (
        <p className="hint">
          {plan?.error
            ? plan.error
            : plan?.fetchedAt
              ? `Exact numbers from claude.ai · updated ${fmtAgo(plan.fetchedAt, now)}`
              : "Reading your usage from claude.ai…"}{" "}
          <button className="link" onClick={() => (plan?.error?.includes("Signed out") ? call.claudeConnect() : call.claudeRefresh())}>
            {plan?.error?.includes("Signed out") ? "Sign in again" : "Refresh"}
          </button>
        </p>
      )}
      {codex?.available && codex.primary && Date.now() - codex.asOf < 14 * 86_400_000 && (
        <div className="cp-codex">
          <CodexTile size={26} />
          <span>Codex · {codex.planType ?? "ChatGPT"}</span>
          <b>{Math.round(codex.primary.usedPercent)}%</b>
        </div>
      )}
    </div>
  );
}

// ---------- RennSquare ----------

export function PortfolioPanel() {
  const p = useChannel<Portfolio>("portfolio");
  if (!p?.configured) return <p className="empty">Set the RennSquare live.json path in Ledge settings.</p>;
  if (p.error) return <p className="empty warn">{p.error}</p>;
  const up = p.totalDayPnl >= 0;
  const asOf = p.pricesAsOf ? new Date(p.pricesAsOf).toLocaleString([], { weekday: "short", day: "numeric", month: "short", hour: "numeric", minute: "2-digit" }) : "";
  return (
    <div className="pf">
      <div className="pf-head">
        <FundTile size={38} />
        <div>
          <small>RennSquare · {p.funds.length} paper funds</small>
          <b>{fmtINR(p.totalValue)}</b>
          <span className={`delta ${up ? "up" : "down"}`}>
            {fmtINR(p.totalDayPnl, true)} · {fmtPct(p.totalDayReturn)} today
          </span>
        </div>
      </div>
      {p.niftyDay != null && (
        <div className="pf-bench">
          <span>Nifty 50</span>
          <b className={p.niftyDay >= 0 ? "up" : "down"}>{fmtPct(p.niftyDay)}</b>
          <span>Funds beat it</span>
          <b>{p.funds.filter((f) => f.dayReturn > (p.niftyDay ?? 0)).length}/{p.funds.length}</b>
        </div>
      )}
      <ul className="pf-list">
        {p.funds.map((f) => (
          <li key={f.id}>
            <span className="pf-name">
              {f.name}
              <small>{f.positions} positions</small>
            </span>
            <span className="pf-val">{fmtINR(f.value)}</span>
            <span className={`pf-ret ${f.dayReturn >= 0 ? "up" : "down"}`}>{fmtPct(f.dayReturn)}</span>
          </li>
        ))}
      </ul>
      {p.alerts.length > 0 && (
        <Section title={`Alerts · ${p.badAlerts} urgent, ${p.warnAlerts} to watch`}>
          <ul className="pf-alerts">
            {p.alerts.slice(0, 5).map((a, i) => (
              <li key={i} className={a.level}>
                <b>{a.what}</b>
                <small>{a.detail}</small>
              </li>
            ))}
          </ul>
        </Section>
      )}
      <p className="hint">Prices as of {asOf}. Read-only from RennSquare.</p>
    </div>
  );
}

// ---------- WhatsApp ----------

export function WhatsAppPanel() {
  const n = useChannel<NotifCenter>("notifications");
  const now = useNow(30_000);
  const items = (n?.items ?? []).filter((x) => x.whatsapp);
  return (
    <div className="wp">
      <div className="cp-head">
        <ChatTile size={38} />
        <div>
          <b>WhatsApp</b>
          <span className="badge">{n?.whatsapp.count ?? 0} unread</span>
        </div>
      </div>
      {items.length === 0 ? (
        <p className="empty">No unread messages.</p>
      ) : (
        <div className="nstack">
          {items.slice(0, 8).map((x) => (
            <NotifCard
              key={x.id}
              n={x}
              now={now}
              onOpen={() => {
                call.openNotif(x.aumid, true);
                call.flyoutClose();
              }}
              onDismiss={() => call.dismiss(x.id)}
            />
          ))}
        </div>
      )}
      <button
        className="pill wide wa-open"
        onClick={() => {
          // WhatsApp Desktop (focused if it's already open), even for messages mirrored from the phone.
          call.openNotif(n?.whatsapp.open || "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App", true);
          call.flyoutClose();
        }}
      >
        Open WhatsApp
      </button>
    </div>
  );
}

// ---------- system ----------

function ProcRow({ name, exe, mb, max }: { name: string; exe: string; mb: number; max: number }) {
  const icon = useIcon(exe);
  return (
    <li>
      {icon ? <img src={icon} alt="" /> : <span className="app-fallback sm">{name.charAt(0)}</span>}
      <span className="proc-name">{name}</span>
      <span className="proc-bar"><i style={{ width: `${(mb / max) * 100}%` }} /></span>
      <span className="proc-mb">{mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`}</span>
    </li>
  );
}

export function SystemPanel() {
  const s = useChannel<SystemStats>("system");
  if (!s) return <p className="empty"><Spinner /></p>;
  const max = Math.max(...s.topRam.map((p) => p.mb), 1);
  return (
    <div className="sp">
      <div className="sp-big">
        <div>
          <small><Cpu size={12} /> CPU</small>
          <b>{s.cpu}%</b>
          <Bar pct={s.cpu} tone="blue" />
        </div>
        <div>
          <small>Memory</small>
          <b>{s.ram}%</b>
          <Bar pct={s.ram} tone="blue" />
          <span>{s.ramUsedGb.toFixed(1)} / {s.ramTotalGb.toFixed(0)} GB</span>
        </div>
      </div>
      <div className="sp-net">
        <span><Down size={12} /> {fmtRate(s.netDown)}</span>
        <span><Up size={12} /> {fmtRate(s.netUp)}</span>
      </div>
      <Section title="Top memory">
        <ul className="procs">
          {s.topRam.map((p) => (
            <ProcRow key={p.exe} {...p} max={max} />
          ))}
        </ul>
      </Section>
      <button className="pill wide" onClick={() => call.shell("taskManager")}>Task Manager</button>
    </div>
  );
}

// ---------- app launcher ----------

let appsCache: Promise<AppEntry[]> | null = null;

function LazyIcon({ id, name }: { id: string; name: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const io = new IntersectionObserver(([e]) => e.isIntersecting && setSeen(true));
    if (ref.current) io.observe(ref.current);
    return () => io.disconnect();
  }, []);
  const src = useIcon(seen ? `shell:AppsFolder\\${id}` : null);
  return <span ref={ref} className="la-icon">{src ? <img src={src} alt="" /> : <span className="app-fallback">{name.charAt(0)}</span>}</span>;
}

export function Launcher() {
  const [cfg] = useConfig();
  const [apps, setApps] = useState<AppEntry[] | null>(null);
  const [q, setQ] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    appsCache ??= call.apps();
    appsCache.then(setApps, () => setApps([]));
    input.current?.focus();
  }, []);
  const list = useMemo(() => {
    if (!apps) return [];
    const k = q.trim().toLowerCase();
    if (!k) return apps;
    const starts = apps.filter((a) => a.name.toLowerCase().startsWith(k));
    const has = apps.filter((a) => !a.name.toLowerCase().startsWith(k) && a.name.toLowerCase().includes(k));
    return [...starts, ...has];
  }, [apps, q]);
  const launch = (id: string) => {
    call.launch(`shell:AppsFolder\\${id}`);
    call.flyoutClose();
    setQ("");
  };
  return (
    <div className="la">
      <div className="la-search">
        <Search size={15} />
        <input
          ref={input}
          value={q}
          placeholder="Search apps"
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && list[0]) launch(list[0].id);
          }}
        />
      </div>
      {!q && cfg?.pins && cfg.pins.length > 0 && (
        <Section title="Pinned">
          <div className="la-grid">
            {cfg.pins.slice(0, 12).map((p) => (
              <PinTile key={p.path} name={p.name} path={p.path} target={p.target} />
            ))}
          </div>
        </Section>
      )}
      <Section title={q ? `${list.length} results` : "All apps"}>
        {!apps ? (
          <p className="empty"><Spinner /></p>
        ) : (
          <div className="la-grid scroll">
            {list.map((a) => (
              <button key={a.id} className="la-app" onClick={() => launch(a.id)} title={a.name}>
                <LazyIcon id={a.id} name={a.name} />
                <span>{a.name}</span>
              </button>
            ))}
          </div>
        )}
      </Section>
      <div className="la-foot">
        <button className="link" onClick={() => call.shell("start")}>Windows Start</button>
        <button className="link" onClick={() => call.openSettings()}>Ledge settings</button>
      </div>
    </div>
  );
}

function PinTile({ name, path, target }: { name: string; path: string; target?: string | null }) {
  const src = useIcon(path, target);
  return (
    <button
      className="la-app"
      onClick={() => {
        call.launch(path);
        call.flyoutClose();
      }}
      title={name}
    >
      <span className="la-icon">{src ? <img src={src} alt="" /> : <span className="app-fallback">{name.charAt(0)}</span>}</span>
      <span>{name}</span>
    </button>
  );
}

// ---------- phone ----------

export function PhonePanel() {
  const phone = useChannel<PhoneState>("phone");
  const c = useChannel<Controls>("controls");
  const n = useChannel<NotifCenter>("notifications");
  const now = useNow(30_000);
  const dev = connectedPhone(c);
  const level = batteryOf(phone, dev?.name);
  const items = (n?.items ?? []).filter((x) => x.aumid.includes("YourPhone"));
  const others = (phone?.batteries ?? []).filter((b) => b.name !== dev?.name && c?.btDevices.some((d) => d.connected && d.name === b.name));
  const ringColor = level == null ? "var(--dim)" : level <= 20 ? "var(--red)" : level <= 40 ? "var(--amber)" : "var(--green)";
  return (
    <div className="pp">
      <div className="pp-hero">
        <div className="pp-phone" style={phone?.wallpaper ? { backgroundImage: `url("${phone.wallpaper}")` } : undefined}>
          <span className="pp-notch" />
          <div className="pp-overlay">
            <Ring pct={level ?? 0} size={74} stroke={6} color={ringColor}>
              <span className="pp-pct">{level != null ? `${level}%` : "—"}</span>
            </Ring>
          </div>
        </div>
        <div className="pp-side">
          <b>{dev?.name ?? "No phone connected"}</b>
          <span className={dev ? "up" : "dim"}>{dev ? "Connected · Bluetooth" : "Connect it from Control Center"}</span>
          <small>Battery as your phone last reported it.</small>
          <button
            className="pill"
            onClick={() => {
              call.launch("shell:AppsFolder\\Microsoft.YourPhone_8wekyb3d8bbwe!App");
              call.flyoutClose();
            }}
          >
            Open Phone Link
          </button>
        </div>
      </div>
      {others.length > 0 && (
        <Section title="Also connected">
          <div className="pp-devs">
            {others.map((b) => (
              <div key={b.name} className="pp-dev">
                <Ring pct={b.level} size={30} stroke={3} color={b.level <= 20 ? "var(--red)" : "var(--green)"}>
                  <span className="ring-num">{b.level}</span>
                </Ring>
                <span>{b.name}</span>
              </div>
            ))}
          </div>
        </Section>
      )}
      <Section title={`From your phone${items.length ? ` · ${items.length}` : ""}`}>
        {items.length === 0 ? (
          <p className="empty">Nothing new.</p>
        ) : (
          <div className="nstack">
            {items.slice(0, 5).map((x) => (
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
          </div>
        )}
      </Section>
    </div>
  );
}
