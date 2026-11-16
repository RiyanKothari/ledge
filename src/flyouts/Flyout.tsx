import { listen } from "@tauri-apps/api/event";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { call, useAppearance, useConfig, type FlyoutKind } from "../ipc";
import { CalendarPanel } from "./CalendarPanel";
import { CenterPanel } from "./CenterPanel";
import { ControlCenter } from "./ControlCenter";
import { ClaudePanel, Launcher, MediaPanel, PhonePanel, PortfolioPanel, SystemPanel, WhatsAppPanel } from "./Panels";

/** The single glass panel window. Rust positions it; we fill it and fit its height. */
export default function Flyout() {
  const [open, setOpen] = useState<{ kind: FlyoutKind; arg: string | null; n: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null); // inner content: its size is the panel's natural size
  const lastH = useRef(0);
  const [cfg] = useConfig();
  useAppearance(cfg?.appearance);

  useEffect(() => {
    const a = listen<{ kind: FlyoutKind; arg: string | null }>("flyout", (e) => {
      lastH.current = 0;
      setOpen((o) => ({ ...e.payload, n: (o?.n ?? 0) + 1 }));
    });
    const b = listen("flyout-closed", () => setOpen(null));
    const esc = (e: KeyboardEvent) => e.key === "Escape" && call.flyoutClose();
    window.addEventListener("keydown", esc);
    return () => {
      a.then((f) => f());
      b.then((f) => f());
      window.removeEventListener("keydown", esc);
    };
  }, []);

  // Grow or shrink the window to the content (bottom edge stays put).
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      const h = Math.min(Math.ceil(el.getBoundingClientRect().height) + 1, Math.floor(window.screen.availHeight * 0.86));
      if (Math.abs(h - lastH.current) > 2) {
        lastH.current = h;
        call.flyoutResize(h);
      }
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [open?.kind, open?.n]);

  if (!open) return null;
  const panel = {
    control: <ControlCenter arg={open.arg} />,
    center: <CenterPanel />,
    media: <MediaPanel />,
    claude: <ClaudePanel />,
    portfolio: <PortfolioPanel />,
    whatsapp: <WhatsAppPanel />,
    system: <SystemPanel />,
    launcher: <Launcher />,
    phone: <PhonePanel />,
    calendar: <CalendarPanel />,
  }[open.kind];
  return (
    <div className={`flyout k-${open.kind}`} key={open.n}>
      <div className="flyout-inner" ref={ref}>
        {panel}
      </div>
    </div>
  );
}
