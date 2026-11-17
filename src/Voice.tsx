import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { useAppearance, useConfig } from "./ipc";

type VoiceState = {
  state: "listening" | "done" | "unknown" | "error";
  heard: string;
  said: string;
  detail: string;
  icon: string;
};

/** Petals of light: [path, colour], each turning on its own around the centre. */
const PETALS: [string, string][] = [
  ["M52 48 C30 20 4 28 7 52 C10 76 38 72 52 48Z", "#ff3d7f"],
  ["M48 52 C66 76 96 70 93 47 C90 24 62 30 48 52Z", "#2fd8ff"],
  ["M50 50 C34 30 48 6 72 10 C92 14 88 42 50 50Z", "#33e6b5"],
  ["M50 50 C76 50 90 78 68 90 C48 100 40 70 50 50Z", "#a46bff"],
  ["M50 50 C40 64 12 64 15 82 C18 96 54 90 50 50Z", "#4d7dff"],
  ["M8 62 C30 40 70 34 94 42 C70 48 32 52 8 62Z", "#6ff5d2"],
]

/** A dark glass sphere holding translucent petals of colour around a bright
 *  core. The petals turn while listening, swell as your words come in, and
 *  settle once Ledge answers. */
function Orb({ state, hot }: { state: string; hot: boolean }) {
  return (
    <div className={`vorb ${state}${hot ? " hot" : ""}`}>
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <defs>
          <radialGradient id="vsph" cx="50%" cy="52%" r="50%">
            <stop offset="0" stopColor="#14213d" />
            <stop offset="0.62" stopColor="#0a1023" />
            <stop offset="1" stopColor="#0c2236" />
          </radialGradient>
          <linearGradient id="vrim" x1="85%" y1="10%" x2="10%" y2="90%">
            <stop offset="0" stopColor="#2ee6c8" />
            <stop offset="0.5" stopColor="#7a5cff" />
            <stop offset="1" stopColor="#ff3d7f" />
          </linearGradient>
          {PETALS.map(([, c], i) => (
            <radialGradient key={i} id={`vp${i}`} gradientUnits="userSpaceOnUse" cx="50" cy="50" r="42">
              <stop offset="0" stopColor="#ffffff" stopOpacity="0.95" />
              <stop offset="0.3" stopColor={c} stopOpacity="0.8" />
              <stop offset="1" stopColor={c} stopOpacity="0.38" />
            </radialGradient>
          ))}
          <radialGradient id="vcore" cx="50%" cy="50%" r="50%">
            <stop offset="0" stopColor="#fff" />
            <stop offset="0.35" stopColor="#fff" stopOpacity="0.85" />
            <stop offset="1" stopColor="#dbe8ff" stopOpacity="0" />
          </radialGradient>
          <linearGradient id="vgloss" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#fff" stopOpacity="0.3" />
            <stop offset="1" stopColor="#fff" stopOpacity="0" />
          </linearGradient>
          <filter id="vsoft" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur stdDeviation="1.8" />
          </filter>
          <filter id="vrimblur" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur stdDeviation="3.2" />
          </filter>
          <clipPath id="vclip">
            <circle cx="50" cy="50" r="46" />
          </clipPath>
        </defs>
        <circle cx="50" cy="50" r="46" fill="url(#vsph)" />
        <g clipPath="url(#vclip)">
          <circle cx="50" cy="50" r="44" fill="none" stroke="url(#vrim)" strokeWidth="10" opacity="0.9" filter="url(#vrimblur)" />
          {PETALS.map(([d], i) => (
            <g key={i} className={`pt p${i + 1}`}>
              <path d={d} fill={`url(#vp${i})`} fillOpacity="0.62" />
            </g>
          ))}
          <g className="core">
            <circle cx="50" cy="50" r="17" fill="url(#vcore)" />
            <ellipse cx="50" cy="50" rx="15" ry="1.3" fill="#fff" opacity="0.7" transform="rotate(-28 50 50)" filter="url(#vsoft)" />
            <ellipse cx="50" cy="50" rx="11" ry="1.1" fill="#fff" opacity="0.55" transform="rotate(38 50 50)" filter="url(#vsoft)" />
          </g>
          <ellipse cx="40" cy="20" rx="24" ry="11" fill="url(#vgloss)" />
        </g>
        <circle cx="50" cy="50" r="45.6" fill="none" stroke="rgba(255,255,255,0.3)" strokeWidth="0.8" />
      </svg>
    </div>
  );
}

/** The voice card above the dock: the orb, what Ledge heard, then its answer. */
export default function Voice() {
  const [cfg] = useConfig();
  useAppearance(cfg?.appearance);
  const [v, setV] = useState<VoiceState | null>(null);
  const [hot, setHot] = useState(false);
  const cool = useRef<number>(0);
  useEffect(() => {
    const off = listen<VoiceState>("voice", (e) => {
      setV((prev) => {
        // New words while listening: the orb swells for a moment.
        if (e.payload.state === "listening" && prev?.state === "listening" && prev.said !== e.payload.said) {
          setHot(true);
          window.clearTimeout(cool.current);
          cool.current = window.setTimeout(() => setHot(false), 450);
        }
        return e.payload;
      });
    });
    return () => {
      off.then((f) => f());
    };
  }, []);
  if (!v) return null;
  const listening = v.state === "listening";
  const top = listening ? "Ledge is listening" : v.heard ? `“${v.heard}”` : "Ledge";
  return (
    <div className={`vcard ${v.state}`}>
      <Orb state={v.state} hot={hot} />
      <div className="vtext" key={`${v.state}:${v.said}`}>
        <small>{top}</small>
        <b>{v.said}</b>
        {v.detail && <span>{v.detail}</span>}
      </div>
    </div>
  );
}
