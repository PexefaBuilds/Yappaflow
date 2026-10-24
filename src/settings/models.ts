import { isMac } from "../os";
import { invoke } from "@tauri-apps/api/core";
import { localModels } from "./local";
import { LANGUAGES, PRICES, ProviderInfo, btn, ctx, h, nextId, page, providerName, row, save, section, select, seg } from "./common";

/** Providers that never take a key. */
const NO_KEY = new Set(["none", "whisper_cpp", "llama_cpp"]);
const PLACEHOLDER: Record<string, string> = { openai: "sk-…", groq: "gsk_…", custom: "Optional key for your server" };

function keyBlock(p: ProviderInfo) {
  const name = providerName(p);
  const ph = PLACEHOLDER[p.id] ?? `Paste your ${name} key`;
  const id = nextId("key");
  const input = h("input", { id, className: "input grow", type: "password", autocomplete: "off", spellcheck: false, placeholder: ph });
  const res = h("span", { role: "status" });
  const say = (t: string, ok: boolean | null) => { res.textContent = t; res.className = ok === null ? "" : ok ? "ok" : "bad"; };
  const status = async () => {
    try {
      const masked = await invoke<string>("provider_key_status", { provider: p.id });
      input.value = "";
      input.placeholder = masked ? `Saved: ${masked}` : ph;
      if (!masked && p.id !== "custom") say(`Add your ${name} key`, null);
      return !!masked;
    } catch (e) { say(String(e), false); return false; }
  };
  const test = async () => {
    say("Testing…", null);
    try { say(`✓ ${await invoke<string>("test_provider_key", { provider: p.id })}`, true); }
    catch (e) { say(String(e), false); }
  };
  const doSave = async () => {
    const v = input.value.trim();
    if (!v) return;
    try { await invoke("set_provider_key", { provider: p.id, key: v }); await status(); await test(); }
    catch (e) { say(String(e), false); }
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") void doSave(); });
  void status();
  return row(`${name} API key`, `Stored in ${isMac ? "your Mac's Keychain" : "Windows Credential Manager"}. Sent only to ${name}.`,
    h("div", { className: "stack key" }, h("div", { className: "actions" }, input, btn("Save", "p", doSave), btn("Test", "g", test)), res), id);
}

/** Base URL for the Custom provider. Plain http:// is accepted by the backend only for localhost / LAN. */
function customUrlRow() {
  const id = nextId("url");
  const warn = h("div", { className: "help bad", role: "status" });
  // The backend owns the rule (https, or http only for localhost / LAN).
  const check = async (v: string) => {
    if (!v) { warn.textContent = ""; return true; }
    try { warn.textContent = await invoke<string>("check_custom_url", { url: v }); return true; }
    catch (e) { warn.textContent = String(e); return false; }
  };
  const input = h("input", { id, className: "input grow", type: "url", spellcheck: false, placeholder: "https://my-server.example/v1", value: String(ctx.settings.custom_base_url ?? ""),
    onchange: async () => { const v = input.value.trim(); if (await check(v)) await save({ custom_base_url: v }); } });
  void check(input.value.trim());
  return row("Server URL", "Base URL of an OpenAI-compatible server, ending in /v1.", h("div", { className: "stack key" }, input, warn), id);
}

function modelControl(p: ProviderInfo, value: string, onSave: (v: string) => void, selId: string) {
  if (p.free_model) {
    const id = nextId("model");
    const list = nextId("models");
    return row("Model", p.note ?? null, h("div", { className: "stack key" },
      h("input", { id, className: "input grow", spellcheck: false, value, placeholder: "Model ID", list, onchange: (e: Event) => onSave((e.target as HTMLInputElement).value.trim()) }),
      h("datalist", { id: list }, ...p.models.map((m) => h("option", { value: m })))), id);
  }
  return row("Model", p.note ?? null, select(
    p.models.map((m) => ({ value: m, label: PRICES[m] ? `${m} · ${PRICES[m]}` : m })), value, onSave, selId, 300));
}

/** Remembered across re-renders (the page rebuilds when settings change). */
let where: "cloud" | "local" | null = null;

export function models() {
  where ??= ctx.settings.stt_provider === "whisper_cpp" || ctx.settings.cleanup_provider === "llama_cpp" ? "local" : "cloud";
  const cloud = cloudModels();
  const local = h("div");
  const body = h("div", { className: "col-gap" });
  const pg = page("Models", "",
    h("div", { className: "tabs" }, seg("Where models run", [{ value: "cloud", label: "Cloud" }, { value: "local", label: "On this PC" }], where, (v) => { where = v as "cloud" | "local"; swap(); })),
    body);
  function swap() {
    if (where === "local" && !local.firstChild) local.append(localModels());
    body.replaceChildren(where === "local" ? local : cloud);
    pg.querySelector(".sub")!.textContent = where === "local" ? "Free, private, offline. Your audio never leaves this PC." : "Who turns your voice into text, and who tidies it up.";
  }
  swap();
  return pg;
}

function cloudModels() {
  const o = ctx.options;
  const s = ctx.settings;
  const keys = h("div");
  const sttExtra = h("div");
  const clExtra = h("div");
  const sttOf = () => o.stt_providers.find((x) => x.id === ctx.settings.stt_provider) ?? o.stt_providers[0];
  const clOf = () => o.cleanup_providers.find((x) => x.id === ctx.settings.cleanup_provider);

  const drawKeys = () => {
    const seen = new Set<string>();
    const ps = [sttOf(), clOf()].filter((p): p is ProviderInfo => !!p && !NO_KEY.has(p.id) && !seen.has(p.id) && !!seen.add(p.id));
    keys.replaceChildren(...ps.map(keyBlock), ...(seen.has("custom") ? [customUrlRow()] : []));
  };
  const drawStt = () => {
    const p = sttOf();
    const rows: Node[] = [];
    if (p.lang_note) rows.push(row("Language", h("span", { className: p.needs_language ? "bad" : "" }, p.lang_note), null));
    if (p.needs_language) {
      // Auto-detect is unusable for this provider, so offer only fixed languages.
      const fixed = LANGUAGES.filter(([v]) => v);
      const cur = String(ctx.settings.language ?? "");
      const items = [...(cur ? [] : [{ value: "", label: "Choose a language", disabled: true }]), ...fixed.map(([value, label]) => ({ value, label }))];
      rows.push(row("Dictation language", "Required for this provider. Also changes General › Language.", select(items, cur, async (v) => { await save({ language: v }); drawStt(); }, "md-lang", 300), "md-lang"));
    }
    rows.push(modelControl(p, ctx.settings.stt_model, (v) => save({ stt_model: v }), "md-stt"));
    sttExtra.replaceChildren(...rows);
  };
  const drawCl = () => {
    const p = clOf();
    clExtra.replaceChildren();
    if (p && (p.models.length || p.free_model)) clExtra.append(modelControl(p, ctx.settings.cleanup_model, (v) => save({ cleanup_model: v }), "md-cl"));
  };
  drawKeys();
  drawStt();
  drawCl();

  return h("div", { className: "col-gap" },
    section("Keys", keys),
    section("Speech to text",
      row("Provider", null, select(o.stt_providers.map((p) => ({ value: p.id, label: p.label, disabled: !p.available })), s.stt_provider, async (v) => {
        const p = o.stt_providers.find((x) => x.id === v)!;
        await save({ stt_provider: v, stt_model: p.models[0] ?? "" });
        drawStt();
        drawKeys();
      }, "md-sp", 300), "md-sp"),
      sttExtra),
    section("Cleanup · removes ums, fixes corrections, adds punctuation",
      row("Provider", null, select(o.cleanup_providers.map((p) => ({ value: p.id, label: p.label, disabled: !p.available })), s.cleanup_provider, async (v) => {
        const p = o.cleanup_providers.find((x) => x.id === v)!;
        await save({ cleanup_provider: v, cleanup_model: p.models[0] ?? (p.free_model ? "" : ctx.settings.cleanup_model) });
        drawCl();
        drawKeys();
      }, "md-cp", 300), "md-cp"),
      clExtra));
}
