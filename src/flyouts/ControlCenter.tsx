import { useEffect, useMemo, useState } from "react";
import { BatteryRing } from "../components/DockWidgets";
import {
  Bluetooth,
  Check,
  ChevronLeft,
  ChevronRight,
  Device,
  Gamepad,
  Gear,
  Headphones,
  Leaf,
  Lock,
  Moon,
  Person,
  Phone,
  Plane,
  Speaker,
  SunIcon,
  Wifi,
  WifiLevel,
} from "../components/glyphs";
import { call, throttled, useChannel, useConfig, type BtDevice, type Controls, type Network, type PhoneState, type SystemStats } from "../ipc";
import { Slider, Spinner } from "./common";
import { SystemPanel } from "./Panels";

type View = "main" | "wifi" | "bt" | "outputs" | "system";

/** Wide toggle: icon + name/state, and a chevron that opens its list. */
function Pill({
  icon,
  label,
  sub,
  on,
  busy,
  disabled,
  onToggle,
  onMore,
}: {
  icon: React.ReactNode;
  label: string;
  sub?: string;
  on: boolean;
  busy?: boolean;
  disabled?: boolean;
  onToggle: () => void;
  onMore?: () => void;
}) {
  return (
    <div className={`qp ${on ? "on" : ""} ${disabled ? "disabled" : ""} ${busy ? "busy" : ""}`}>
      <button className="qp-main" onClick={onToggle} disabled={disabled} aria-pressed={on} title={`${label}: ${on ? "on" : "off"}`}>
        <span className="qp-icon">{icon}</span>
        <span className="qp-text">
          <b>{label}</b>
          <small>{sub ?? (on ? "On" : "Off")}</small>
        </span>
      </button>
      {onMore && (
        <>
          <span className="qp-div" />
          <button className="qp-more" onClick={onMore} aria-label={`${label} options`}>
            <ChevronRight size={15} />
          </button>
        </>
      )}
    </div>
  );
}

/** Round toggle with a caption underneath. */
function Round({ icon, label, on, disabled, onToggle }: { icon: React.ReactNode; label: string; on: boolean; disabled?: boolean; onToggle: () => void }) {
  return (
    <div className={`qr ${on ? "on" : ""} ${disabled ? "disabled" : ""}`}>
      <button className="qr-btn" onClick={onToggle} disabled={disabled} aria-pressed={on} title={label}>
        {icon}
      </button>
      <span>{label}</span>
    </div>
  );
}

/** Runs an action, remembering the state we asked for until Windows reports it. */
function useOptimistic() {
  const [pending, setPending] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const run = async (key: string, want: boolean, fn: () => Promise<unknown>) => {
    setPending((p) => ({ ...p, [key]: want }));
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
      setPending((p) => {
        const { [key]: _, ...rest } = p;
        return rest;
      });
    }
    setTimeout(
      () =>
        setPending((p) => {
          const { [key]: _, ...rest } = p;
          return rest;
        }),
      2500,
    );
  };
  return { pending, error, setError, run };
}

const setBrightness = throttled((v: number) => call.brightness(v));
const setVolume = throttled((v: number) => call.volume(v / 100));

