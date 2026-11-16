import { Menu } from "@tauri-apps/api/menu";
import { invoke } from "@tauri-apps/api/core";
import { useMemo, useRef } from "react";
import { call, useIcon, type Config, type Pin, type WinInfo } from "../ipc";
import { Sortable } from "./Sortable";

type Group = {
  id: string;
  name: string;
  pin?: Pin;
  windows: WinInfo[];
  icon: string;
  iconFallback?: string;
  launchPath: string;
  /** For pinning a running app. */
  exe: string;
  aumid: string;
};

const lc = (s?: string | null) => (s ?? "").toLowerCase();
const exeName = (p: string) => {
  const base = p.split("\\").pop() ?? p;
  const stem = base.replace(/\.exe$/i, "");
  return stem.charAt(0).toUpperCase() + stem.slice(1);
};

/** Explicit app IDs win when both sides have one; otherwise match on the executable. */
function belongs(pin: Pin, w: WinInfo): boolean {
  if (pin.aumid && w.aumid) return lc(pin.aumid) === lc(w.aumid);
  return !!pin.target && lc(pin.target) === lc(w.exe);
}

export function buildGroups(pins: Pin[], wins: WinInfo[]): Group[] {
  const used = new Set<number>();
  const pinned: Group[] = pins.map((p) => {
    const ws = wins.filter((w) => !used.has(w.hwnd) && belongs(p, w));
    ws.forEach((w) => used.add(w.hwnd));
    return {
      id: `pin:${p.path}`,
      name: p.name,
      pin: p,
      windows: ws,
      icon: p.path,
      iconFallback: p.target ?? undefined,
      launchPath: p.path,
      exe: p.target ?? "",
      aumid: p.aumid ?? "",
    };
  });
  const running = new Map<string, Group>();
  for (const w of wins) {
    if (used.has(w.hwnd)) continue;
    let g = running.get(w.key);
    if (!g) {
      const appsItem = w.aumid ? `shell:AppsFolder\\${w.aumid}` : "";
      g = {
        id: `run:${w.key}`,
        name: exeName(w.exe),
        windows: [],
        icon: appsItem || w.exe,
        iconFallback: w.exe,
        launchPath: appsItem || w.exe,
        exe: w.exe,
        aumid: w.aumid,
      };
      running.set(w.key, g);
    }
    g.windows.push(w);
  }
  return [...pinned, ...running.values()];
}

/** Shared across buttons: once a preview is up, moving to another app switches at once. */
let previewUp = false;

function AppButton({ g, cfg, saveCfg }: { g: Group; cfg: Config; saveCfg: (c: Config) => Promise<void> }) {
  const src = useIcon(g.icon, g.iconFallback);
  const ref = useRef<HTMLButtonElement>(null);
  const timer = useRef(0);
  const showPreview = () => {
    const r = ref.current?.getBoundingClientRect();
    if (!r || !g.windows.length) return;
    previewUp = true;
    invoke("preview_show", { windows: g.windows.map((w) => [w.hwnd, w.title]), anchorX: r.left + r.width / 2, icon: src });
  };
  const onEnter = () => {
    clearTimeout(timer.current);
    if (!g.windows.length) return;
    if (previewUp) showPreview();
    else timer.current = window.setTimeout(showPreview, 380);
  };
  const onLeave = () => {
    clearTimeout(timer.current);
    if (previewUp) {
      invoke("preview_hide", { soon: true });
      // If nothing else picks it up, the next hover starts with the delay again.
      window.setTimeout(() => (previewUp = false), 400);
    }
  };
  const hideNow = () => {
    clearTimeout(timer.current);
    previewUp = false;
    invoke("preview_hide", { soon: false });
  };
  const running = g.windows.length > 0;
  const active = g.windows.some((w) => w.active && !w.minimized);
  const title = running ? g.windows.map((w) => w.title).join("\n") : g.name;

  const onClick = () => {
    hideNow();
    if (running) call.click(g.windows.map((w) => w.hwnd));
    else call.launch(g.launchPath);
  };

  const onMenu = async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    hideNow();
    const pins = cfg.pins ?? [];
    const items: Parameters<typeof Menu.new>[0] = { items: [] };
    const list = items.items!;
    for (const w of g.windows.slice(0, 10)) {
      list.push({ id: `f${w.hwnd}`, text: w.title.length > 60 ? `${w.title.slice(0, 57)}…` : w.title, action: () => call.focus(w.hwnd) });
    }
    if (g.windows.length) list.push({ item: "Separator" });
    list.push({ id: "new", text: running ? "New window" : `Open ${g.name}`, action: () => call.launch(g.launchPath) });
    if (g.pin) {
      list.push({
        id: "unpin",
        text: "Unpin from dock",
        action: () => saveCfg({ ...cfg, pins: pins.filter((p) => p.path !== g.pin!.path) }),
      });
    } else {
      list.push({
        id: "pin",
        text: "Pin to dock",
        action: () =>
          saveCfg({
            ...cfg,
            pins: [...pins, { name: g.name, path: g.launchPath, aumid: g.aumid || null, target: g.exe || null }],
          }),
      });
    }
    if (g.windows.length) {
      list.push({ item: "Separator" });
      list.push({
        id: "close",
        text: g.windows.length > 1 ? `Close all ${g.windows.length} windows` : "Close window",
        action: () => g.windows.forEach((w) => call.close(w.hwnd)),
      });
    }
    const menu = await Menu.new(items);
    await menu.popup();
  };

  return (
    <button
      ref={ref}
      className={`app ${running ? "running" : ""} ${active ? "active" : ""}`}
      title={running ? undefined : title}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onPointerDown={() => clearTimeout(timer.current)}
      onClick={onClick}
      onAuxClick={(e) => e.button === 1 && call.launch(g.launchPath)}
      onContextMenu={onMenu}
    >
      {src ? <img src={src} alt="" draggable={false} /> : <span className="app-fallback">{g.name.charAt(0)}</span>}
      <span className="rundots" aria-hidden>
        {running && <i />}
        {g.windows.length > 1 && <i />}
      </span>
    </button>
  );
}

export function Apps({ windows, cfg, saveCfg }: { windows: WinInfo[]; cfg: Config; saveCfg: (c: Config) => Promise<void> }) {
  const groups = useMemo(() => buildGroups(cfg.pins ?? [], windows), [cfg.pins, windows]);
  const pinned = groups.filter((g) => g.pin);
  const byId = new Map(pinned.map((g) => [g.id, g]));
  const running = groups.filter((g) => !g.pin);
  return (
    <div className="apps">
      {/* Pinned apps: drag to reorder. */}
      <Sortable
        ids={pinned.map((g) => g.id)}
        onCommit={(order) => {
          const pins = order.map((id) => byId.get(id)?.pin).filter((p): p is Pin => !!p);
          saveCfg({ ...cfg, pins });
        }}
        render={(id) => {
          const g = byId.get(id);
          return g ? <AppButton g={g} cfg={cfg} saveCfg={saveCfg} /> : null;
        }}
      />
      {running.length > 0 && pinned.length > 0 && <span className="apps-sep" />}
      {running.map((g) => (
        <AppButton key={g.id} g={g} cfg={cfg} saveCfg={saveCfg} />
      ))}
    </div>
  );
}
