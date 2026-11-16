import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { call, DOCK_LABELS, dockOrder, useAppearance, useConfig, useIcon, type Appearance, type Config, type DockStyle, type Pin, type Widgets } from "./ipc";

/** Each dock style sets the glass; tweaking a slider afterwards makes it "custom". */
const STYLES: { id: DockStyle; name: string; look: Omit<Appearance, "style" | "accent">; preview: string }[] = [
  { id: "black", name: "Black", look: { tint: "#0e0e12", opacity: 0.72, blur: 28 }, preview: "linear-gradient(180deg,#26262b,#141417)" },
  { id: "white", name: "White", look: { tint: "#f3f4f7", opacity: 0.58, blur: 30 }, preview: "linear-gradient(180deg,#ffffff,#e9eaef)" },
  {
    id: "liquid",
    name: "Liquid",
    look: { tint: "#ffffff", opacity: 0.05, blur: 0 },
    preview: "linear-gradient(135deg,rgba(255,255,255,.35),rgba(255,255,255,.06) 45%,rgba(255,255,255,.22)),linear-gradient(120deg,#3a6ea5,#a0587a 60%,#d99a4e)",
  },
  { id: "ios", name: "iOS", look: { tint: "#22314f", opacity: 0.5, blur: 30 }, preview: "linear-gradient(180deg,#4a6290,#2a3a5e)" },
  { id: "midnight", name: "Midnight", look: { tint: "#0b1024", opacity: 0.68, blur: 26 }, preview: "linear-gradient(180deg,#1c2238,#0b0f1e)" },
];

const ACCENTS = ["", "#5b5bd6", "#a855f7", "#f43f5e", "#ef4444", "#ea580c", "#16a34a", "#0d9488", "#8e8e98"];

const WIDGET_LABELS: [keyof Widgets, string][] = [
  ["ai", "Claude usage"],
  ["media", "Now playing"],
  ["whatsapp", "WhatsApp"],
  ["portfolio", "RennSquare portfolio"],
  ["phone", "Phone (battery, Phone Link)"],
  ["system", "CPU and memory"],
  ["battery", "Battery"],
  ["calendar", "Next event"],
  ["weather", "Weather"],
];

type Section = "appearance" | "dock" | "notifications" | "claude" | "widgets" | "general";
const SECTIONS: [Section, string, string][] = [
  ["appearance", "Appearance", "linear-gradient(150deg,#7dd3fc,#3b82f6)"],
  ["dock", "Dock", "linear-gradient(150deg,#93c5fd,#1d4ed8)"],
  ["notifications", "Notifications", "linear-gradient(150deg,#fca5a5,#e11d48)"],
  ["claude", "Claude", "linear-gradient(150deg,#f4a582,#d97757)"],
  ["widgets", "Widgets & data", "linear-gradient(150deg,#86efac,#16a34a)"],
  ["general", "General", "linear-gradient(150deg,#d4d4d8,#71717a)"],
];

const WIDGET_OF: Partial<Record<string, keyof Widgets>> = {
  claude: "ai",
  media: "media",
  event: "calendar",
  whatsapp: "whatsapp",
  phone: "phone",
  portfolio: "portfolio",
  system: "system",
  battery: "battery",
};

function PinRow({ pin, onRemove, onUp }: { pin: Pin; onRemove: () => void; onUp?: () => void }) {
  const src = useIcon(pin.path, pin.target);
  return (
    <li className="pin-row">
      {src ? <img src={src} alt="" /> : <span className="pin-dot" />}
      <span className="pin-name">{pin.name}</span>
      {onUp && (
        <button className="ghost" onClick={onUp} title="Move left">
          ↑
        </button>
      )}
      <button className="ghost" onClick={onRemove}>
        Remove
      </button>
    </li>
  );
}

function Toggle({ on, onChange, label, hint }: { on: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <div className="row toggle-row">
      <span>
        {label}
        {hint && <small>{hint}</small>}
      </span>
      <button className={`switch ${on ? "on" : ""}`} role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)}>
        <i />
      </button>
    </div>
  );
}

