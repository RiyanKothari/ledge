// Hand-drawn icons. Stroke icons inherit `currentColor`. Service tiles are
// plain shapes in the service's colour, not copied logos.

import { useIcon } from "../ipc";

const WHATSAPP_APP = "shell:AppsFolder\\5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App";

type P = { size?: number; className?: string };

/** Start: four brushed-silver tiles on a dark rounded tile. */
export const StartGlyph = ({ size = 30 }: P) => (
  <svg width={size} height={size} viewBox="0 0 30 30" aria-hidden className="start-mark">
    <defs>
      <linearGradient id="st-tile" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stopColor="#3a3a40" />
        <stop offset="1" stopColor="#18181c" />
      </linearGradient>
      <linearGradient id="st-pane" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset=".55" stopColor="#dcdce2" />
        <stop offset="1" stopColor="#b9b9c2" />
      </linearGradient>
    </defs>
    <rect x=".75" y=".75" width="28.5" height="28.5" rx="8" fill="url(#st-tile)" stroke="rgba(255,255,255,.16)" strokeWidth="1" />
    <rect x="7.2" y="7.2" width="7.3" height="7.3" rx="1.4" fill="url(#st-pane)" />
    <rect x="15.5" y="7.2" width="7.3" height="7.3" rx="1.4" fill="url(#st-pane)" />
    <rect x="7.2" y="15.5" width="7.3" height="7.3" rx="1.4" fill="url(#st-pane)" />
    <rect x="15.5" y="15.5" width="7.3" height="7.3" rx="1.4" fill="url(#st-pane)" />
  </svg>
);

const S = ({ size = 16, className, children, sw = 2 }: P & { children: React.ReactNode; sw?: number }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={sw}
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
    aria-hidden
  >
    {children}
  </svg>
);

export const Chevron = (p: P) => <S {...p}><path d="m6 15 6-6 6 6" /></S>;
export const ChevronRight = (p: P) => <S {...p}><path d="m9 6 6 6-6 6" /></S>;
export const ChevronLeft = (p: P) => <S {...p}><path d="m15 6-6 6 6 6" /></S>;
export const ChevronDown = (p: P) => <S {...p}><path d="m6 9 6 6 6-6" /></S>;
export const Prev = (p: P) => <S {...p}><path d="M19 20 9 12l10-8v16Z" fill="currentColor" /><path d="M5 19V5" /></S>;
export const Next = (p: P) => <S {...p}><path d="m5 4 10 8-10 8V4Z" fill="currentColor" /><path d="M19 5v14" /></S>;
export const Play = (p: P) => <S {...p}><path d="M7 4.5v15l12.5-7.5L7 4.5Z" fill="currentColor" /></S>;
export const Pause = (p: P) => (
  <S {...p}>
    <rect x="6" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none" />
    <rect x="14" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none" />
  </S>
);
export const Down = (p: P) => <S {...p}><path d="M12 5v14M5 12l7 7 7-7" /></S>;
export const Up = (p: P) => <S {...p}><path d="M12 19V5M5 12l7-7 7 7" /></S>;
export const Close = (p: P) => <S {...p}><path d="M6 6l12 12M18 6 6 18" /></S>;
export const Check = (p: P) => <S {...p}><path d="m5 12.5 4.5 4.5L19 7.5" /></S>;
export const Lock = (p: P) => <S {...p}><rect x="5" y="11" width="14" height="10" rx="2.5" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></S>;
export const Search = (p: P) => <S {...p}><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></S>;
export const Gear = (p: P) => (
  <S {...p}>
    <circle cx="12" cy="12" r="3.2" />
    <path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1Z" />
  </S>
);
export const Power = (p: P) => <S {...p}><path d="M12 3v9" /><path d="M6.4 6.4a8 8 0 1 0 11.2 0" /></S>;
export const Bell = (p: P) => <S {...p}><path d="M6 8a6 6 0 1 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a2 2 0 0 0 3.4 0" /></S>;

export const Wifi = (p: P) => (
  <S {...p}>
    <path d="M2 8.8a15 15 0 0 1 20 0" />
    <path d="M5 12.6a10 10 0 0 1 14 0" />
    <path d="M8.5 16.4a5 5 0 0 1 7 0" />
    <circle cx="12" cy="20" r=".9" fill="currentColor" />
  </S>
);

