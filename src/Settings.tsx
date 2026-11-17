import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import ConnectedApps from "./ConnectedApps";
import { ACCENTS, STYLES } from "./looks";
import { call, DOCK_LABELS, dockOrder, useAppearance, useIcon, type Appearance, type Config, type Pin, type Widgets } from "./ipc";


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

type Section = "appearance" | "dock" | "notifications" | "apps" | "widgets" | "general";
const SECTIONS: [Section, string, string][] = [
  ["appearance", "Appearance", "linear-gradient(150deg,#7dd3fc,#3b82f6)"],
  ["dock", "Dock", "linear-gradient(150deg,#93c5fd,#1d4ed8)"],
  ["notifications", "Notifications", "linear-gradient(150deg,#fca5a5,#e11d48)"],
  ["apps", "Connected apps", "linear-gradient(150deg,#c4b5fd,#7c3aed)"],
  ["widgets", "Widgets", "linear-gradient(150deg,#86efac,#16a34a)"],
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

function Segmented<T extends string>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="seg">
      {options.map(([v, label]) => (
        <button key={v} className={v === value ? "on" : ""} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export default function Settings() {
  // `saved` is what's stored; `draft` is what you're trying out. The dock and
  // panels preview the draft live; nothing is kept until "Save changes".
  const [saved, setSaved] = useState<Config | null>(null);
  const [draft, setDraft] = useState<Config | null>(null);
  const [status, setStatus] = useState("");
  const [tab, setTab] = useState<Section>("appearance");
  const draftRef = useRef<Config | null>(null);
  draftRef.current = draft;
  const [voiceKey, setVoiceKey] = useState("");
  useEffect(() => {
    invoke<string>("voice_hotkey").then(setVoiceKey);
    invoke<string | null>("settings_tab_take").then((t) => t && setTab(t as Section));
    const offTab = listen<string>("settings-tab", (e) => setTab(e.payload as Section));
    invoke<Config>("get_config").then((c) => {
      setSaved(c);
      setDraft(c);
    });
    // Outside changes (a widget hidden from the dock) — adopt them unless you're mid-edit.
    const off = listen<Config>("config", (e) => {
      setSaved(e.payload);
      const d = draftRef.current;
      setDraft((cur) => (!cur || (d && JSON.stringify(d) === JSON.stringify(cur)) ? e.payload : cur));
    });
    return () => {
      off.then((f) => f());
      offTab.then((f) => f());
    };
  }, []);
  useAppearance(draft?.appearance);
  const dirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved);

  // Live preview everywhere while you edit.
  useEffect(() => {
    if (draft && dirty) emit("config-preview", draft);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft]);

  const saveChanges = async () => {
    if (!draft) return;
    setStatus("Saving…");
    try {
      const stored = await invoke<Config>("save_config", { cfg: { ...draft, icsUrls: draft.icsUrls.map((u) => u.trim()).filter(Boolean) } });
      setSaved(stored);
      setDraft(stored);
      setStatus("Saved");
      setTimeout(() => setStatus(""), 1800);
    } catch (e) {
      setStatus(`Couldn't save: ${e}`);
    }
  };
  const discard = () => {
    if (!saved) return;
    setDraft(saved);
    emit("config-preview", saved);
    setStatus("");
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        saveChanges();
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });

  if (!draft) return null;
  const set = <K extends keyof Config>(k: K, v: Config[K], _slow = false) => {
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

            <h3>Personalise</h3>
            <div className="card-group">
              <div className="row">
                <span>
                  App icon size
                  <small>How big apps look on the dock.</small>
                </span>
                <Segmented value={draft.appearance.iconSize} options={[["s", "Small"], ["m", "Medium"], ["l", "Large"]]} onChange={(v) => look({ iconSize: v })} />
              </div>
              <div className="row">
                <span>
                  Clock
                  <small>Flip cards, or plain digits.</small>
                </span>
                <Segmented value={draft.appearance.clockStyle} options={[["flip", "Flip"], ["simple", "Simple"]]} onChange={(v) => look({ clockStyle: v })} />
              </div>
              <div className="row">
                <span>
                  Widget tiles
                  <small>Glass tiles, or flat on the dock.</small>
                </span>
                <Segmented value={draft.appearance.tiles} options={[["glass", "Glass"], ["flat", "Flat"]]} onChange={(v) => look({ tiles: v })} />
              </div>
              <div className="row">
                <span>
                  Banners appear
                  <small>Where notifications slide in.</small>
                </span>
                <Segmented value={draft.bannerPosition} options={[["left", "Left"], ["center", "Centre"], ["right", "Right"]]} onChange={(v) => set("bannerPosition", v)} />
              </div>
              <Toggle on={draft.appearance.hoverZoom} onChange={(v) => look({ hoverZoom: v })} label="Grow apps under the pointer" />
              <Toggle
                on={draft.appearance.accentFromMusic}
                onChange={(v) => look({ accentFromMusic: v })}
                label="Accent follows the music"
                hint="While something plays, the accent colour comes from its album art."
              />
              <Toggle on={draft.appearance.motion === "reduced"} onChange={(v) => look({ motion: v ? "reduced" : "full" })} label="Reduce motion" hint="Fewer animations, no springy bounces." />
            </div>

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

        {tab === "apps" && <ConnectedApps draft={draft} saved={saved} set={set} />}

        {tab === "widgets" && (
          <>
            <h2>Widgets</h2>
            <div className="card-group">
              {WIDGET_LABELS.map(([k, label]) => (
                <Toggle key={k} on={draft.widgets[k]} onChange={(v) => set("widgets", { ...draft.widgets, [k]: v })} label={label} />
              ))}
            </div>
            <p className="hint">Weather, calendars, Claude and the rest are set up in Connected apps.</p>
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
                hint="Say “Hey Ledge, what's the weather?” or “Hey Ledge, open WhatsApp”. Recognised on this PC by Windows; nothing is sent anywhere. Keeps the microphone on while enabled."
              />
              <Toggle on={draft.voiceReplies} onChange={(v) => set("voiceReplies", v)} label="Speak answers out loud" hint="Uses your Windows voice (Settings → Time & language → Speech)." />
            </div>
            <p className="hint">
              Or press <code>{voiceKey || "the mic on the dock"}</code> (or the mic on the dock) and just ask. Questions: “what time is it”, “what's the weather”,
              “how much battery”, “what's playing”, “what's next on my calendar”, “Claude usage”, “set a timer for ten minutes”. Commands: “open Chrome”, “close WhatsApp”, “close this window”, “pause”,
              “next song”, “volume forty”, “do not disturb on”, “Wi-Fi off”, “lock the computer”, “take a screenshot”.
            </p>
            <p className="hint">
              Ledge always puts the taskbar back when it quits. If it ever doesn't, run <code>ledge --restore-taskbar</code> or restart Windows Explorer.
            </p>
            <button className="ghost tour-btn" onClick={() => invoke("open_welcome")}>
              Show the welcome tour again
            </button>
            <br />
            <button className="danger" onClick={() => call.quit()}>
              Quit Ledge and restore the taskbar
            </button>
          </>
        )}
      </main>

      <div className={`s-savebar ${dirty ? "show" : ""}`} role="region" aria-label="Unsaved changes">
        <span>{status || "You have unsaved changes — you're seeing a preview."}</span>
        <button className="ghost" onClick={discard}>
          Discard
        </button>
        <button className="primary" onClick={saveChanges}>
          Save changes
        </button>
      </div>
    </div>
  );
}