export default function Settings() {
  const [saved, save] = useConfig();
  const [draft, setDraft] = useState<Config | null>(null);
  const [status, setStatus] = useState("");
  const [tab, setTab] = useState<Section>("appearance");
  const lastEdit = useRef(0);
  // Typing waits for a pause; every switch, slider and swatch applies at once.
  const typing = useRef(false);
  const [voiceKey, setVoiceKey] = useState("");
  useEffect(() => {
    invoke<string>("voice_hotkey").then(setVoiceKey);
  }, []);
  useAppearance(draft?.appearance);

  // Seed from the stored config; pick up outside changes (e.g. a widget hidden
  // from the dock) unless you're in the middle of editing.
  useEffect(() => {
    if (saved && (!draft || Date.now() - lastEdit.current > 2500)) setDraft(saved);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved]);

  // Changes apply as you make them.
  useEffect(() => {
    if (!draft || !saved || JSON.stringify(draft) === JSON.stringify(saved)) return;
    const t = setTimeout(async () => {
      setStatus("Saving…");
      try {
        await save({ ...draft, icsUrls: draft.icsUrls.map((u) => u.trim()).filter(Boolean) });
        setStatus("Saved");
      } catch (e) {
        setStatus(`Couldn't save: ${e}`);
      }
    }, typing.current ? 500 : 0);
    return () => clearTimeout(t);
  }, [draft, saved, save]);

  if (!draft) return null;
  const set = <K extends keyof Config>(k: K, v: Config[K], slow = false) => {
    typing.current = slow;
    lastEdit.current = Date.now();
    setDraft({ ...draft, [k]: v });
  };
  const look = (a: Partial<Appearance>) => set("appearance", { ...draft.appearance, ...a });
  const pins = draft.pins ?? [];
  const liquid = draft.appearance.style === "liquid";

  return (
    <div className="settings2">
      <nav className="s-nav">
        <h1>Settings</h1>
        {SECTIONS.map(([id, name, color]) => (
          <button key={id} className={`s-tab ${tab === id ? "on" : ""}`} onClick={() => setTab(id)}>
            <i style={{ background: color }} />
            {name}
          </button>
        ))}
        <span className="s-status">{status}</span>
      </nav>

      <main className="s-main">
        {tab === "appearance" && (
          <>
            <h2>Appearance</h2>
            <h3>Dock style</h3>
            <div className="styles">
              {STYLES.map((st) => (
                <button key={st.id} className={`style-card ${draft.appearance.style === st.id ? "on" : ""}`} onClick={() => look({ style: st.id, ...st.look })}>
                  <span className={`style-prev p-${st.id}`} style={{ background: st.preview }}>
                    <i />
                    <i />
                    <i />
                  </span>
                  <b>{st.name}</b>
                </button>
              ))}
            </div>
            {liquid && <p className="hint">Liquid bends your wallpaper through the glass: the rim refracts what lies just beyond it, no blur. Over an open app, every style is live glass that takes on the app's colours.</p>}

            <h3>Colour</h3>
            <div className="card-group">
              <div className="row">
                <span>Accent</span>
                <div className="dots">
                  {ACCENTS.map((c) => (
                    <button
                      key={c || "default"}
                      className={`dot ${draft.appearance.accent === c ? "on" : ""}`}
                      style={{ background: c || "#3d8bff" }}
                      title={c || "Blue (default)"}
                      onClick={() => look({ accent: c })}
                    />
                  ))}
                  <input type="color" value={draft.appearance.accent || "#3d8bff"} onChange={(e) => look({ accent: e.target.value })} title="Any colour" />
                </div>
              </div>
            </div>
            <p className="hint">Toggles, sliders, bars, links and every selection wear it, including the Wi-Fi and Bluetooth lists.</p>

            <h3>Fine-tune</h3>
            <div className="card-group">
              <label className="row">
                <span>Glass opacity</span>
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={draft.appearance.opacity}
                  onChange={(e) => look({ style: liquid ? "liquid" : "custom", opacity: Number(e.target.value) })}
                />
                <em>{Math.round(draft.appearance.opacity * 100)}%</em>
              </label>
              <label className="row">
                <span>Tint</span>
                <input type="color" value={draft.appearance.tint} onChange={(e) => look({ style: liquid ? "liquid" : "custom", tint: e.target.value })} />
              </label>
            </div>
          </>
        )}

        {tab === "dock" && (
          <>
            <h2>Dock</h2>
            <div className="card-group">
              <Toggle on={draft.dockAutohide} onChange={(v) => set("dockAutohide", v)} label="Auto-hide" hint="Slides away; touch the bottom edge or press Alt + ` to bring it back." />
              <Toggle on={draft.clock24} onChange={(v) => set("clock24", v)} label="24-hour clock" />
            </div>
            <h3>Order and visibility</h3>
            <p className="hint">Drag items on the dock, or right-click any widget there to hide or move it.</p>
            <ul className="pins">
              {dockOrder(draft.dockOrder).map((id, i, all) => {
                const key = WIDGET_OF[id];
                return (
                  <li key={id} className="pin-row">
                    <span className="pin-name">{DOCK_LABELS[id]}</span>
                    {i > 0 && (
                      <button className="ghost" title="Move left" onClick={() => set("dockOrder", all.map((x, j) => (j === i - 1 ? id : j === i ? all[i - 1] : x)))}>
                        ↑
                      </button>
                    )}
                    {i < all.length - 1 && (
                      <button className="ghost" title="Move right" onClick={() => set("dockOrder", all.map((x, j) => (j === i + 1 ? id : j === i ? all[i + 1] : x)))}>
                        ↓
                      </button>
                    )}
                    {key && (
                      <button
                        className={`switch sm ${draft.widgets[key] ? "on" : ""}`}
                        role="switch"
                        aria-checked={draft.widgets[key]}
                        aria-label={`Show ${DOCK_LABELS[id]}`}
                        onClick={() => set("widgets", { ...draft.widgets, [key]: !draft.widgets[key] })}
                      >
                        <i />
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
            <h3>Pinned apps</h3>
            <p className="hint">Right-click any app on the dock to pin it; drag pinned apps to reorder.</p>
            <ul className="pins">
              {pins.map((p, i) => (
                <PinRow
                  key={p.path}
                  pin={p}
                  onRemove={() => set("pins", pins.filter((_, j) => j !== i))}
                  onUp={i > 0 ? () => set("pins", pins.map((q, j) => (j === i - 1 ? p : j === i ? pins[i - 1] : q))) : undefined}
                />
              ))}
            </ul>
          </>
        )}

        {tab === "notifications" && (
          <>
            <h2>Notifications</h2>
            <div className="card-group">
              <Toggle on={draft.widgets.banners} onChange={(v) => set("widgets", { ...draft.widgets, banners: v })} label="Banners" hint="New notifications slide in at the top right." />
              <Toggle on={draft.dnd} onChange={(v) => set("dnd", v)} label="Do not disturb" hint="Notifications are still collected; no banners." />
              <label className="row">
                <span>
                  Banner stays for
                  <small>Hover a banner to keep it. Click it to open the app.</small>
                </span>
                <input type="range" min={1} max={15} step={1} value={draft.bannerSeconds} onChange={(e) => set("bannerSeconds", Number(e.target.value))} />
                <em>{draft.bannerSeconds}s</em>
              </label>
            </div>
            <p className="hint">
              To stop Windows' own banners appearing too, turn on its Do not disturb (Settings → System → Notifications). Ledge still sees every
              notification.
            </p>
          </>
        )}

        {tab === "claude" && (
          <>
            <h2>Claude</h2>
            <div className="card-group">
              <div className="row">
                <span>
                  claude.ai usage{saved?.claudeWeb ? " · connected" : ""}
                  <small>The same 5-hour and weekly numbers as claude.ai → Settings → Usage. You sign in once in a Ledge window; Ledge never sees your password.</small>
                </span>
                <button className="primary" onClick={() => call.claudeConnect()}>
                  {saved?.claudeWeb ? "Sign in again" : "Connect"}
                </button>
              </div>
              {saved?.claudeWeb && <Toggle on={draft.claudeWeb} onChange={(v) => set("claudeWeb", v)} label="Use claude.ai numbers" />}
              <Toggle
                on={draft.claudeLiveLimits}
                onChange={(v) => set("claudeLiveLimits", v)}
                label="Command-line sign-in"
                hint="Uses ~/.claude/.credentials.json when the command-line Claude Code made one, sent only to api.anthropic.com."
              />
            </div>
            <p className="hint">
              Not connected, Ledge estimates from Claude Code's own logs (it learns your 5-hour limit the last time Claude Code was told “limit reached”).
              Chats in the Claude app aren't in those logs, so the estimate runs low when you use both.
            </p>
          </>
        )}

        {tab === "widgets" && (
          <>
            <h2>Widgets & data</h2>
            <div className="card-group">
              {WIDGET_LABELS.map(([k, label]) => (
                <Toggle key={k} on={draft.widgets[k]} onChange={(v) => set("widgets", { ...draft.widgets, [k]: v })} label={label} />
              ))}
            </div>
            <h3>Weather</h3>
            <div className="card-group">
              <label className="row">
                <span>City</span>
                <input value={draft.city} placeholder="e.g. Mumbai" onChange={(e) => set("city", e.target.value, true)} />
              </label>
              <Toggle on={draft.fahrenheit} onChange={(v) => set("fahrenheit", v)} label="Fahrenheit" />
            </div>
            {saved?.placeLabel && saved.city === draft.city && <p className="hint">Showing weather for {saved.placeLabel}.</p>}
            <h3>Calendars</h3>
            <p className="hint">
              One secret iCal address per line (Google: calendar settings → “Secret address in iCal format”; Outlook: Publish → ICS; iCloud: public calendar).
              Events you add in Ledge's own calendar live on this PC.
            </p>
            <textarea
              rows={3}
              spellCheck={false}
              value={draft.icsUrls.join("\n")}
              placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
              onChange={(e) => set("icsUrls", e.target.value.split("\n"), true)}
            />
            <h3>RennSquare</h3>
            <div className="card-group">
              <label className="row">
                <span>live.json</span>
                <input value={draft.portfolioPath} spellCheck={false} onChange={(e) => set("portfolioPath", e.target.value, true)} />
              </label>
            </div>
          </>
        )}

        {tab === "general" && (
          <>
            <h2>General</h2>
            <div className="card-group">
              <Toggle on={draft.startWithWindows} onChange={(v) => set("startWithWindows", v)} label="Start Ledge when I sign in" hint="This is what makes Ledge your taskbar." />
              <Toggle on={draft.hideNativeTaskbar} onChange={(v) => set("hideNativeTaskbar", v)} label="Hide the Windows taskbar while Ledge runs" />
            </div>
            <h3>Voice</h3>
            <div className="card-group">
              <Toggle
                on={draft.wakeWord}
                onChange={(v) => set("wakeWord", v)}
                label="Listen for “Hey Ledge”"
                hint="Say “Hey Ledge, open WhatsApp”. Recognised on this PC by Windows; nothing is sent anywhere. Keeps the microphone on while enabled."
              />
            </div>
            <p className="hint">
              Or press <code>{voiceKey || "the mic on the dock"}</code> (or the mic on the dock) and say a command: “open Chrome”, “pause”, “next song”, “volume 40”,
              “do not disturb on”, “Wi-Fi off”, “brightness 70”, “show my calendar”, “Claude usage”, “show desktop”.
            </p>
            <p className="hint">
              Ledge always puts the taskbar back when it quits. If it ever doesn't, run <code>ledge --restore-taskbar</code> or restart Windows Explorer.
            </p>
            <button className="danger" onClick={() => call.quit()}>
              Quit Ledge and restore the taskbar
            </button>
          </>
        )}
      </main>
    </div>
  );
}
