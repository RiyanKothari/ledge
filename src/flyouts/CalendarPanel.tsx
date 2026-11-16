import { useMemo, useState } from "react";
import { ChevronLeft, ChevronRight, ClaudeTile, Close } from "../components/glyphs";
import { call, fmtTime, useChannel, useConfig, useNow, type AiUsage, type CalEvent, type CalendarState, type PlanLimits } from "../ipc";
import { KIND_COLOR } from "./CenterPanel";

const LEGEND: [string, string][] = [
  ["renewal", "Renewals"],
  ["payout", "Payouts"],
  ["event", "Events"],
  ["allday", "All day"],
  ["due", "Due dates"],
  ["limit", "Limit resets"],
];

const NEW_KINDS: [string, string][] = [
  ["event", "Event"],
  ["allday", "All day"],
  ["renewal", "Renewal"],
  ["payout", "Payout"],
  ["due", "Due date"],
];

const dayKey = (t: number) => {
  const d = new Date(t);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
};
const sameDay = (a: Date, b: Date) => dayKey(a.getTime()) === dayKey(b.getTime());

/** Small square icon for an event, coloured by kind. */
function KindIcon({ kind, size = 26 }: { kind: string; size?: number }) {
  if (kind === "limit") return <ClaudeTile size={size} />;
  const c = KIND_COLOR[kind] ?? KIND_COLOR.event;
  const glyph: Record<string, React.ReactNode> = {
    event: <path d="M7 3v3M17 3v3M4 9h16M5 5h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z" />,
    allday: <path d="M4 12h16M12 4v16" />,
    renewal: <path d="M20 11a8 8 0 0 0-14.3-4.9L4 8M4 4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16M20 20v-4h-4" />,
    payout: <path d="M12 3v18M17 7.5c0-1.9-2.2-3-5-3s-5 1.1-5 3 2.2 2.6 5 3.2 5 1.4 5 3.3-2.2 3.4-5 3.4-5-1.3-5-3.2" />,
    due: <path d="M12 7v6M12 17h.01M10.3 3.9 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />,
  };
  return (
    <span className="kind-icon" style={{ width: size, height: size, background: c }}>
      <svg width={size * 0.6} height={size * 0.6} viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        {glyph[kind] ?? glyph.event}
      </svg>
    </span>
  );
}

/** Claude's limit resets as calendar entries. */
function limitResets(ai: AiUsage | null, plan: PlanLimits | null): CalEvent[] {
  const out: CalEvent[] = [];
  const add = (t: number | null | undefined, title: string) => {
    if (t && t > Date.now()) out.push({ title, start: t, end: t, allDay: false, kind: "limit", localId: null });
  };
  if (plan?.enabled && plan.fiveHour) {
    add(plan.fiveHour.resetsAt, "Claude 5-hour limit resets");
    add(plan.sevenDay?.resetsAt, "Claude weekly limit resets");
  } else {
    add(ai?.claude.sessionResetsAt, "Claude 5-hour window resets");
  }
  return out;
}

