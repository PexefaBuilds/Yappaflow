import { isMac } from "./os";
import "./pill.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { tick } from "./sounds";

type Outcome = { kind: "pasted" | "typed" } | { kind: "left_on_clipboard"; reason: string };
type Status =
  | { state: "idle" | "processing" | "polishing" | "empty" | "cancelled" }
  | { state: "paused"; until_ms: number }
  | { state: "listening"; hands_free: boolean; command?: boolean }
  | { state: "rewriting"; instruction: string }
  | { state: "done"; text: string; total_ms: number; outcome: Outcome; cleanup_failed?: boolean; cleanup_skipped?: boolean }
  | { state: "error"; message: string };

const BARS = 9;
const pill = document.getElementById("pill")!;
const bubble = document.getElementById("bubble")!;

/** PillExtras #09: command mode's sparkle badge (violet, coral under the Violet accent). */
function spark() {
  const s = el("span", "spark");
  s.innerHTML =
    '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.8 4.6L18.5 9l-4.7 1.4L12 15l-1.8-4.6L5.5 9l4.7-1.4z"/></svg>';
  return s;
}
let levels: number[] = new Array(BARS).fill(0);
let timer = 0;
let hideTimer = 0;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = "") {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function bars() {
  const box = el("div", "bars");
  box.setAttribute("aria-hidden", "true");
  for (let i = 0; i < BARS; i++) box.append(el("span", "bar"));
  return box;
}

function paintBars() {
  pill.querySelectorAll<HTMLElement>(".bar").forEach((b, i) => {
    b.style.height = `${(6 + 20 * levels[i]) * 0.72}px`; // 0.72 = --u in pill.css
  });
}

// Damped spring sampled into a CSS linear() easing: overshoots, then settles ("bloop").
function spring(damping: number, cycles: number): string {
  const pts: string[] = [];
  for (let i = 0; i <= 40; i++) {
    const t = i / 40;
    pts.push((1 - Math.exp(-damping * t) * Math.cos(2 * Math.PI * cycles * t)).toFixed(4));
  }
  pts[40] = "1";
  return `linear(${pts.join(", ")})`;
}
const GROW = spring(6, 1.1); // small overshoot, then settles
const SHRINK = spring(5, 0.8); // slower, softer settle

const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Swap the pill's content and morph its width (and a little squash) from the old shape to the new. */
function show(cls: string, ...kids: Node[]) {
  clearInterval(timer);
  const wasHidden = pill.hidden;
  const from = wasHidden ? 0 : pill.offsetWidth;
  pill.getAnimations({ subtree: true }).forEach((a) => a.id === "morph" && a.cancel());
  pill.className = `pill ${cls}`.trim();
  pill.replaceChildren(...kids);
  pill.hidden = false;
  if (calm) return;
  const to = pill.offsetWidth;
  const start = wasHidden ? Math.min(to, 30) : from;
  if (Math.abs(to - start) < 2 && !wasHidden) return;
  const grow = to >= start;
  const dur = grow ? 750 : 1100;
  const easing = grow ? GROW : SHRINK;
  // Only the background (::before) morphs, with a whisper of squash; the text never moves or scales.
  const base = "translateX(-50%)";
  const w = pill.animate(
    [
      { width: `${start}px`, transform: `${base} ${grow ? "scale(0.98, 1.03)" : "scale(1.02, 0.97)"}` },
      { width: `${to}px`, transform: `${base} scale(1, 1)` },
    ],
    { duration: dur, easing, pseudoElement: "::before" },
  );
  w.id = "morph";
  // Growing: wait until the shape mostly covers the text, so it never shows outside the pill.
  Array.from(pill.children).forEach((k) =>
    (k as HTMLElement).animate([{ opacity: 0 }, { opacity: 1 }], { duration: 220, delay: grow ? 160 : 90, easing: "ease-out", fill: "backwards" }),
  );
}

function mmss(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// Rust hides the window after these; hide the element too, so the next recording
// grows from a dot instead of morphing out of the stale "Pasted" shape.
const HIDE_AFTER: Record<string, number> = { done: 1200, empty: 1400, error: 5000 };
const FAILED_HIDE_AFTER = 3000; // pill.rs keeps the window up as long

function render(s: Status) {
  clearTimeout(hideTimer);
  const hideAfter = s.state === "done" && (s.cleanup_failed || s.cleanup_skipped) ? FAILED_HIDE_AFTER : HIDE_AFTER[s.state];
  if (hideAfter) hideTimer = window.setTimeout(() => (pill.hidden = true), hideAfter);
  // CommandMode.dc.html: the spoken instruction above the pill while the edit runs.
  bubble.hidden = s.state !== "rewriting";
  if (s.state === "rewriting") bubble.textContent = `“${s.instruction.trim()}”`;
  switch (s.state) {
    case "listening": {
      levels = new Array(BARS).fill(0);
      const cmd = !!s.command;
      if (!s.hands_free) {
        if (cmd) show("command has-spark", spark(), el("span", "label strong", "Command"), bars());
        else show("", el("span", "dot"), bars());
        break;
      }
      const t0 = Date.now();
      const clock = el("span", "timer", "0:00");
      const stop = el("button", "stop");
      stop.type = "button";
      stop.setAttribute("aria-label", "Stop and paste");
      stop.append(el("span"));
      stop.addEventListener("click", () => invoke("pill_stop"));
      if (cmd) show("has-stop command", spark(), clock, bars(), stop);
      else show("has-stop", clock, bars(), stop);
      timer = window.setInterval(() => (clock.textContent = mmss(Date.now() - t0)), 500);
      break;
    }
    case "processing": {
      const track = el("div", "track");
      track.setAttribute("aria-hidden", "true");
      track.append(el("div", "sheen"));
      show("", track, el("span", "label", "Transcribing"));
      break;
    }
    case "polishing": {
      // How long cleanup is taking, and a way out: paste the raw transcript now.
      const track = el("div", "track");
      track.setAttribute("aria-hidden", "true");
      track.append(el("div", "sheen"));
      const t0 = Date.now();
      const clock = el("span", "meta", "0.0 s");
      const skip = el("button", "skip", "Paste now");
      skip.type = "button";
      skip.setAttribute("aria-label", "Paste now without cleanup");
      skip.addEventListener("click", () => {
        skip.disabled = true;
        void invoke("pill_skip_cleanup");
      });
      show("has-skip", track, el("span", "label", "Polishing"), clock, skip);
      timer = window.setInterval(() => (clock.textContent = `${((Date.now() - t0) / 1000).toFixed(1)} s`), 100);
      break;
    }
    case "rewriting": {
      const track = el("div", "track");
      track.setAttribute("aria-hidden", "true");
      track.append(el("div", "sheen"));
      show("command", track, el("span", "label", "Rewriting"));
      break;
    }
    case "paused": {
      const until = new Date(s.until_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      const resume = el("button", "resume");
      resume.type = "button";
      resume.setAttribute("aria-label", `Paused until ${until}. Resume`);
      resume.innerHTML =
        '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>';
      resume.append(el("span", "", `Paused until ${until}`));
      resume.addEventListener("click", () => invoke("tray_pause", { on: false }));
      show("paused", resume);
      break;
    }
    case "done": {
      const tick = el("span", "tick");
      tick.innerHTML =
        '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
      if (s.outcome.kind === "left_on_clipboard") {
        show("has-tick fade", tick, el("span", "label strong", isMac ? "Copied · press ⌘V" : "Copied, press Ctrl+V"));
      } else if (s.cleanup_failed || s.cleanup_skipped) {
        // The raw transcript went in; say so, so a missing cleanup isn't a mystery.
        const word = s.outcome.kind === "typed" ? "Typed" : "Pasted";
        const why = s.cleanup_failed ? "cleanup failed" : "cleanup skipped";
        show("fade", el("span", s.cleanup_failed ? "dot warn" : "dot muted"), el("span", "label strong", word), el("span", "meta", why));
      } else {
        const word = s.outcome.kind === "typed" ? "Typed" : "Pasted";
        show("has-tick fade", tick, el("span", "label strong", word), el("span", "meta", `${(s.total_ms / 1000).toFixed(2)} s`));
      }
      break;
    }
    case "empty":
      show("fade", el("span", "dot muted"), el("span", "label", "Didn't hear anything"));
      break;
    case "error":
      show("", el("span", "dot warn"), el("span", "label trunc", s.message.split("\n")[0].slice(0, 60)));
      break;
    default:
      pill.hidden = true;
  }
}

let soundsOn = true;
let wasListening = false;

async function applyAccent() {
  try {
    const st = await invoke<{ accent: string; sounds_enabled?: boolean }>("get_settings");
    document.documentElement.dataset.accent = st.accent;
    soundsOn = st.sounds_enabled !== false;
  } catch {
    /* default accent */
  }
}

async function init() {
  // Measure with the real fonts: a late font swap would resize the pill after it settled.
  await Promise.all(
    ["300", "400", "500"].map((w) => document.fonts.load(`${w} 12px Unbounded`, "Transcribing Pasted 0123456789")),
  ).catch(() => {});
  await document.fonts.ready;
  await applyAccent();
  await listen<Status>("status", (e) => {
    const st = e.payload.state;
    if (st === "listening") void applyAccent();
    // Start tick once per recording (hands-free re-emits "listening"); stop tick when it ends.
    if (st === "listening" && !wasListening && soundsOn) tick("start");
    if ((st === "done" || st === "cancelled") && wasListening && soundsOn) tick("stop");
    wasListening = st === "listening" || (wasListening && (st === "processing" || st === "rewriting"));
    render(e.payload);
  });
  await listen<number>("level", (e) => {
    levels.shift();
    levels.push(Math.min(1, e.payload * 3));
    paintBars();
  });
}
void init();
