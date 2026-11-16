import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type VoiceState = { state: "listening" | "done" | "unknown" | "error"; heard: string; said: string };

/** Fixed sparkle positions inside the orb (percent of its box). */
const SPARKS = [
  [38, 44, 0],
  [52, 40, 0.4],
  [61, 52, 0.9],
  [44, 58, 1.3],
  [56, 62, 0.2],
  [34, 54, 1.1],
  [66, 44, 0.6],
  [48, 50, 1.6],
  [42, 36, 0.8],
  [58, 34, 1.9],
];

/** A glowing orb of swirling light while Ledge listens, then what it did. */
export default function Voice() {
  const [v, setV] = useState<VoiceState | null>(null);
  useEffect(() => {
    const off = listen<VoiceState>("voice", (e) => setV(e.payload));
    return () => {
      off.then((f) => f());
    };
  }, []);
  if (!v) return null;
  return (
    <div className={`vstage ${v.state}`}>
      <div className="vorb">
        <div className="vcore" />
        <i className="vring r1" />
        <i className="vring r2" />
        <i className="vring r3" />
        <i className="vring r4" />
        {SPARKS.map(([x, y, d], i) => (
          <b key={i} className="vspark" style={{ left: `${x}%`, top: `${y}%`, animationDelay: `${d}s` }} />
        ))}
      </div>
      <div className="vcaption" key={`${v.state}-${v.said}`}>
        {v.heard && <small>“{v.heard}”</small>}
        <span>{v.said}</span>
      </div>
    </div>
  );
}