/** Wi-Fi arcs lit by signal strength (0–100). */
export function WifiLevel({ signal, size = 16 }: { signal: number; size?: number }) {
  const lit = signal >= 70 ? 3 : signal >= 45 ? 2 : signal >= 20 ? 1 : 0;
  const o = (i: number) => (lit >= i ? 1 : 0.28);
  return (
    <S size={size}>
      <path d="M2 8.8a15 15 0 0 1 20 0" opacity={o(3)} />
      <path d="M5 12.6a10 10 0 0 1 14 0" opacity={o(2)} />
      <path d="M8.5 16.4a5 5 0 0 1 7 0" opacity={o(1)} />
      <circle cx="12" cy="20" r=".9" fill="currentColor" />
    </S>
  );
}

export const Ethernet = (p: P) => (
  <S {...p}>
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 16v4h8v-4M8 8v3M12 8v3M16 8v3" />
  </S>
);
export const Offline = (p: P) => <S {...p}><path d="M2 8.8a15 15 0 0 1 20 0M5 12.6a10 10 0 0 1 14 0" opacity=".4" /><path d="m4 4 16 16" /></S>;
export const Bluetooth = (p: P) => <S {...p}><path d="m7 7 10 10-5 5V2l5 5L7 17" /></S>;
export const Plane = (p: P) => (
  <S {...p}>
    <path d="M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2Z" />
  </S>
);
export const Leaf = (p: P) => (
  <S {...p}>
    <path d="M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.5 19 2c1 2 2 4.2 2 8 0 5.5-4.8 10-10 10Z" />
    <path d="M2 21c0-3 1.9-5.4 5.1-6" />
  </S>
);
export const Moon = (p: P) => <S {...p}><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" /></S>;
export const SunIcon = (p: P) => (
  <S {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
  </S>
);
export const Person = (p: P) => (
  <S {...p}>
    <circle cx="12" cy="4.5" r="2" />
    <path d="M5 8.5 12 10l7-1.5M12 10v4.5M9 22l3-7.5 3 7.5" />
  </S>
);
export const Headphones = (p: P) => (
  <S {...p}>
    <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
    <path d="M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z" />
  </S>
);
export const Phone = (p: P) => <S {...p}><rect x="6" y="2" width="12" height="20" rx="3" /><path d="M11 18h2" /></S>;
export const Gamepad = (p: P) => (
  <S {...p}>
    <path d="M6 11h4M8 9v4M15 12h.01M18 10h.01" />
    <path d="M17.3 5H6.7a4 4 0 0 0-4 3.6L2 15a3 3 0 0 0 5.4 1.8L9 15h6l1.6 1.8A3 3 0 0 0 22 15l-.7-6.4a4 4 0 0 0-4-3.6Z" />
  </S>
);
export const Device = (p: P) => <S {...p}><rect x="3" y="5" width="18" height="12" rx="2" /><path d="M8 21h8M12 17v4" /></S>;
export const Cpu = (p: P) => (
  <S {...p}>
    <rect x="6" y="6" width="12" height="12" rx="2" />
    <path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" />
  </S>
);

export function Speaker({ level, muted, ...p }: P & { level: number; muted: boolean }) {
  return (
    <S {...p}>
      <path d="M11 5 6 9H2v6h4l5 4V5Z" fill="currentColor" />
      {muted ? (
        <path d="m22 9-6 6M16 9l6 6" />
      ) : (
        <>
          {level > 0.05 && <path d="M15.5 8.5a5 5 0 0 1 0 7" />}
          {level > 0.5 && <path d="M19 5a10 10 0 0 1 0 14" />}
        </>
      )}
    </S>
  );
}

export function BatteryGlyph({ percent, charging }: { percent: number; charging: boolean }) {
  const w = Math.max(1.5, (percent / 100) * 15);
  const color = charging ? "var(--green)" : percent <= 15 ? "var(--red)" : "currentColor";
  return (
    <svg width="22" height="16" viewBox="0 0 26 16" aria-hidden>
      <rect x="1" y="2" width="20" height="12" rx="3.2" fill="none" stroke="currentColor" strokeOpacity=".55" strokeWidth="1.6" />
      <rect x="22.5" y="5.5" width="2" height="5" rx="1" fill="currentColor" opacity=".55" />
      <rect x="3.5" y="4.5" width={w} height="7" rx="1.6" fill={color} />
      {charging && <path d="M12.5 3.5 8.5 9h3l-1 4 4-5.5h-3l1-4Z" fill="#0b0b0f" />}
    </svg>
  );
}

export const Bolt = ({ size = 12 }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
    <path d="M13 2 4 14h7l-1 8 9-12h-7l1-8Z" fill="currentColor" />
  </svg>
);

/** WMO weather code → glyph. */
export function WeatherGlyph({ code, isDay, size = 22 }: { code: number | null; isDay: boolean; size?: number }) {
  const c = code ?? 0;
  const sun = <circle cx="12" cy="12" r="4.5" fill="#fbbf24" stroke="none" />;
  const rays = (
    <g stroke="#fbbf24" strokeWidth="2" strokeLinecap="round">
      {[0, 45, 90, 135, 180, 225, 270, 315].map((a) => (
        <line key={a} x1="12" y1="2.5" x2="12" y2="4.5" transform={`rotate(${a} 12 12)`} />
      ))}
    </g>
  );
  const moon = <path d="M15.5 3.5a8 8 0 1 0 5 12.8A7 7 0 0 1 15.5 3.5Z" fill="#c7d2fe" stroke="none" />;
  const cloud = (x = 0, y = 0, fill = "#e5e7eb") => (
    <path transform={`translate(${x} ${y})`} d="M7 19h10.5a4 4 0 0 0 .4-8 5.5 5.5 0 0 0-10.6-1.4A4.6 4.6 0 0 0 7 19Z" fill={fill} stroke="none" />
  );
  let body: React.ReactNode;
  if (c === 0) body = isDay ? <>{rays}{sun}</> : moon;
  else if (c <= 2) body = <><g transform="translate(-3 -3) scale(.8)">{isDay ? <>{rays}{sun}</> : moon}</g>{cloud(2, 2)}</>;
  else if (c === 3) body = cloud(0, 0, "#cbd5e1");
  else if (c === 45 || c === 48) body = <g stroke="#cbd5e1" strokeWidth="2" strokeLinecap="round"><path d="M4 9h16M3 13h18M5 17h14" /></g>;
  else if (c >= 71 && c <= 77) body = <>{cloud(0, -3)}<g fill="#e0f2fe"><circle cx="8" cy="20" r="1.3" /><circle cx="12" cy="22" r="1.3" /><circle cx="16" cy="20" r="1.3" /></g></>;
  else if (c >= 95) body = <>{cloud(0, -3, "#94a3b8")}<path d="M12.5 15 9.5 20h3l-1 3.5 4-5.5h-3l1-3Z" fill="#fde047" /></>;
  else body = <>{cloud(0, -3, "#cbd5e1")}<g stroke="#60a5fa" strokeWidth="2" strokeLinecap="round"><path d="M8 18.5 7 21.5M12 18.5l-1 3M16 18.5l-1 3" /></g></>;
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
      {body}
    </svg>
  );
}