function MainView({ c, s, go }: { c: Controls; s: SystemStats | null; go: (v: View) => void }) {
  const { pending, error, run } = useOptimistic();
  const [cfg, saveCfg] = useConfig();
  const [bright, setBright] = useState<number | null>(null);
  const [vol, setVol] = useState<number | null>(null);
  const val = (k: string, real: boolean) => (k in pending ? pending[k] : real);
  const wifiOn = val("wifi", !!c.wifiOn);
  const btOn = val("bt", !!c.bluetoothOn);
  const connectedBt = c.btDevices.filter((d) => d.connected);
  const out = c.outputs.find((o) => o.default);
  const volume = vol ?? Math.round((s?.volume?.level ?? 0) * 100);
  const brightness = bright ?? c.brightness ?? 0;

  // Drop the local slider value once Windows has caught up.
  useEffect(() => {
    const t = setTimeout(() => setBright(null), 1800);
    return () => clearTimeout(t);
  }, [bright]);
  useEffect(() => {
    const t = setTimeout(() => setVol(null), 1800);
    return () => clearTimeout(t);
  }, [vol]);

  return (
    <div className="cc">
      <div className="qtoggles">
        <div className="qt-row">
          <Pill
            icon={<Wifi size={18} />}
            label="Wi-Fi"
            sub={!wifiOn ? "Off" : c.wifi.connected ? c.wifi.ssid : "Not connected"}
            on={wifiOn}
            disabled={c.wifiOn == null}
            onToggle={() => run("wifi", !wifiOn, () => call.radio("wifi", !wifiOn))}
            onMore={() => go("wifi")}
          />
          <Pill
            icon={<Bluetooth size={18} />}
            label="Bluetooth"
            sub={!btOn ? "Off" : connectedBt.length ? connectedBt.map((d) => d.name).join(", ") : "On"}
            on={btOn}
            disabled={c.bluetoothOn == null}
            onToggle={() => run("bt", !btOn, () => call.radio("bluetooth", !btOn))}
            onMore={() => go("bt")}
          />
        </div>
        <div className="qt-row rounds">
          <Round icon={<Plane size={18} />} label="Flight" on={val("flight", c.flight)} onToggle={() => run("flight", !c.flight, () => call.flight(!c.flight))} />
          <Round
            icon={<Leaf size={18} />}
            label={c.onBattery || c.energySaver ? "Saver" : "Saver (battery)"}
            on={val("saver", c.energySaver)}
            onToggle={() => run("saver", !c.energySaver, () => call.energySaver(!c.energySaver))}
          />
          <Round
            icon={<SunIcon size={18} />}
            label="Night light"
            on={val("night", !!c.nightLight)}
            disabled={c.nightLight == null}
            onToggle={() => run("night", !c.nightLight, () => call.nightLight(!c.nightLight))}
          />
          <Round icon={<Moon size={18} />} label="Focus" on={!!cfg?.dnd} onToggle={() => cfg && saveCfg({ ...cfg, dnd: !cfg.dnd })} />
          <Round icon={<Person size={18} />} label="Access" on={false} onToggle={() => call.launch("ms-settings:easeofaccess")} />
        </div>
      </div>

      {error && <div className="cc-error">{error}</div>}

      <div className="cc-sliders">
        <Slider
          icon={<SunIcon size={16} />}
          label="Brightness"
          value={brightness}
          disabled={c.brightness == null}
          right={c.brightness == null ? "External display" : undefined}
          onChange={(v) => {
            setBright(v);
            setBrightness(v);
          }}
        />
        <Slider
          icon={<Speaker size={16} level={volume / 100} muted={!!s?.volume?.muted} />}
          label={
            <button className="link" onClick={() => go("outputs")}>
              {out?.name.replace(/\s*\(.*\)$/, "") ?? "Speakers"} <ChevronRight size={11} />
            </button>
          }
          value={volume}
          onChange={(v) => {
            setVol(v);
            setVolume(v);
          }}
        />
      </div>

      <div className="cc-foot">
        <div className="cc-batt">
          <BatteryRing s={s} />
          {s?.battery && (
            <span>
              {s.battery.percent}%{s.battery.charging ? " · charging" : ""}
            </span>
          )}
        </div>
        <div className="cc-foot-right">
          {s && (
            <button className="chip" onClick={() => go("system")} title="CPU, memory and top apps">
              CPU {s.cpu}% · RAM {s.ram}%
            </button>
          )}
          <button className="icon-round" onClick={() => call.openSettings()} title="Ledge settings">
            <Gear size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}

function Header({ title, back, right }: { title: string; back: () => void; right?: React.ReactNode }) {
  return (
    <div className="sub-head">
      <button className="icon-round" onClick={back} aria-label="Back">
        <ChevronLeft size={16} />
      </button>
      <span>{title}</span>
      {right}
    </div>
  );
}

function Switch({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return <button className={`switch ${on ? "on" : ""}`} onClick={() => onChange(!on)} role="switch" aria-checked={on} />;
}

function WifiView({ c, back }: { c: Controls; back: () => void }) {
  const [nets, setNets] = useState<Network[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const scan = () => {
    setNets(null);
    call.wifiScan().then(setNets, () => setNets([]));
  };
  useEffect(() => {
    if (c.wifiOn) scan();
  }, [c.wifiOn]);

  const connect = async (n: Network, password?: string) => {
    setBusy(n.ssid);
    setMsg(null);
    try {
      await call.wifiConnect(n.ssid, password);
      setOpen(null);
      setPw("");
      scan();
    } catch (e) {
      setMsg(String(e));
    }
    setBusy(null);
  };

  return (
    <div className="sub">
      <Header title="Wi-Fi" back={back} right={<Switch on={!!c.wifiOn} onChange={(v) => call.radio("wifi", v)} />} />
      {!c.wifiOn ? (
        <p className="empty">Wi-Fi is off.</p>
      ) : nets == null ? (
        <p className="empty"><Spinner /> Looking for networks…</p>
      ) : nets.length === 0 ? (
        <p className="empty">
          No networks found. If you're sure there are some, turn on Settings → Privacy → Location → “Let desktop apps access your location”.
        </p>
      ) : (
        <ul className="list">
          {nets.map((n) => (
            <li key={n.ssid} className={`row ${n.connected ? "active" : ""}`}>
              <button
                className="row-main"
                onClick={() => {
                  if (n.connected) setOpen(open === n.ssid ? null : n.ssid);
                  else if (n.saved || !n.secure) connect(n);
                  else setOpen(open === n.ssid ? null : n.ssid);
                }}
              >
                <span className="row-icon"><WifiLevel signal={n.signal} size={17} /></span>
                <span className="row-text">
                  <b>{n.ssid}</b>
                  <small>{n.connected ? "Connected" : busy === n.ssid ? "Connecting…" : n.saved ? "Saved" : n.secure ? "Secured" : "Open"}</small>
                </span>
                {n.secure && <Lock size={12} />}
                {n.connected && <Check size={15} />}
                {busy === n.ssid && <Spinner />}
              </button>
              {open === n.ssid && n.connected && (
                <div className="row-actions">
                  <button className="pill" onClick={() => call.wifiDisconnect().then(scan)}>Disconnect</button>
                  <button className="pill ghost" onClick={() => call.wifiForget(n.ssid).then(scan)}>Forget</button>
                </div>
              )}
              {open === n.ssid && !n.connected && (
                <form
                  className="row-actions"
                  onSubmit={(e) => {
                    e.preventDefault();
                    connect(n, pw);
                  }}
                >
                  <input
                    autoFocus
                    type="password"
                    placeholder="Password"
                    value={pw}
                    onChange={(e) => setPw(e.target.value)}
                    autoComplete="off"
                  />
                  <button className="pill" disabled={busy != null || pw.length < 8}>Connect</button>
                </form>
              )}
            </li>
          ))}
        </ul>
      )}
      {msg && <div className="cc-error">{msg}</div>}
      {c.wifiOn && nets && (
        <button className="link center" onClick={scan}>
          Scan again
        </button>
      )}
    </div>
  );
}

const btIcon = (d: BtDevice) =>
  d.kind === "audio" ? <Headphones size={16} /> : d.kind === "phone" ? <Phone size={16} /> : d.kind === "input" ? <Gamepad size={16} /> : <Device size={16} />;

function BtView({ c, back }: { c: Controls; back: () => void }) {
  const phone = useChannel<PhoneState>("phone");
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  // Several identical controllers show as one row with a count.
  const devices = useMemo(() => {
    const seen = new Map<string, BtDevice & { n: number }>();
    for (const d of c.btDevices) {
      const prev = seen.get(d.name);
      if (prev) {
        prev.n++;
        prev.connected ||= d.connected;
      } else seen.set(d.name, { ...d, n: 1 });
    }
    return [...seen.values()];
  }, [c.btDevices]);
  const act = async (d: BtDevice) => {
    setBusy(d.name);
    setMsg(null);
    try {
      await call.btAudio(d.name, !d.connected);
    } catch (e) {
      setMsg(String(e));
    }
    setTimeout(() => setBusy(null), 2500);
  };
  return (
    <div className="sub">
      <Header title="Bluetooth" back={back} right={<Switch on={!!c.bluetoothOn} onChange={(v) => call.radio("bluetooth", v)} />} />
      {!c.bluetoothOn ? (
        <p className="empty">Bluetooth is off.</p>
      ) : (
        <ul className="list">
          {devices.map((d) => (
            <li key={d.name} className={`row ${d.connected ? "active" : ""}`}>
              <div className="row-main">
                <span className="row-icon">{btIcon(d)}</span>
                <span className="row-text">
                  <b>
                    {d.name}
                    {d.n > 1 && <em> ×{d.n}</em>}
                  </b>
                  <small>
                    {d.connected ? "Connected" : "Paired"}
                    {d.connected && phone?.batteries.find((b) => b.name === d.name) ? ` · ${phone.batteries.find((b) => b.name === d.name)!.level}% battery` : ""}
                  </small>
                </span>
                {d.kind === "audio" &&
                  (busy === d.name ? (
                    <Spinner />
                  ) : (
                    <button className={`pill ${d.connected ? "ghost" : ""}`} onClick={() => act(d)}>
                      {d.connected ? "Disconnect" : "Connect"}
                    </button>
                  ))}
              </div>
            </li>
          ))}
        </ul>
      )}
      {msg && <div className="cc-error">{msg}</div>}
      <button className="link center" onClick={() => call.launch("ms-settings:bluetooth")}>
        Pair a new device…
      </button>
    </div>
  );
}

function OutputsView({ c, s, back }: { c: Controls; s: SystemStats | null; back: () => void }) {
  const [vol, setVol] = useState<number | null>(null);
  const volume = vol ?? Math.round((s?.volume?.level ?? 0) * 100);
  return (
    <div className="sub">
      <Header title="Sound output" back={back} />
      <ul className="list">
        {c.outputs.map((o) => (
          <li key={o.id} className={`row ${o.default ? "active" : ""}`}>
            <button className="row-main" onClick={() => call.output(o.id)}>
              <span className="row-icon"><Speaker size={16} level={1} muted={false} /></span>
              <span className="row-text"><b>{o.name}</b></span>
              {o.default && <Check size={15} />}
            </button>
          </li>
        ))}
      </ul>
      <Slider
        label="Volume"
        value={volume}
        onChange={(v) => {
          setVol(v);
          setVolume(v);
        }}
        onCommit={() => setTimeout(() => setVol(null), 1500)}
      />
    </div>
  );
}

export function ControlCenter({ arg }: { arg?: string | null }) {
  const c = useChannel<Controls>("controls");
  const s = useChannel<SystemStats>("system");
  const [view, setView] = useState<View>((arg as View) || "main");
  useEffect(() => setView((arg as View) || "main"), [arg]);
  if (!c) return <p className="empty"><Spinner /></p>;
  const back = () => setView("main");
  if (view === "wifi") return <WifiView c={c} back={back} />;
  if (view === "bt") return <BtView c={c} back={back} />;
  if (view === "outputs") return <OutputsView c={c} s={s} back={back} />;
  if (view === "system")
    return (
      <div className="sub">
        <Header title="System" back={back} />
        <SystemPanel />
      </div>
    );
  return <MainView c={c} s={s} go={setView} />;
}
