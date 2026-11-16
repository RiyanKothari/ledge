// "Liquid glass": the wallpaper behind the dock, refracted by a rounded pane.
//
// Near the pane's edge, light is bent: each pixel shows the wallpaper from a
// little further out, so the rim squeezes in what lies just beyond the glass
// (red bends slightly more than blue, which gives the faint colour fringe).
// It is computed once per wallpaper / dock size, not per frame.

export type Rect = { x: number; y: number; w: number; h: number };

/** Signed distance from a rounded rectangle (negative inside) and its outward normal. */
export function roundedRect(px: number, py: number, w: number, h: number, r: number) {
  const cx = w / 2;
  const cy = h / 2;
  const dx = px - cx;
  const dy = py - cy;
  const qx = Math.abs(dx) - (cx - r);
  const qy = Math.abs(dy) - (cy - r);
  const ox = Math.max(qx, 0);
  const oy = Math.max(qy, 0);
  const d = Math.hypot(ox, oy) + Math.min(Math.max(qx, qy), 0) - r;
  let nx: number;
  let ny: number;
  if (qx > 0 && qy > 0) {
    const l = Math.hypot(qx, qy) || 1;
    nx = (qx / l) * Math.sign(dx);
    ny = (qy / l) * Math.sign(dy);
  } else if (qx > qy) {
    nx = Math.sign(dx);
    ny = 0;
  } else {
    nx = 0;
    ny = Math.sign(dy);
  }
  return { d, nx, ny };
}

/**
 * Renders the refracted pane. `wall` is the wallpaper drawn at monitor size
 * (DIPs) by `drawWall(ctx)`; `dock` is the pane's rect on the monitor.
 */
export async function bakeLiquid(
  drawWall: (ctx: CanvasRenderingContext2D, offsetX: number, offsetY: number) => void,
  dock: Rect,
  opts: { radius: number; edge: number; dpr: number },
): Promise<Blob | null> {
  const { radius, edge, dpr } = opts;
  const M = Math.ceil(edge * 1.2); // margin of wallpaper beyond the pane
  const sw = Math.round((dock.w + 2 * M) * dpr);
  const sh = Math.round((dock.h + 2 * M) * dpr);
  const src = document.createElement("canvas");
  src.width = sw;
  src.height = sh;
  const sctx = src.getContext("2d", { willReadFrequently: true });
  if (!sctx) return null;
  sctx.scale(dpr, dpr);
  sctx.filter = "blur(1.4px) saturate(1.2)";
  drawWall(sctx, -(dock.x - M), -(dock.y - M));
  const sp = sctx.getImageData(0, 0, sw, sh).data;

  const ow = Math.round(dock.w * dpr);
  const oh = Math.round(dock.h * dpr);
  const out = document.createElement("canvas");
  out.width = ow;
  out.height = oh;
  const octx = out.getContext("2d");
  if (!octx) return null;
  const img = octx.createImageData(ow, oh);
  const op = img.data;
  const sample = (x: number, y: number, c: number) => {
    // x, y in DIPs relative to the pane; source has the margin around it.
    const sx = Math.min(sw - 1, Math.max(0, Math.round((x + M) * dpr)));
    const sy = Math.min(sh - 1, Math.max(0, Math.round((y + M) * dpr)));
    return sp[(sy * sw + sx) * 4 + c];
  };
  for (let j = 0; j < oh; j++) {
    const y = (j + 0.5) / dpr;
    for (let i = 0; i < ow; i++) {
      const x = (i + 0.5) / dpr;
      const { d, nx, ny } = roundedRect(x, y, dock.w, dock.h, radius);
      const inside = -d; // distance from the rim, inwards
      const o = (j * ow + i) * 4;
      let amount = 0;
      if (inside < edge) {
        const t = 1 - Math.max(inside, 0) / edge;
        amount = edge * 0.95 * t * t * t;
      }
      // A gentle magnification towards the middle, like a thick pane.
      const mx = (x - dock.w / 2) * -0.04;
      const my = (y - dock.h / 2) * -0.04;
      op[o] = sample(x + nx * amount * 1.1 + mx, y + ny * amount * 1.1 + my, 0);
      op[o + 1] = sample(x + nx * amount + mx, y + ny * amount + my, 1);
      op[o + 2] = sample(x + nx * amount * 0.9 + mx, y + ny * amount * 0.9 + my, 2);
      op[o + 3] = 255;
    }
  }
  octx.putImageData(img, 0, 0);
  return new Promise((res) => out.toBlob(res));
}
