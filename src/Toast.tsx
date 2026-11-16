import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { ChatTile, Close } from "./components/glyphs";
import { call, useConfig, useIcon, type Notif } from "./ipc";

const CHARGING_MS = 2600;

/** macOS-style banner: one at a time, top-right, glass, never steals focus. */
export default function Toast() {
  const [queue, setQueue] = useState<Notif[]>([]);
  const [hover, setHover] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const cur = queue[0];
  const timer = useRef<number>(0);
  const [cfg] = useConfig();
  const showMs = Math.min(30, Math.max(1, cfg?.bannerSeconds ?? 4)) * 1000;

  useEffect(() => {
    const off = listen<Notif>("toast", (e) => setQueue((q) => (q.some((x) => x.id === e.payload.id) ? q : [...q, e.payload].slice(-6))));
    return () => {
      off.then((f) => f());
    };
  }, []);

  useEffect(() => {
    // A banner that closed under the pointer never got its mouseleave: start fresh.
    setHover(false);
    if (!cur) {
      call.toast(false);
      return;
    }
    setLeaving(false);
    call.toast(true);
  }, [cur?.id]);

  const next = () => {
    setLeaving(true);
    window.setTimeout(() => {
      setQueue((q) => q.slice(1));
      setLeaving(false);
    }, 220);
  };

  useEffect(() => {
    if (!cur || hover) return;
    timer.current = window.setTimeout(next, cur.kind === "charging" ? CHARGING_MS : showMs);
    return () => clearTimeout(timer.current);
  }, [cur?.id, hover, showMs]);

  if (!cur) return null;
  if (cur.kind === "charging") {
    const level = cur.level ?? 0;
    return (
      <div className={`banner charging ${leaving ? "leaving" : ""}`} key={cur.id} onClick={next}>
        <div className="chg-bolt">
          <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden>
            <path d="M13.5 2 5 13.5h6L9.5 22 19 9.5h-6.2L13.5 2Z" fill="currentColor" />
          </svg>
        </div>
        <div className="banner-body">
          <b>Charging</b>
          <div className="chg-bar">
            <i style={{ width: `${Math.max(4, Math.min(100, level))}%` }} />
          </div>
        </div>
        <span className="chg-pct">{level}%</span>
      </div>
    );
  }
  return (
    <div
      className={`banner ${leaving ? "leaving" : ""}`}
      key={cur.id}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onClick={() => {
        call.openNotif(cur.aumid, cur.whatsapp);
        next();
      }}
    >
      <BannerIcon n={cur} />
      <div className="banner-body">
        <div className="banner-meta">
          <span>{cur.app}</span>
          <span>{queue.length > 1 ? `+${queue.length - 1} more` : "now"}</span>
        </div>
        <b>{cur.title}</b>
        {cur.body && <p>{cur.body}</p>}
      </div>
      <button
        className="banner-x"
        aria-label="Dismiss"
        onClick={(e) => {
          e.stopPropagation();
          next();
        }}
      >
        <Close size={10} />
      </button>
    </div>
  );
}

function BannerIcon({ n }: { n: Notif }) {
  const app = useIcon(n.whatsapp || n.aumid.includes("YourPhone") ? null : `shell:AppsFolder\\${n.aumid}`);
  return (
    <div className="banner-icon">
      {n.image ? <img src={n.image} alt="" /> : n.whatsapp ? <ChatTile size={38} /> : app ? <img src={app} alt="" /> : <span className="app-fallback">{n.app.charAt(0)}</span>}
    </div>
  );
}
