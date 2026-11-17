import { useState, type ReactNode } from "react";
import { ChatTile, ClaudeTile, CodexTile, FundTile, Phone, Play, WeatherGlyph } from "./components/glyphs";
import { call, useChannel, useIcon, type AiUsage, type CalendarState, type Config, type NowPlaying, type PhoneState, type PlanLimits, type Portfolio, type Weather } from "./ipc";

const WHATSAPP_APP = "shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App";
const PHONE_LINK = "shell:AppsFolder\\Microsoft.YourPhone_8wekyb3d8bbwe!App";

type State = "on" | "off" | "warn";
type Set = <K extends keyof Config>(k: K, v: Config[K]) => void;

function Card({ logo, name, state, status, about, actions, children }: {
  logo: ReactNode;
  name: string;
  state: State;
  status: string;
  about?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <section className={`ca-card ${state}`}>
      <header className="ca-head">
        <span className="ca-logo">{logo}</span>
        <span className="ca-title">
          <b>{name}</b>
          <span className="ca-status">
            <i />
            {status}
          </span>
        </span>
        {actions && <span className="ca-actions">{actions}</span>}
      </header>
      {about && <p className="ca-about">{about}</p>}
      {children && <div className="ca-body">{children}</div>}
    </section>
  );
}

const Tile = ({ bg, children }: { bg: string; children: ReactNode }) => (
  <div className="tile" style={{ width: 36, height: 36, background: bg, display: "grid", placeItems: "center", color: "#fff" }}>
    {children}
  </div>
);

const CalendarLogo = () => (
  <Tile bg="linear-gradient(150deg,#ff8a7a,#e5484d)">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" aria-hidden>
      <rect x="3.5" y="5" width="17" height="15" rx="3" />
      <path d="M3.5 10h17M8 3v4M16 3v4" />
    </svg>
  </Tile>
);

const pct = (n: number) => `${Math.round(n)}%`;
const hours = (m: number) => (m >= 1440 ? "week" : `${Math.round(m / 60)}-hour`);

/** Calendar links are secrets: show only where they come from. */
function calendarName(url: string): string {
  try {
    const u = new URL(url);
    if (u.host.includes("google")) return "Google Calendar";
    if (u.host.includes("outlook") || u.host.includes("office") || u.host.includes("live.com")) return "Outlook";
    if (u.host.includes("icloud")) return "iCloud";
    return u.host;
  } catch {
    return "Calendar link";
  }
}

