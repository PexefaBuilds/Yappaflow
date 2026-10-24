import { PASTE } from "./os";
import "./styles.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Options, Settings, applyAppearance, ctx } from "./settings/common";
import { apps } from "./settings/apps";
import { general } from "./settings/general";
import { history } from "./settings/history";
import { hotkeys } from "./settings/hotkeys";
import { mic } from "./settings/mic";
import { models } from "./settings/models";
import { rules } from "./settings/rules";
import { dictionary } from "./settings/dictionary";
import { snippets } from "./settings/snippets";
import { about, advanced, soon } from "./settings/misc";

type Outcome = { kind: "pasted" } | { kind: "typed" } | { kind: "left_on_clipboard"; reason: string };
type Status =
  | { state: "idle" } | { state: "listening"; hands_free: boolean; command?: boolean } | { state: "processing" } | { state: "polishing" }
  | { state: "rewriting"; instruction: string }
  | { state: "paused"; until_ms: number }
  | { state: "done"; text: string; total_ms: number; outcome: Outcome }
  | { state: "empty" } | { state: "cancelled" } | { state: "error"; message: string };

type Page = HTMLElement & { cleanup?: () => void };
const PAGES: { id: string; label: string; icon: string; build: () => Page }[] = [
  { id: "general", label: "General", icon: "M4 7h9M17 7h3M14 5v4M4 17h3M11 17h9M8 15v4", build: general },
  { id: "hotkeys", label: "Hotkeys", icon: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8", build: hotkeys },
  { id: "mic", label: "Microphone", icon: "M12 4a3 3 0 0 1 3 3v4a3 3 0 0 1-6 0V7a3 3 0 0 1 3-3zM6 11a6 6 0 0 0 12 0M12 17v3", build: mic },
  { id: "models", label: "Models", icon: "M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4", build: models },
  { id: "apps", label: "App styles", icon: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z", build: apps },
  { id: "rules", label: "Writing rules", icon: "M5 4h14v16H5zM9 9h6M9 13h6M9 17h3", build: rules },
  { id: "dictionary", label: "Dictionary", icon: "M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h11", build: dictionary },
  { id: "snippets", label: "Snippets", icon: "M8 7l-5 5 5 5M16 7l5 5-5 5M13 5l-2 14", build: snippets },
  { id: "history", label: "History", icon: "M12 7v5l3 2M3 12a9 9 0 1 0 3-6.7M3 4v4h4", build: history },
  { id: "advanced", label: "Advanced", icon: "M14 6a4 4 0 0 0-5 5l-6 6 2 2 6-6a4 4 0 0 0 5-5l-3 3-2-2z", build: advanced },
  { id: "about", label: "About", icon: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01", build: about },
];

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
let current: Page | null = null;
let resetTimer: number | undefined;

async function reloadSettings() {
  try { ctx.settings = await invoke<Settings>("get_settings"); applyAppearance(); } catch { /* keep cached */ }
}

function show(id: string) {
  const def = PAGES.find((p) => p.id === id) ?? PAGES[0];
  current?.cleanup?.();
  current = def.build();
  $("main").replaceChildren(current);
  $("main").scrollTop = 0;
  document.querySelectorAll<HTMLButtonElement>("#nav button").forEach((b) =>
    b.setAttribute("aria-current", b.dataset.id === def.id ? "page" : "false"));
  try { localStorage.setItem("page", def.id); } catch { /* storage unavailable */ }
}

function buildNav() {
  const NS = "http://www.w3.org/2000/svg";
  $("nav").replaceChildren(...PAGES.map((p) => {
    const b = document.createElement("button");
    b.type = "button";
    b.dataset.id = p.id;
    const svg = document.createElementNS(NS, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    const path = document.createElementNS(NS, "path");
    path.setAttribute("d", p.icon);
    svg.append(path);
    b.append(svg, p.label);
    b.addEventListener("click", () => show(p.id));
    return b;
  }));
}

let toastTimer: number | undefined;
ctx.toast = (m, bad = false) => {
  const t = $("toast");
  t.textContent = m;
  t.className = `toast${bad ? " bad" : ""}`;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (t.hidden = true), 6000);
};

// Sidebar status card: dot colour + one line, same states as the old chip.
function showStatus(s: Status) {
  const dot = $("status-dot");
  const title = $("status-title");
  const text = $("status-text");
  dot.className = "dot";
  dot.style.transform = "";
  text.title = "";
  text.dir = "auto";
  clearTimeout(resetTimer);
  const ready = () => { dot.className = "dot"; title.textContent = "Ready"; text.textContent = `Hold ${ctx.options?.hotkey ?? "Ctrl + Win"}`; };
  const later = (ms: number) => (resetTimer = window.setTimeout(ready, ms));
  switch (s.state) {
    case "idle": ready(); break;
    case "listening":
      title.textContent = s.command ? "Command" : "Listening";
      text.textContent = s.hands_free ? "Hands-free · press again to finish" : s.command ? "Say the change, then release" : "Release to paste";
      break;
    case "rewriting": dot.classList.add("busy"); title.textContent = "Rewriting…"; text.textContent = s.instruction; text.title = s.instruction; break;
    case "processing": dot.classList.add("busy"); title.textContent = "Transcribing…"; text.textContent = ""; break;
    case "polishing": dot.classList.add("busy"); title.textContent = "Polishing…"; text.textContent = ""; break;
    case "paused": dot.classList.add("muted"); title.textContent = "Paused"; text.textContent = `Until ${new Date(s.until_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`; break;
    case "done":
      if (s.outcome.kind === "left_on_clipboard") {
        dot.classList.add("warn"); title.textContent = "Copied"; text.textContent = `Press ${PASTE} (${s.outcome.reason})`; later(6000);
      } else {
        title.textContent = s.outcome.kind === "typed" ? "Typed" : "Pasted"; text.textContent = `${(s.total_ms / 1000).toFixed(2)} s`; later(2500);
      }
      break;
    case "empty": title.textContent = "Didn't hear anything"; text.textContent = ""; later(1800); break;
    case "cancelled": title.textContent = "Cancelled"; text.textContent = ""; later(1200); break;
    case "error": dot.classList.add("warn"); title.textContent = "Problem"; text.textContent = s.message; text.title = s.message; later(8000); break;
  }
}

async function init() {
  buildNav();
  [ctx.options, ctx.settings] = await Promise.all([invoke<Options>("get_options"), invoke<Settings>("get_settings")]);
  applyAppearance();
  let start = "general";
  try { start = localStorage.getItem("page") ?? start; } catch { /* ignore */ }
  show(start);
  showStatus({ state: "idle" });
  await listen<string>("open-settings", async (e) => {
    await reloadSettings();
    const id = e.payload === "microphone" ? "mic" : e.payload;
    show(PAGES.some((p) => p.id === id) ? id : "general");
  });
  window.addEventListener("focus", async () => {
    const before = JSON.stringify(ctx.settings);
    await reloadSettings();
    if (JSON.stringify(ctx.settings) !== before) show(document.querySelector<HTMLElement>("#nav [aria-current=page]")?.dataset.id ?? "general");
  });
  await listen<Status>("status", (e) => showStatus(e.payload));
  await listen<number>("level", (e) => {
    $("status-dot").style.transform = `scale(${1 + Math.min(1, e.payload * 3) * 1.4})`;
  });
}

init().catch((e) => {
  $("main").textContent = `Couldn't start the settings app: ${e}`;
});
