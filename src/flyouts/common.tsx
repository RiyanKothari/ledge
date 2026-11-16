import { useRef } from "react";

/** A thick glass slider (brightness, volume, seek). `value` is 0–100. */
export function Slider({
  value,
  onChange,
  onCommit,
  icon,
  label,
  right,
  accent,
  disabled,
}: {
  value: number;
  onChange: (v: number) => void;
  onCommit?: (v: number) => void;
  icon?: React.ReactNode;
  label?: React.ReactNode;
  right?: React.ReactNode;
  accent?: string;
  disabled?: boolean;
}) {
  const track = useRef<HTMLDivElement>(null);
  const at = (x: number) => {
    const r = track.current!.getBoundingClientRect();
    return Math.max(0, Math.min(100, ((x - r.left) / r.width) * 100));
  };
  return (
    <div className={`slider ${disabled ? "disabled" : ""}`}>
      {(icon || label || right) && (
        <div className="slider-head">
          {icon}
          <span className="slider-label">{label}</span>
          <span className="slider-right">{right ?? `${Math.round(value)}%`}</span>
        </div>
      )}
      <div
        ref={track}
        className="slider-track"
        onPointerDown={(e) => {
          if (disabled) return;
          (e.target as Element).setPointerCapture(e.pointerId);
          onChange(at(e.clientX));
        }}
        onPointerMove={(e) => {
          if (!disabled && e.buttons & 1) onChange(at(e.clientX));
        }}
        onPointerUp={(e) => !disabled && onCommit?.(at(e.clientX))}
        onWheel={(e) => !disabled && onChange(Math.max(0, Math.min(100, value + (e.deltaY < 0 ? 2 : -2))))}
      >
        <div className="slider-fill" style={{ width: `${value}%`, ...(accent ? { background: accent } : {}) }} />
      </div>
    </div>
  );
}

export function Section({ title, action, children }: { title: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="fsec">
      <div className="fsec-head">
        <span>{title}</span>
        {action}
      </div>
      {children}
    </section>
  );
}

export function Spinner() {
  return <span className="spinner" aria-label="Loading" />;
}
