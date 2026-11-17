import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, type ReactNode } from "react";
import { useAppearance, useConfig } from "./ipc";

type VoiceState = {
  state: "listening" | "done" | "unknown" | "error";
  heard: string;
  said: string;
  detail: string;
  icon: string;
};

const svg = (d: ReactNode) => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    {d}
  </svg>
);

const ICONS: Record<string, ReactNode> = {
  mic: svg(
    <>
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
    </>,
  ),
  clock: svg(
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>,
  ),
  calendar: svg(
    <>
      <rect x="3.5" y="5" width="17" height="15" rx="3" />
      <path d="M3.5 10h17M8 3v4M16 3v4" />
    </>,
  ),
  weather: svg(
    <>
      <path d="M7 18.5h10a4 4 0 0 0 .4-8 5.5 5.5 0 0 0-10.6 1.6A3.2 3.2 0 0 0 7 18.5Z" />
    </>,
  ),
  battery: svg(
    <>
      <rect x="2.5" y="7.5" width="17" height="9" rx="2.5" />
      <path d="M21.5 10.5v3M6 10.5v3M9 10.5v3" />
    </>,
  ),
  music: svg(
    <>
      <path d="M9 18V5.5l11-2V16" />
      <circle cx="6.5" cy="18" r="2.5" />
      <circle cx="17.5" cy="16" r="2.5" />
    </>,
  ),
  timer: svg(
    <>
      <circle cx="12" cy="13.5" r="7.5" />
      <path d="M12 9.5v4l2.5 1.5M9.5 2.5h5" />
    </>,
  ),
  check: svg(<path d="m5 12.5 4.5 4.5L19 7.5" />),
  lock: svg(
    <>
      <rect x="5" y="10.5" width="14" height="10" rx="2.5" />
      <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" />
    </>,
  ),
  claude: svg(<path d="M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4" />),
  help: svg(
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M9.6 9.4a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.1-2.4 3.7M12 17h.01" />
    </>,
  ),
};

/** A quiet glass card above the dock: what Ledge heard, then its answer. */
export default function Voice() {
  const [cfg] = useConfig();
  useAppearance(cfg?.appearance);
  const [v, setV] = useState<VoiceState | null>(null);
  useEffect(() => {
    const off = listen<VoiceState>("voice", (e) => setV(e.payload));
    return () => {
      off.then((f) => f());
    };
  }, []);
  if (!v) return null;
  const listening = v.state === "listening";
  const top = listening ? "Ledge is listening" : v.heard ? `“${v.heard}”` : "Ledge";
  return (
    <div className={`vcard ${v.state}`}>
      <div className="vicon">{ICONS[v.icon] ?? ICONS.mic}</div>
      <div className="vtext" key={`${v.state}:${v.said}`}>
        <small>{top}</small>
        <b>{v.said}</b>
        {v.detail && <span>{v.detail}</span>}
      </div>
    </div>
  );
}
