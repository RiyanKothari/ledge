import { getCurrentWindow } from "@tauri-apps/api/window";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import Dock from "./App";
import Flyout from "./flyouts/Flyout";
import Preview from "./Preview";
import Settings from "./Settings";
import Toast from "./Toast";
import Voice from "./Voice";
import "./styles.css";

const label = getCurrentWindow().label;
const view = (["settings", "flyout", "toast", "preview", "voice"] as const).find((v) => v === label) ?? "bar";
document.documentElement.dataset.view = view;

const Root = { bar: Dock, flyout: Flyout, toast: Toast, settings: Settings, preview: Preview, voice: Voice }[view];
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
