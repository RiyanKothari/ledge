import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { StartGlyph } from "./components/glyphs";
import { call, useAppearance, useConfig, type Config } from "./ipc";
import { ACCENTS, STYLES } from "./looks";

const STEPS = ["Welcome", "Your taskbar", "Make it yours", "Voice", "Connect", "You're set"] as const;

function Switch({ on, onChange, label, hint }: { on: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <div className="w-row">
      <span>
        <b>{label}</b>
        {hint && <small>{hint}</small>}
      </span>
      <button className={`w-switch ${on ? "on" : ""}`} role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)}>
        <i />
      </button>
    </div>
  );
}

/** First-run tour: explains Ledge and sets it up as you go. Every choice applies at once. */
export default function Welcome() {
  const [cfg, save] = useConfig();
  const [step, setStep] = useState(0);
  const [voiceKey, setVoiceKey] = useState("");
  const [city, setCity] = useState("");
  useAppearance(cfg?.appearance);
  useEffect(() => {
    invoke<string>("voice_hotkey").then(setVoiceKey);
  }, []);
  useEffect(() => {
    if (cfg && !city) setCity(cfg.city);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cfg?.city]);
  if (!cfg) return null;
  const set = (patch: Partial<Config>) => save({ ...cfg, ...patch });
  const finish = async () => {
    await save({ ...cfg, city: city.trim() || cfg.city, onboarded: true });
    getCurrentWindow().close();
  };
  const last = step === STEPS.length - 1;

  return (
    <div className="welcome">
      <aside className="w-side">
        <div className="w-brand">
          <StartGlyph size={34} />
          <b>Ledge</b>
        </div>
        <ol>
          {STEPS.map((s, i) => (
            <li key={s} className={i === step ? "now" : i < step ? "done" : ""} onClick={() => setStep(i)}>
              <i>{i < step ? "✓" : i + 1}</i>
              {s}
            </li>
          ))}
        </ol>
        <button className="w-skip" onClick={finish}>
          Skip the tour
        </button>
      </aside>

      <main className="w-main" key={step}>
        {step === 0 && (
          <>
            <h1>A glass dock for your PC</h1>
            <p className="w-lead">
              Ledge replaces the Windows taskbar with a floating glass dock: your apps, live widgets (music, calendar, battery, notifications,
              weather…), its own Control Center, window previews and voice commands.
            </p>
            <div className="w-cards">
              <div>
                <b>Touch the bottom edge</b>
                <span>The dock slides up. Move away and it tucks itself out of sight.</span>
              </div>
              <div>
                <b>Click the clock</b>
                <span>Notifications, calendar, and apps running in the background.</span>
              </div>
              <div>
                <b>Click Wi-Fi / volume</b>
                <span>Ledge's Control Center: Wi-Fi, Bluetooth, brightness, sound.</span>
              </div>
            </div>
            <p className="w-note">This takes about a minute. Everything here can be changed later in Settings (right-click the dock).</p>
          </>
        )}

        {step === 1 && (
          <>
            <h1>Your taskbar</h1>
            <p className="w-lead">Ledge works best as your only taskbar. You can always get the Windows one back.</p>
            <div className="w-group">
              <Switch
                on={cfg.hideNativeTaskbar}
                onChange={(v) => set({ hideNativeTaskbar: v })}
                label="Hide the Windows taskbar"
                hint="Quitting Ledge (right-click the dock → Quit) always brings it back."
              />
              <Switch
                on={cfg.dockAutohide}
                onChange={(v) => set({ dockAutohide: v })}
                label="Auto-hide the dock"
                hint="Touch the bottom edge, or press Alt + ` anywhere, to bring it up."
              />
              <Switch on={cfg.startWithWindows} onChange={(v) => set({ startWithWindows: v })} label="Start Ledge when I sign in" />
            </div>
          </>
        )}

        {step === 2 && (
          <>
            <h1>Make it yours</h1>
            <p className="w-lead">Pick a dock style and a colour. Toggles, sliders and highlights wear the colour.</p>
            <div className="w-styles">
              {STYLES.map((st) => (
                <button key={st.id} className={`w-style ${cfg.appearance.style === st.id ? "on" : ""}`} onClick={() => set({ appearance: { ...cfg.appearance, style: st.id, ...st.look } })}>
                  <span style={{ background: st.preview }}>
                    <i />
                    <i />
                    <i />
                  </span>
                  {st.name}
                </button>
              ))}
            </div>
            <div className="w-dots">
              {ACCENTS.map((c) => (
                <button
                  key={c || "default"}
                  className={`w-dot ${cfg.appearance.accent === c ? "on" : ""}`}
                  style={{ background: c || "#3d8bff" }}
                  aria-label={c || "Blue"}
                  onClick={() => set({ appearance: { ...cfg.appearance, accent: c } })}
                />
              ))}
            </div>
          </>
        )}

        {step === 3 && (
          <>
            <h1>Talk to Ledge</h1>
            <p className="w-lead">
              Press <kbd>{voiceKey || "the mic"}</kbd> or the mic on the dock and say a command. Recognised by Windows on this PC — nothing is
              sent anywhere.
            </p>
            <div className="w-cards small">
              {["“Open WhatsApp”", "“Pause” · “Next song”", "“Volume 40”", "“Do not disturb on”", "“Show my calendar”", "“Wi-Fi off”"].map((t) => (
                <div key={t}>
                  <span>{t}</span>
                </div>
              ))}
            </div>
            <div className="w-group">
              <Switch
                on={cfg.wakeWord}
                onChange={(v) => set({ wakeWord: v })}
                label="Listen for “Hey Ledge”"
                hint="Say “Hey Ledge, open Chrome” without pressing anything. Keeps the microphone on while enabled."
              />
            </div>
            <button className="w-try" onClick={() => call.listen()}>
              Try it now
            </button>
            <p className="w-note">Needs English speech recognition in Windows (Settings → Time & language → Speech) and the microphone allowed for desktop apps.</p>
          </>
        )}

        {step === 4 && (
          <>
            <h1>Connect things (optional)</h1>
            <p className="w-lead">Ledge reads what's on this PC by itself. These are extras.</p>
            <div className="w-group">
              <div className="w-row">
                <span>
                  <b>Weather</b>
                  <small>Your city, for the clock and calendar.</small>
                </span>
                <input value={city} placeholder="e.g. Mumbai" onChange={(e) => setCity(e.target.value)} onBlur={() => city.trim() !== cfg.city && set({ city: city.trim() })} />
              </div>
              <div className="w-row">
                <span>
                  <b>Calendar</b>
                  <small>Paste a secret iCal link (Google / Outlook / iCloud) in Settings → Widgets & data. Or add events in Ledge's own calendar.</small>
                </span>
                <button className="w-ghost" onClick={() => call.openSettings()}>
                  Open Settings
                </button>
              </div>
              <div className="w-row">
                <span>
                  <b>Claude usage</b>
                  <small>If you use Claude: sign in once to show your exact 5-hour and weekly limits on the dock.</small>
                </span>
                <button className="w-ghost" onClick={() => call.claudeConnect()}>
                  Connect
                </button>
              </div>
            </div>
          </>
        )}

        {step === 5 && (
          <>
            <h1>You're set</h1>
            <p className="w-lead">A few things worth knowing:</p>
            <ul className="w-tips">
              <li>
                <b>Hover an open app</b> on the dock to see its windows; click one to switch.
              </li>
              <li>
                <b>Drag</b> apps and widgets to reorder them. <b>Right-click a widget</b> to hide or move it.
              </li>
              <li>
                <b>Right-click the dock</b> for Settings, Task Manager, and Quit (which restores the Windows taskbar).
              </li>
              <li>
                Something wrong with the taskbar? Run <code>ledge --restore-taskbar</code>, or restart Windows Explorer from Task Manager.
              </li>
            </ul>
          </>
        )}

        <footer className="w-foot">
          {step > 0 && (
            <button className="w-ghost" onClick={() => setStep(step - 1)}>
              Back
            </button>
          )}
          <span />
          <button className="w-next" onClick={() => (last ? finish() : setStep(step + 1))}>
            {last ? "Start using Ledge" : step === 0 ? "Get started" : "Continue"}
          </button>
        </footer>
      </main>
    </div>
  );
}
