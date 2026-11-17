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

/** A dark glass sphere with soft ribbons of colour drifting inside it. It
 *  swirls while listening, swells as your words come in, and settles once
 *  Ledge answers. */
function Orb({ state, hot }: { state: string; hot: boolean }) {
  return (
    <div className={`vorb ${state}${hot ? " hot" : ""}`}>
      <i className="o1" />
      <i className="o2" />
      <i className="o3" />
      <i className="o4" />
      <b />
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
