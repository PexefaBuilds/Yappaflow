import "./tray.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Settings = { mic_device: string; stt_model: string; accent: string; [k: string]: unknown };
type Row = { app_exe: string; created_at: number; final_text: string | null };

const menu = document.getElementById("menu")!;
let view: "main" | "mics" = "main";
let handsFree = false;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = "") {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}
function btn(cls: string, text: string, onClick: () => void) {
  const b = el("button", cls, text);
  b.type = "button";
  b.setAttribute("role", "menuitem");
  b.addEventListener("click", onClick);
  return b;
}

function ago(ms: number) {
  const m = Math.round((Date.now() - ms) / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return h < 24 ? `${h} h ago` : `${Math.round(h / 24)} d ago`;
}

const LOGO =
  '<svg viewBox="0 0 64 64" aria-hidden="true"><path d="M8 32c2-24 5-24 7 0s5 19 7 0 9 0 12 0h22" fill="none" stroke="currentColor" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
const CHEVRON =
  '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6"/></svg>';

async function renderMain() {
  const [s, st, rows] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<{ paused_until: number }>("tray_state"),
    invoke<Row[]>("recent_dictations", { limit: 10 }).catch(() => [] as Row[]),
  ]);
  document.documentElement.dataset.accent = s.accent;
  const paused = st.paused_until > Date.now();
  const last = rows.find((r) => r.final_text?.trim());

  const head = el("div", "head");
  const logo = el("div", "logo");
  logo.innerHTML = LOGO;
  const info = el("div");
  const status = el("span", `status${paused ? " paused" : ""}`);
  const until = new Date(st.paused_until).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  status.append(el("span", "dot"), document.createTextNode(paused ? `Paused until ${until}` : `Ready · ${s.stt_model}`));
  info.append(el("span", "name", "Yappaflow"), status);
  head.append(logo, info);
  menu.replaceChildren(head);

  if (last?.final_text) {
    const text = last.final_text;
    const card = el("div", "last");
    const meta = el("span", "meta", `Last · ${last.app_exe || "unknown app"} · ${ago(last.created_at)}`);
    const body = el("span", "text", text);
    body.dir = "auto";
    const btns = el("div", "btns");
    const paste = el("button", "btn primary", "Paste again");
    paste.type = "button";
    paste.addEventListener("click", () => void invoke("tray_paste_again", { text }));
    const copy = el("button", "btn", "Copy");
    copy.type = "button";
    copy.addEventListener("click", async () => {
      await invoke("copy_text", { text });
      copy.textContent = "Copied";
    });
    btns.append(paste, copy);
    card.append(meta, body, btns);
    menu.append(card);
  }

  const hf = el("label", "item", "Hands-free mode");
  hf.setAttribute("role", "menuitemcheckbox");
  const sw = el("input", "sw");
  sw.type = "checkbox";
  sw.checked = handsFree;
  sw.disabled = paused;
  sw.addEventListener("change", () => void invoke("tray_hands_free", { on: sw.checked }));
  hf.append(sw);

  const mic = btn("item", "Microphone", () => {
    view = "mics";
    void render();
  });
  const val = el("span", "val");
  val.dir = "auto";
  val.append(el("span", "", s.mic_device || "System default"));
  val.insertAdjacentHTML("beforeend", CHEVRON);
  mic.append(val);

  const pause = btn("item", paused ? "Resume dictation" : "Pause for 1 hour", async () => {
    await invoke("tray_pause", { on: !paused });
    void invoke("tray_hide");
  });

  menu.append(
    hf,
    mic,
    pause,
    el("div", "sep"),
    btn("item", "History", () => void invoke("open_settings", { section: "history" })),
    btn("item", "Settings…", () => void invoke("open_settings", { section: null })),
    btn("item quiet", "Quit Yappaflow", () => void invoke("quit_app")),
  );
}

async function renderMics() {
  const [s, mics] = await Promise.all([invoke<Settings>("get_settings"), invoke<string[]>("list_mics").catch(() => [] as string[])]);
  menu.replaceChildren(
    btn("item", "‹ Microphone", () => {
      view = "main";
      void render();
    }),
    el("div", "sep"),
  );
  for (const name of ["", ...mics]) {
    const b = btn("item", "", async () => {
      const fresh = await invoke<Settings>("get_settings"); // merge into the latest, not a stale copy
      await invoke("save_settings", { settings: { ...fresh, mic_device: name } });
      view = "main";
      void render();
    });
    b.setAttribute("role", "menuitemradio");
    b.setAttribute("aria-checked", String(s.mic_device === name));
    const label = el("span", "", name || "System default");
    label.dir = "auto";
    const check = el("span", "check", s.mic_device === name ? "✓" : "");
    b.replaceChildren(label, check);
    menu.append(b);
  }
}

async function render() {
  await (view === "main" ? renderMain() : renderMics());
  // Keyboard users land on the first control.
  void invoke("tray_fit", { height: menu.offsetHeight });
  menu.querySelector<HTMLElement>("button, input")?.focus();
}

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (view === "mics") {
    view = "main";
    void render();
  } else void invoke("tray_hide");
});

void listen<string>("tray-open", (e) => {
  document.body.classList.toggle("top", e.payload === "top");
  view = "main";
  void render();
});
void listen<{ state: string; hands_free?: boolean }>("status", (e) => {
  handsFree = e.payload.state === "listening" && !!e.payload.hands_free;
});