export function CalendarPanel() {
  const [cfg] = useConfig();
  const cal = useChannel<CalendarState>("calendar");
  const ai = useChannel<AiUsage>("ai");
  const plan = useChannel<PlanLimits>("ai-plan");
  const now = useNow(30_000);
  const h24 = !!cfg?.clock24;
  const [sel, setSel] = useState(() => new Date());
  const [month, setMonth] = useState(() => new Date(new Date().getFullYear(), new Date().getMonth(), 1));
  const [time, setTime] = useState(() => {
    const d = new Date(Date.now() + 3_600_000);
    return `${String(d.getHours()).padStart(2, "0")}:00`;
  });
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState("event");
  const [repeat, setRepeat] = useState("");
  const [err, setErr] = useState("");

  const all = useMemo(() => {
    const list = [...(cal?.month ?? cal?.events ?? []), ...limitResets(ai, plan)];
    list.sort((a, b) => a.start - b.start);
    return list;
  }, [cal, ai, plan]);

  const byDay = useMemo(() => {
    const m = new Map<string, CalEvent[]>();
    for (const e of all) {
      const k = dayKey(e.start);
      m.set(k, [...(m.get(k) ?? []), e]);
    }
    return m;
  }, [all]);

  const offset = (month.getDay() + 6) % 7; // Monday first
  const days = new Date(month.getFullYear(), month.getMonth() + 1, 0).getDate();
  const cells = Array.from({ length: Math.ceil((offset + days) / 7) * 7 }, (_, i) => {
    const n = i - offset + 1;
    return n >= 1 && n <= days ? new Date(month.getFullYear(), month.getMonth(), n) : null;
  });
  const today = new Date(now);
  const dayList = (byDay.get(dayKey(sel.getTime())) ?? []).filter((e) => e.kind !== "limit" || e.start > now);
  const isToday = sameDay(sel, today);

  const add = async () => {
    setErr("");
    if (!title.trim()) return setErr("Give it a name");
    const [h, m] = time.split(":").map(Number);
    const allDay = kind === "allday" || kind === "renewal" || kind === "payout" || kind === "due";
    const start = new Date(sel.getFullYear(), sel.getMonth(), sel.getDate(), allDay ? 9 : h || 0, allDay ? 0 : m || 0).getTime();
    try {
      await call.eventAdd({ id: "", title: title.trim(), start, end: allDay ? start : start + 3_600_000, allDay, kind, repeat });
      setTitle("");
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <div className="bigcal">
      <div className="bc-main">
        <div className="bc-head">
          <h1>
            {month.toLocaleDateString([], { month: "long" })}, {month.getFullYear()}
          </h1>
          <div className="bc-actions">
            <button
              className="link"
              onClick={() => {
                const t = new Date();
                setSel(t);
                setMonth(new Date(t.getFullYear(), t.getMonth(), 1));
              }}
            >
              Today
            </button>
            <button className="icon-round" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))} aria-label="Previous month">
              <ChevronLeft size={15} />
            </button>
            <button className="icon-round" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))} aria-label="Next month">
              <ChevronRight size={15} />
            </button>
            <button className="icon-round" onClick={() => call.flyoutClose()} aria-label="Close">
              <Close size={13} />
            </button>
          </div>
        </div>
        <div className="bc-legend">
          {LEGEND.map(([k, label]) => (
            <span key={k}>
              <i style={{ background: KIND_COLOR[k] }} />
              {label}
            </span>
          ))}
        </div>
        <div className="bc-grid">
          {["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"].map((d, i) => (
            <span key={d} className={`bc-dow ${(today.getDay() + 6) % 7 === i ? "now" : ""}`}>
              {d}
            </span>
          ))}
          {cells.map((d, i) => {
            if (!d) return <span key={`x${i}`} />;
            const list = byDay.get(dayKey(d.getTime())) ?? [];
            const first = list[0];
            return (
              <button
                key={dayKey(d.getTime())}
                className={`bc-day ${sameDay(d, today) ? "today" : ""} ${sameDay(d, sel) ? "sel" : ""} ${d < today && !sameDay(d, today) ? "past" : ""}`}
                onClick={() => setSel(d)}
                title={list.map((e) => e.title).join("\n")}
              >
                {first && (
                  <span className="bc-top">
                    <KindIcon kind={first.kind} size={24} />
                    {list.length > 1 && <span className="bc-more">+{list.length - 1}</span>}
                  </span>
                )}
                {first && <i className="bc-dot" style={{ background: KIND_COLOR[first.kind] ?? KIND_COLOR.event }} />}
                <span className="bc-num">{d.getDate()}</span>
              </button>
            );
          })}
        </div>
      </div>

      <aside className="bc-side">
        <div className="bc-side-head">
          <b>{isToday ? "Today" : sel.toLocaleDateString([], { weekday: "long", day: "numeric", month: "short" })}</b>
          <span>
            {dayList.length} event{dayList.length === 1 ? "" : "s"}
          </span>
        </div>
        <ul className="bc-list">
          {dayList.length === 0 && <li className="empty">Nothing on this day.</li>}
          {dayList.map((e, i) => (
            <li key={`${e.start}-${i}`} className={!e.allDay && e.start <= now && e.end > now ? "live" : ""}>
              <span className="bc-time">{e.allDay ? "All day" : fmtTime(e.start, h24)}</span>
              <KindIcon kind={e.kind} />
              <span className="bc-title">
                <b>{e.title || "(no title)"}</b>
                <small>
                  {e.kind === "limit"
                    ? "Claude"
                    : e.localId
                      ? `Ledge · ${NEW_KINDS.find(([k]) => k === e.kind)?.[1] ?? "Event"}`
                      : e.allDay
                        ? "Calendar · all day"
                        : `Calendar · ${fmtTime(e.start, h24)} – ${fmtTime(e.end, h24)}`}
                </small>
              </span>
              {e.localId && (
                <button className="bc-del" aria-label="Delete" title="Delete" onClick={() => call.eventRemove(e.localId!)}>
                  <Close size={10} />
                </button>
              )}
            </li>
          ))}
        </ul>
        {err && <p className="empty warn">{err}</p>}
        <div className="bc-new">
          <div className="bc-new-row">
            <select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="Kind">
              {NEW_KINDS.map(([k, label]) => (
                <option key={k} value={k}>
                  {label}
                </option>
              ))}
            </select>
            <select value={repeat} onChange={(e) => setRepeat(e.target.value)} aria-label="Repeat">
              <option value="">Once</option>
              <option value="monthly">Monthly</option>
              <option value="yearly">Yearly</option>
            </select>
            {kind === "event" && <input type="time" value={time} onChange={(e) => setTime(e.target.value)} aria-label="Time" />}
          </div>
          <div className="bc-new-row">
            <input
              className="bc-title-in"
              value={title}
              placeholder={`New ${NEW_KINDS.find(([k]) => k === kind)?.[1].toLowerCase()} ${isToday ? "today" : sel.toLocaleDateString([], { day: "numeric", month: "short" })}`}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <button className="bc-add" onClick={add} aria-label="Add">
              +
            </button>
          </div>
        </div>
      </aside>
    </div>
  );
}