export const ClaudeTile = ({ size = 30 }: P) => (
  <div className="tile" style={{ width: size, height: size, background: "linear-gradient(140deg,#e8875f,#c4552f)" }}>
    <svg width={size * 0.62} height={size * 0.62} viewBox="0 0 24 24" aria-hidden>
      <g stroke="#fff4ec" strokeWidth="2.5" strokeLinecap="round">
        {[0, 30, 60, 90, 120, 150].map((a) => (
          <line key={a} x1="12" y1="3" x2="12" y2="21" transform={`rotate(${a} 12 12)`} />
        ))}
      </g>
    </svg>
  </div>
);

export const CodexTile = ({ size = 30 }: P) => (
  <div className="tile" style={{ width: size, height: size, background: "linear-gradient(140deg,#f5f5f5,#cfcfd4)" }}>
    <svg width={size * 0.6} height={size * 0.6} viewBox="0 0 24 24" fill="none" stroke="#111" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="m5 8 4 4-4 4M12 17h7" />
    </svg>
  </div>
);

/** WhatsApp Desktop's own icon when it's installed; a plain green chat tile otherwise. */
export function ChatTile({ size = 30 }: P) {
  const real = useIcon(WHATSAPP_APP);
  if (real) return <img className="tile-img" src={real} alt="" width={size} height={size} draggable={false} />;
  return (
    <div className="tile" style={{ width: size, height: size, background: "linear-gradient(150deg,#4be37a,#1fb455)" }}>
      <svg width={size * 0.64} height={size * 0.64} viewBox="0 0 24 24" aria-hidden>
        <path d="M12 3a9 9 0 0 0-7.8 13.5L3 21l4.6-1.2A9 9 0 1 0 12 3Z" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinejoin="round" />
      </svg>
    </div>
  );
}

export const FundTile = ({ size = 30 }: P) => (
  <div className="tile" style={{ width: size, height: size, background: "linear-gradient(150deg,#6d7bff,#3b46d6)" }}>
    <svg width={size * 0.6} height={size * 0.6} viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 17l6-6 4 4 8-8" />
      <path d="M15 7h6v6" />
    </svg>
  </div>
);
