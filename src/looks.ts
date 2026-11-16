import type { Appearance, DockStyle } from "./ipc";

/** Each dock style sets the glass; tweaking a slider afterwards makes it "custom". */
export const STYLES: { id: DockStyle; name: string; look: Omit<Appearance, "style" | "accent">; preview: string }[] = [
  { id: "black", name: "Black", look: { tint: "#0e0e12", opacity: 0.72, blur: 28 }, preview: "linear-gradient(180deg,#26262b,#141417)" },
  { id: "white", name: "White", look: { tint: "#f3f4f7", opacity: 0.58, blur: 30 }, preview: "linear-gradient(180deg,#ffffff,#e9eaef)" },
  {
    id: "liquid",
    name: "Liquid",
    look: { tint: "#ffffff", opacity: 0.05, blur: 0 },
    preview: "linear-gradient(135deg,rgba(255,255,255,.35),rgba(255,255,255,.06) 45%,rgba(255,255,255,.22)),linear-gradient(120deg,#3a6ea5,#a0587a 60%,#d99a4e)",
  },
  { id: "ios", name: "iOS", look: { tint: "#22314f", opacity: 0.5, blur: 30 }, preview: "linear-gradient(180deg,#4a6290,#2a3a5e)" },
  { id: "midnight", name: "Midnight", look: { tint: "#0b1024", opacity: 0.68, blur: 26 }, preview: "linear-gradient(180deg,#1c2238,#0b0f1e)" },
];

export const ACCENTS = ["", "#5b5bd6", "#a855f7", "#f43f5e", "#ef4444", "#ea580c", "#16a34a", "#0d9488", "#8e8e98"];
