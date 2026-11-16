import { useLayoutEffect, useRef, useState } from "react";
import { call } from "../ipc";

type Drag = {
  id: string;
  dx: number;
  from: number; // index in the full list
  rankFrom: number; // rank among visible items
  toRank: number; // where it would land among the other visible items
  width: number;
  /** full index -> rank among visible items other than the dragged one */
  rank: Map<number, number>;
};

/**
 * Horizontal drag-to-reorder. Press and drag an item more than a few pixels to
 * pick it up; the others slide out of its way; release to drop. A plain click
 * still clicks (the drag swallows only its own click). Items that render
 * nothing (a hidden widget) keep their place in the saved order.
 */
export function Sortable({
  ids,
  onCommit,
  render,
  className,
}: {
  ids: string[];
  onCommit: (ids: string[]) => void;
  render: (id: string) => React.ReactNode;
  className?: string;
}) {
  const row = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  // Keep a dropped order on screen until the saved order comes back.
  const [pending, setPending] = useState<string[] | null>(null);
  const shown = pending ?? ids;
  useLayoutEffect(() => setPending(null), [ids.join("|")]);

  const start = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0 || !row.current) return;
    const x0 = e.clientX;
    const items = [...row.current.children] as HTMLElement[];
    const rects = items.map((el) => el.getBoundingClientRect());
    const from = shown.indexOf(id);
    if (from < 0 || rects[from].width === 0) return;
    const vis = rects.map((r, i) => (r.width > 0 ? i : -1)).filter((i) => i >= 0);
    const others = vis.filter((i) => i !== from);
    const rank = new Map(others.map((i, r) => [i, r]));
    const rankFrom = vis.indexOf(from);
    const gap = vis.length > 1 ? rects[vis[1]].left - rects[vis[0]].right : 6;
    const width = rects[from].width + gap;
    let picked = false;
    let last: Drag | null = null;

    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      if (!picked) {
        if (Math.abs(dx) < 7) return;
        picked = true;
        call.hold(true);
      }
      const center = rects[from].left + rects[from].width / 2 + dx;
      const toRank = others.filter((i) => rects[i].left + rects[i].width / 2 < center).length;
      last = { id, dx, from, rankFrom, toRank, width, rank };
      setDrag(last);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (!picked) return;
      call.hold(false);
      // Swallow the click that follows the drop.
      const eat = (ce: MouseEvent) => {
        ce.stopPropagation();
        ce.preventDefault();
      };
      window.addEventListener("click", eat, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", eat, { capture: true }), 60);
      const d = last;
      setDrag(null);
      if (!d || d.toRank === d.rankFrom) return;
      const next = shown.filter((x) => x !== id);
      if (d.toRank < others.length) next.splice(next.indexOf(shown[others[d.toRank]]), 0, id);
      else next.splice(next.indexOf(shown[others[others.length - 1]]) + 1, 0, id);
      setPending(next);
      onCommit(next);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const offset = (i: number) => {
    if (!drag) return 0;
    if (i === drag.from) return drag.dx;
    const r = drag.rank.get(i);
    if (r == null) return 0;
    if (drag.rankFrom < drag.toRank && r >= drag.rankFrom && r < drag.toRank) return -drag.width;
    if (drag.rankFrom > drag.toRank && r >= drag.toRank && r < drag.rankFrom) return drag.width;
    return 0;
  };

  return (
    <div ref={row} className={`sortable ${className ?? ""} ${drag ? "dragging" : ""}`}>
      {shown.map((id, i) => (
        <div
          key={id}
          className={`sort-item ${drag?.id === id ? "lifted" : ""}`}
          style={offset(i) ? { transform: `translateX(${offset(i)}px)` } : undefined}
          onPointerDown={(e) => start(e, id)}
        >
          {render(id)}
        </div>
      ))}
    </div>
  );
}
