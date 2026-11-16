import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { Close } from "./components/glyphs";
import { call } from "./ipc";

type Card = { hwnd: number; title: string; x: number; y: number; w: number; h: number; minimized: boolean };

/** Window previews above a dock app: a picture of each window; click to switch, × or middle-click to close. */
export default function Preview() {
  const [cards, setCards] = useState<Card[]>([]);
  const [icon, setIcon] = useState<string | null>(null);
  const [shots, setShots] = useState<Record<number, string>>({});
  useEffect(() => {
    const a = listen<{ cards: Card[]; icon: string | null }>("preview", (e) => {
      setCards(e.payload.cards);
      setIcon(e.payload.icon);
      // Keep pictures of windows we already have, so switching apps doesn't flash.
      setShots((s) => Object.fromEntries(e.payload.cards.filter((c) => s[c.hwnd]).map((c) => [c.hwnd, s[c.hwnd]])));
    });
    const b = listen<{ hwnd: number; img: string }>("preview-shot", (e) => setShots((s) => ({ ...s, [e.payload.hwnd]: e.payload.img })));
    const c = listen("preview-hidden", () => setCards([]));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
      c.then((f) => f());
    };
  }, []);

  const hide = () => invoke("preview_hide", { soon: false });
  const closeWin = (hwnd: number) => {
    call.close(hwnd);
    setCards((l) => {
      const rest = l.filter((x) => x.hwnd !== hwnd);
      if (!rest.length) hide();
      return rest;
    });
  };

  return (
    <div className="preview" onMouseEnter={() => invoke("preview_hover", { on: true })} onMouseLeave={() => invoke("preview_hover", { on: false })}>
      {cards.map((c) => (
        <div
          key={c.hwnd}
          className="pv-card"
          style={{ left: c.x, top: c.y, width: c.w, height: c.h }}
          onClick={() => {
            call.focus(c.hwnd);
            hide();
          }}
          onAuxClick={(e) => e.button === 1 && closeWin(c.hwnd)}
        >
          <div className="pv-title">
            {icon && <img src={icon} alt="" />}
            <span>{c.title || "Window"}</span>
            <button
              className="pv-x"
              aria-label="Close window"
              onClick={(e) => {
                e.stopPropagation();
                closeWin(c.hwnd);
              }}
            >
              <Close size={10} />
            </button>
          </div>
          <div className="pv-shot">
            {shots[c.hwnd] ? (
              <img className="pv-img" src={shots[c.hwnd]} alt="" draggable={false} />
            ) : icon ? (
              <img src={icon} alt="" className="pv-big" />
            ) : null}
            {c.minimized && <span className="pv-min">Minimized</span>}
          </div>
        </div>
      ))}
    </div>
  );
}