/** Settings → Connected apps: every service Ledge talks to, its live status, and how to connect it. */
export default function ConnectedApps({ draft, saved, set }: { draft: Config; saved: Config | null; set: Set }) {
  const plan = useChannel<PlanLimits>("ai-plan");
  const ai = useChannel<AiUsage>("ai");
  const cal = useChannel<CalendarState>("calendar");
  const weather = useChannel<Weather>("weather");
  const media = useChannel<NowPlaying>("media");
  const phone = useChannel<PhoneState>("phone");
  const portfolio = useChannel<Portfolio>("portfolio");
  const whatsapp = !!useIcon(WHATSAPP_APP);
  const [link, setLink] = useState("");

  // Claude
  const claudeOn = !!saved?.claudeWeb;
  const claudeState: State = claudeOn ? (plan?.error ? "warn" : "on") : "off";
  const claudeStatus = !claudeOn
    ? "Not connected"
    : plan?.error
      ? plan.error
      : plan?.fiveHour
        ? `Connected · 5-hour ${pct(plan.fiveHour.usedPercent)}${plan.sevenDay ? ` · week ${pct(plan.sevenDay.usedPercent)}` : ""}`
        : "Connected · fetching your usage";

  // ChatGPT (via Codex's own usage log on this PC)
  const codex = ai?.codex;
  const codexOn = !!codex?.available && !!codex.primary;
  const codexStatus = codexOn
    ? `Connected · ${codex!.planType ? `${codex!.planType[0].toUpperCase()}${codex!.planType.slice(1)} · ` : ""}${hours(codex!.primary!.windowMinutes)} ${pct(codex!.primary!.usedPercent)}${codex!.secondary ? ` · ${hours(codex!.secondary.windowMinutes)} ${pct(codex!.secondary.usedPercent)}` : ""}`
    : "Not found on this PC";

  // Calendars
  const urls = draft.icsUrls.filter((u) => u.trim());
  const calState: State = urls.length === 0 ? "off" : cal?.error ? "warn" : "on";
  const calStatus = urls.length === 0 ? "No calendars linked" : cal?.error ? cal.error : `${urls.length} calendar${urls.length === 1 ? "" : "s"} linked`;
  const addLink = () => {
    const u = link.trim();
    if (!/^(https?|webcal):\/\//i.test(u)) return;
    set("icsUrls", [...urls, u.replace(/^webcal:/i, "https:")]);
    setLink("");
  };

  // Weather
  const wOn = !!draft.city.trim();
  const wStatus = !wOn ? "Set your city" : weather?.error ? weather.error : weather?.temp != null ? `${weather.place || draft.city} · ${Math.round(weather.temp)}${weather.unit}` : "Connected";

  const battery = phone?.batteries?.[0];

  return (
    <>
      <h2>Connected apps</h2>
      <p className="hint">The services Ledge reads from. Each one only sends what it needs, and only to that service.</p>

      <Card
        logo={<ClaudeTile size={36} />}
        name="Claude"
        state={claudeState}
        status={claudeStatus}
        about="Your exact 5-hour and weekly limits, as on claude.ai → Settings → Usage. You sign in once in a Ledge window; Ledge never sees your password."
        actions={
          <>
            {claudeOn && (
              <button className="ghost" onClick={() => set("claudeWeb", false)}>
                Disconnect
              </button>
            )}
            <button className="primary" onClick={() => call.claudeConnect()}>
              {claudeOn ? "Sign in again" : "Connect"}
            </button>
          </>
        }
      >
        <label className="ca-check">
          <input type="checkbox" checked={draft.claudeLiveLimits} onChange={(e) => set("claudeLiveLimits", e.target.checked)} />
          Also use Claude Code's command-line sign-in (sent only to api.anthropic.com)
        </label>
      </Card>

      <Card
        logo={<CodexTile size={36} />}
        name="ChatGPT"
        state={codexOn ? "on" : "off"}
        status={codexStatus}
        about={
          codexOn
            ? "Your ChatGPT plan's usage limits, read from the Codex app on this PC. Nothing to sign in."
            : "Ledge reads your ChatGPT plan's usage limits from Codex on this PC. Sign in to the Codex app or command line with your ChatGPT account and use it once. (ChatGPT doesn't publish usage numbers for ordinary chats.)"
        }
        actions={
          <button className="ghost" onClick={() => call.launch("https://chatgpt.com/codex")}>
            Open Codex
          </button>
        }
      />

      <Card
        logo={<CalendarLogo />}
        name="Calendars"
        state={calState}
        status={calStatus}
        about="Google, Outlook or iCloud, by each calendar's private iCal link. Events you add in Ledge stay on this PC."
      >
        {urls.length > 0 && (
          <ul className="ca-list">
            {urls.map((u, i) => (
              <li key={i}>
                <span>{calendarName(u)}</span>
                <small>…{u.slice(-18)}</small>
                <button className="ghost" onClick={() => set("icsUrls", urls.filter((_, j) => j !== i))}>
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="ca-add">
          <input
            value={link}
            spellCheck={false}
            placeholder="Paste a calendar's iCal link (…/basic.ics)"
            onChange={(e) => setLink(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addLink()}
          />
          <button className="primary" disabled={!/^(https?|webcal):\/\//i.test(link.trim())} onClick={addLink}>
            Add
          </button>
        </div>
        <div className="ca-links">
          <span>Get the link:</span>
          <button className="ghost" onClick={() => call.launch("https://calendar.google.com/calendar/r/settings")}>
            Google
          </button>
          <button className="ghost" onClick={() => call.launch("https://outlook.live.com/calendar/0/options/calendar/SharedCalendars")}>
            Outlook
          </button>
          <button className="ghost" onClick={() => call.launch("https://www.icloud.com/calendar/")}>
            iCloud
          </button>
        </div>
        <p className="ca-tip">
          Google: open a calendar's settings → “Secret address in iCal format”. Outlook: Shared calendars → Publish → ICS. iCloud: share the calendar as a public
          calendar.
        </p>
      </Card>

      <Card
        logo={
          <Tile bg="linear-gradient(150deg,#7cc8ff,#2f7bf5)">
            <WeatherGlyph code={weather?.code ?? 1} isDay={weather?.isDay ?? true} size={22} />
          </Tile>
        }
        name="Weather"
        state={wOn ? (weather?.error ? "warn" : "on") : "off"}
        status={wStatus}
        about="From Open-Meteo (free, no account). Only your city is sent."
      >
        <div className="ca-add">
          <input value={draft.city} placeholder="Your city, e.g. Chennai" onChange={(e) => set("city", e.target.value)} />
          <label className="ca-check">
            <input type="checkbox" checked={draft.fahrenheit} onChange={(e) => set("fahrenheit", e.target.checked)} />
            °F
          </label>
        </div>
      </Card>

      <Card
        logo={<ChatTile size={36} />}
        name="WhatsApp"
        state={whatsapp ? "on" : "off"}
        status={whatsapp ? "Connected · WhatsApp Desktop" : "WhatsApp Desktop isn't installed"}
        about="Unread chats on the dock and in banners, opened straight in the WhatsApp app. Reads Windows' notifications; nothing leaves this PC."
        actions={
          !whatsapp && (
            <button className="ghost" onClick={() => call.launch("ms-windows-store://pdp/?productid=9NKSQGP7F2NH")}>
              Get WhatsApp
            </button>
          )
        }
      />

      <Card
        logo={
          <Tile bg="linear-gradient(150deg,#5ee27f,#1db954)">
            <Play size={18} />
          </Tile>
        }
        name="Spotify & media"
        state={media?.title ? "on" : "off"}
        status={media?.title ? `Playing · ${media.title}${media.sourceApp ? ` · ${media.sourceApp.split("!")[0].split(".").pop()}` : ""}` : "Works automatically"}
        about="Spotify, YouTube in your browser, or any app that shows in Windows' media controls."
      />

      <Card
        logo={
          <Tile bg="linear-gradient(150deg,#9aa4b8,#4b5568)">
            <Phone size={18} />
          </Tile>
        }
        name="Phone"
        state={battery ? "on" : "off"}
        status={battery ? `Connected · ${battery.name} ${battery.level}%` : "Not paired"}
        about="Your phone's battery and wallpaper, through Windows Phone Link."
        actions={
          <button className="ghost" onClick={() => call.launch(PHONE_LINK)}>
            Open Phone Link
          </button>
        }
      />

      <Card
        logo={<FundTile size={36} />}
        name="RennSquare"
        state={portfolio?.configured ? (portfolio.error ? "warn" : "on") : "off"}
        status={portfolio?.configured ? (portfolio.error ? portfolio.error : `Connected · ${portfolio.funds.length} funds`) : "Not connected"}
        about="Your portfolio's live.json."
      >
        <div className="ca-add">
          <input value={draft.portfolioPath} spellCheck={false} placeholder="…\data\dashboard\live.json" onChange={(e) => set("portfolioPath", e.target.value)} />
        </div>
      </Card>
    </>
  );
}
