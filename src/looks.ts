import type { Appearance, DockStyle } from "./ipc";

/**
 * Each dock style sets the glass (tint + how much of it the frost carries);
 * tweaking the sliders afterwards makes it "custom". Kept light on purpose:
 * the frosted blur behind the dock should always show.
 */
export const STYLES: { id: DockStyle; name: string; look: Pick<Appearance, "tint" | "opacity" | "blur">; preview: string }[] = [
  { id: "black", name: "Black", look: { tint: "#18181c", opacity: 0.4, blur: 28 }, preview: "linear-gradient(180deg,rgba(60,60,68,.9),rgba(24,24,28,.92))" },
  { id: "white", name: "White", look: { tint: "#f4f5f8", opacity: 0.45, blur: 30 }, preview: "linear-gradient(180deg,#ffffff,#e6e8ee)" },
  {
    id: "liquid",
    name: "Liquid",
    look: { tint: "#ffffff", opacity: 0.06, blur: 0 },
    preview:
      "radial-gradient(60% 30% at 20% 0%,rgba(255,255,255,.55),transparent 70%),linear-gradient(180deg,rgba(255,255,255,.22),rgba(255,255,255,.04) 45%,rgba(255,255,255,.1)),linear-gradient(120deg,#3a6ea5,#a0587a 60%,#d99a4e)",
  },
  {
    id: "ios",
    name: "iOS",
    look: { tint: "#3c4250", opacity: 0.28, blur: 30 },
    preview: "linear-gradient(180deg,rgba(255,255,255,.28),rgba(255,255,255,.08) 50%),linear-gradient(135deg,#5b6b8c,#8a7fa8 55%,#c69a8a)",
  },
  {
    id: "midnight",
    name: "Midnight",
    look: { tint: "#0d1236", opacity: 0.42, blur: 26 },
    preview:
      "radial-gradient(1.5px 1.5px at 22% 30%,#fff,transparent),radial-gradient(1px 1px at 70% 22%,#cfd6ff,transparent),radial-gradient(1px 1px at 55% 70%,#fff,transparent),linear-gradient(160deg,#1c2460,#0a0d26 60%,#2a1450)",
  },
];

export const ACCENTS = ["", "#5b5bd6", "#a855f7", "#f43f5e", "#ef4444", "#ea580c", "#16a34a", "#0d9488", "#8e8e98"];
