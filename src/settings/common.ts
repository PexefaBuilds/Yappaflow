import { invoke } from "@tauri-apps/api/core";

export type InsertMode = "paste" | "type";
export interface Settings {
  stt_provider: string; stt_model: string; cleanup_provider: string; cleanup_model: string;
  language: string; mic_device: string; insert_mode: InsertMode; restore_clipboard: boolean;
  restore_delay_ms: number; openai_base_url: string; keep_recordings: number;
  silence_threshold_dbfs: number; accent: string; theme: string; sounds_enabled: boolean; typing_style: string; start_with_windows: boolean;
  auto_update: boolean; update_channel: string;
  [extra: string]: unknown; // fields owned by other modules (hotkeys, groq…) round-trip untouched
}
export interface ProviderInfo {
  id: string; label: string; models: string[]; available: boolean; note?: string | null;
  lang_note?: string | null; needs_language?: boolean; free_model?: boolean;
}
/** Language codes offered in General and for providers that need a fixed language. "" = auto. */
export const LANGUAGES = [["", "Auto-detect (mixing OK)"], ["en", "English"], ["ar", "Arabic"], ["fr", "French"], ["es", "Spanish"], ["de", "German"], ["hi", "Hindi"], ["ur", "Urdu"]];
/** "Deepgram (cloud)" → "Deepgram". */
export const providerName = (p: ProviderInfo) => p.label.replace(/\s*\(.*\)$/, "");
export interface Options { stt_providers: ProviderInfo[]; cleanup_providers: ProviderInfo[]; hotkey: string; version: string }
export interface Timings { prepare_ms: number; stt_ms: number; cleanup_ms: number; insert_ms: number; total_ms: number }
export interface Dictation {
  id: number; created_at: number; app_exe: string; window_title: string; mode: string;
  duration_ms: number; audio_path: string | null; raw_text: string | null; final_text: string | null;
  stt_model: string; timings: Timings | null; status: string; error: string | null;
}

export const ctx = { settings: {} as Settings, options: {} as Options, toast: (_m: string, _bad = false) => {} };

export const ACCENTS = [
  { id: "azure", name: "Azure", color: "#5ab0ff" }, { id: "coral", name: "Coral", color: "#ff7a59" },
  { id: "tangerine", name: "Tangerine", color: "#ff9a3d" }, { id: "sunflower", name: "Sunflower", color: "#ffc94d" },
  { id: "mint", name: "Mint", color: "#5ee3b0" }, { id: "violet", name: "Violet", color: "#9a88ff" },
  { id: "chalk", name: "Chalk", color: "#eceae3" },
];
export const PRICES: Record<string, string> = {
  "gpt-transcribe": "$0.0045/min", "gpt-4o-transcribe": "$0.006/min", "gpt-4o-mini-transcribe": "$0.003/min",
  // docs/providers.md, Oct 2026. Re-check before a release.
  "whisper-large-v3-turbo": "$0.04/hour", "whisper-large-v3": "$0.111/hour",
  "nova-3": "$0.0043/min", "scribe_v2": "$0.22/hour",
  "universal-3-5-pro": "$0.21/hour", "universal-2": "$0.15/hour",
  "voxtral-mini-2602": "$0.003/min", "voxtral-mini-transcribe-3-0": "$0.0035/min · preview",
  "gemini-3.5-flash-lite": "≈ $0.0006/min", "gemini-3.1-flash-lite": "≈ $0.001/min",
};

export function applyAppearance() {
  const r = document.documentElement;
  r.dataset.accent = ctx.settings.accent || "azure";
  if (ctx.settings.theme === "light" || ctx.settings.theme === "dark") r.dataset.theme = ctx.settings.theme;
  else delete r.dataset.theme; // "system": tokens.css follows prefers-color-scheme
}

export async function save(patch: Partial<Settings>) {
  try {
    // Merge into the backend's copy: hotkey commands and the tray also write settings.
    ctx.settings = { ...(await invoke<Settings>("get_settings")), ...patch };
    await invoke("save_settings", { settings: ctx.settings });
  } catch (e) {
    ctx.toast(`Couldn't save settings: ${e}`, true);
  }
}

type Child = Node | string | null | undefined | false;
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K, props: Record<string, unknown> = {}, ...kids: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2).toLowerCase(), v as EventListener);
    else if (k in el && k !== "list") (el as unknown as Record<string, unknown>)[k] = v;
    else el.setAttribute(k, String(v));
  }
  for (const c of kids) if (c) el.append(c);
  return el;
}

let uid = 0;
export const nextId = (p: string) => `${p}-${++uid}`;

export function page(title: string, sub: string, ...sections: Node[]) {
  return h("div", { className: "page" }, h("h1", {}, title), h("p", { className: "sub" }, sub), ...sections);
}
export function section(caption: string, ...rows: Node[]) {
  return h("section", {}, h("h2", { className: "cap" }, caption), h("div", { className: "card" }, ...rows));
}
export function row(label: string, help: string | Node | null, control: Node | null, forId?: string) {
  return h("div", { className: "row" },
    h("div", {}, forId ? h("label", { className: "lbl", htmlFor: forId }, label) : h("span", { className: "lbl" }, label),
      help ? h("div", { className: "help" }, help) : null),
    control);
}
export function sw(checked: boolean, onChange: (v: boolean) => void, disabled = false, id = nextId("sw")) {
  return h("input", { id, className: "sw", type: "checkbox", role: "switch", checked, disabled,
    onchange: (e: Event) => onChange((e.target as HTMLInputElement).checked) });
}
export function seg(label: string, opts: { value: string; label: string; disabled?: boolean }[], value: string, onChange: (v: string) => void) {
  const name = nextId("seg");
  return h("div", { className: "seg", role: "radiogroup", ariaLabel: label },
    ...opts.flatMap((o) => {
      const id = nextId("opt");
      return [
        h("input", { type: "radio", name, id, checked: o.value === value, disabled: !!o.disabled, onchange: () => onChange(o.value) }),
        h("label", { htmlFor: id }, o.label),
      ];
    }));
}
export function select(items: { value: string; label: string; disabled?: boolean }[], value: string, onChange: (v: string) => void, id = nextId("sel"), width = 220) {
  const s = h("select", { id, className: "input", style: `width:${width}px`, onchange: () => onChange(s.value) },
    ...items.map((it) => h("option", { value: it.value, disabled: !!it.disabled, selected: it.value === value }, it.label)));
  return s;
}
export function btn(label: string, kind: "p" | "g" | "s" , onClick: (ev: MouseEvent) => void, disabled = false) {
  return h("button", { type: "button", className: `btn ${kind === "p" ? "primary" : "ghost"}${kind === "s" ? " small" : ""}`, disabled, onclick: onClick }, label);
}
export const badge = (t: string) => h("span", { className: "badge" }, t);

export function ago(ms: number) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
}

export async function copy(text: string, b: HTMLButtonElement) {
  try {
    await invoke("copy_text", { text });
    b.textContent = "Copied ✓";
    setTimeout(() => (b.textContent = "Copy"), 1500);
  } catch (e) {
    ctx.toast(`Copy failed: ${e}`, true);
  }
}
