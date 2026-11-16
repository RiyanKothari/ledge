// Time-synced lyrics from LRCLIB (the same free, public source SongSeam uses).
// Only the song title, artist and length are sent.

import { useEffect, useState } from "react";

export type Line = { t: number; text: string };
export type Lyrics = { synced: boolean; lines: Line[] };

const cache = new Map<string, Lyrics | null>();

/** "[01:23.45] words" lines -> sorted, timed lines (ms). */
export function parseLrc(lrc: string): Line[] {
  const out: Line[] = [];
  for (const raw of lrc.split(/\r?\n/)) {
    const stamps = [...raw.matchAll(/\[(\d{1,2}):(\d{2})(?:[.:](\d{1,3}))?\]/g)];
    if (!stamps.length) continue;
    const text = raw.replace(/\[[^\]]*\]/g, "").trim();
    for (const m of stamps) {
      const frac = m[3] ? Number(m[3].padEnd(3, "0")) : 0;
      out.push({ t: Number(m[1]) * 60_000 + Number(m[2]) * 1000 + frac, text });
    }
  }
  return out.sort((a, b) => a.t - b.t);
}

const clean = (s: string) =>
  s
    .replace(/\s*[([].*?[)\]]/g, "")
    .replace(/\s*-\s*(remaster(ed)?|live|mono|stereo|radio edit).*$/i, "")
    .trim();

function pick(item: { syncedLyrics?: string | null; plainLyrics?: string | null } | null): Lyrics | null {
  if (!item) return null;
  if (item.syncedLyrics && item.syncedLyrics.length > 10) {
    const lines = parseLrc(item.syncedLyrics);
    if (lines.length) return { synced: true, lines };
  }
  if (item.plainLyrics) {
    return { synced: false, lines: item.plainLyrics.split(/\r?\n/).map((text) => ({ t: 0, text: text.trim() })) };
  }
  return null;
}

export async function fetchLyrics(title: string, artist: string, durationMs: number): Promise<Lyrics | null> {
  const t = clean(title);
  const a = (artist || "").split(",")[0].replace(/feat\..*/i, "").trim();
  const key = `${t}|${a}|${Math.round(durationMs / 1000)}`.toLowerCase();
  if (cache.has(key)) return cache.get(key)!;
  let best: Lyrics | null = null;
  try {
    const q = `track_name=${encodeURIComponent(t)}&artist_name=${encodeURIComponent(a)}`;
    const r = await fetch(`https://lrclib.net/api/get?${q}${durationMs ? `&duration=${Math.round(durationMs / 1000)}` : ""}`);
    if (r.ok) best = pick(await r.json());
    if (!best?.synced) {
      const s = await fetch(`https://lrclib.net/api/search?${q}`);
      if (s.ok) {
        const list = (await s.json()) as { duration?: number; syncedLyrics?: string; plainLyrics?: string }[];
        const secs = durationMs / 1000;
        const fits = list.filter((x) => !durationMs || !x.duration || Math.abs(x.duration - secs) <= 4);
        const pool = fits.length ? fits : list;
        const found = pick(pool.find((x) => x.syncedLyrics) ?? pool[0] ?? null);
        if (found && (!best || found.synced)) best = found;
      }
    }
  } catch {
    best = null;
  }
  cache.set(key, best);
  return best;
}

export function useLyrics(title: string | undefined, artist: string | undefined, durationMs: number | undefined) {
  const [state, setState] = useState<{ key: string; lyrics: Lyrics | null; loading: boolean }>({ key: "", lyrics: null, loading: false });
  const key = `${title}|${artist}`;
  useEffect(() => {
    if (!title) return;
    let alive = true;
    setState({ key, lyrics: null, loading: true });
    fetchLyrics(title, artist ?? "", durationMs ?? 0).then((lyrics) => alive && setState({ key, lyrics, loading: false }));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return state.key === key ? state : { key, lyrics: null, loading: !!title };
}

/** Index of the line being sung at `pos` (ms), or -1 before the first. */
export function currentLine(lines: Line[], pos: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= pos + 150) {
      ans = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return ans;
}
