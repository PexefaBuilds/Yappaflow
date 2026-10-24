import "./toast.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Act = { label: string; kind: "retry" | "cloud_once" | "settings" | "dismiss" | "restart" | "add_term" | "skip_term" | "system_settings"; arg: string | null };
type Toast = { dot: "accent" | "warn" | "muted"; title: string; body: string; action: Act | null; secondary: Act | null };
type Row = { final_text: string | null };

const box = document.getElementById("toast")!;
const SHOW_MS = 6000;
let timer = 0;
let left = SHOW_MS;
let started = 0;
let hovering = false;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = "") {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function arm() {
  clearTimeout(timer);
  started = Date.now();
  timer = window.setTimeout(close, left);
}

function close() {
  clearTimeout(timer);
  box.classList.add("out");
  setTimeout(
    () => {
      box.hidden = true;
      void invoke("toast_hide");
    },
    matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 160,
  );
}

async function run(a: Act, btn: HTMLButtonElement) {
  switch (a.kind) {
    case "retry":
    case "cloud_once": {
      btn.disabled = true;
      btn.textContent = "Retrying…";
      clearTimeout(timer);
      try {
        const cmd = a.kind === "cloud_once" ? "retry_dictation_cloud" : "retry_dictation";
        const row = await invoke<Row>(cmd, { id: Number(a.arg) });
        const text = row.final_text?.trim();
        if (text) {
          await invoke("copy_text", { text });
          return render({ dot: "accent", title: "Transcribed · copied", body: text.slice(0, 140), action: null, secondary: null });
        }
        render({ dot: "warn", title: "Still can't transcribe it", body: "The recording is kept in History.", action: null, secondary: null });
      } catch {
        render({ dot: "warn", title: "Retry failed", body: "The recording is kept in History.", action: null, secondary: null });
      }
      return;
    }
    case "add_term":
    case "skip_term":
      try { await invoke("answer_term_suggestion", { term: a.arg ?? "", add: a.kind === "add_term" }); } catch { /* suggestion already answered */ }
      if (a.kind === "skip_term") return close();
      clearTimeout(timer);
      return render({ dot: "accent", title: "Added to your dictionary", body: "", action: null, secondary: null });
    case "settings":
      await invoke("open_settings", { section: a.arg });
      return close();
    case "system_settings": // macOS privacy page, e.g. after a permission was revoked
      try { await invoke("open_system_settings", { kind: a.arg ?? "" }); } catch { /* nothing to open */ }
      return close();
    default: // "restart" is a stub until an updater exists
      return close();
  }
}

function button(a: Act, cls: string) {
  const b = el("button", `btn ${cls}`, a.label);
  b.type = "button";
  b.addEventListener("click", () => void run(a, b));
  return b;
}

function render(t: Toast) {
  const text = el("div", "text");
  const title = el("span", "title", t.title);
  const body = el("span", "body", t.body);
  title.dir = body.dir = "auto";
  text.append(title, body);
  const head = el("div", "head");
  const dot = el("span", `dot ${t.dot}`);
  dot.setAttribute("aria-hidden", "true");
  head.append(dot, text);
  box.replaceChildren(head);
  if (t.action || t.secondary) {
    const row = el("div", "actions");
    if (t.action) row.append(button(t.action, t.dot === "accent" ? "primary" : ""));
    if (t.secondary) row.append(button(t.secondary, "quiet"));
    box.append(row);
  }
  box.classList.remove("out");
  box.hidden = false;
  void invoke("toast_fit", { height: box.offsetHeight });
  left = SHOW_MS;
  if (hovering) clearTimeout(timer);
  else arm();
}

// Hover pauses the timer; leaving restarts it with whatever was left (at least 1.5 s).
box.addEventListener("mouseenter", () => {
  hovering = true;
  clearTimeout(timer);
  left = Math.max(1500, left - (Date.now() - started));
});
box.addEventListener("mouseleave", () => {
  hovering = false;
  if (!box.hidden) arm();
});

async function applyAccent() {
  try {
    document.documentElement.dataset.accent = (await invoke<{ accent: string }>("get_settings")).accent;
  } catch {
    /* default accent */
  }
}
void listen<Toast>("toast", (e) => {
  void applyAccent();
  render(e.payload);
});
