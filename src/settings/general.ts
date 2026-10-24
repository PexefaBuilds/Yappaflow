import { isMac } from "../os";
import { invoke } from "@tauri-apps/api/core";
import { ACCENTS, LANGUAGES, applyAppearance, btn, ctx, h, page, row, save, section, select, seg, sw, InsertMode } from "./common";

export function general() {
  const { settings: s, options: o } = ctx;
  const lang = LANGUAGES;
  const models = h("div");
  const restore = sw(s.restore_clipboard, (v) => save({ restore_clipboard: v }), s.insert_mode !== "paste", "g-restore");

  const renderModel = () => {
    const p = o.cleanup_providers.find((x) => x.id === ctx.settings.cleanup_provider);
    models.replaceChildren();
    if (p && p.models.length) {
      models.append(row("Cleanup model", null, select(p.models.map((m) => ({ value: m, label: m })), ctx.settings.cleanup_model,
        (v) => save({ cleanup_model: v }), "g-cmodel")));
    }
  };
  renderModel();

  const accentName = h("span");
  const swatches = h("div", { className: "swatches", role: "radiogroup", ariaLabel: "Accent colour" });
  const drawAccents = () => {
    const cur = ACCENTS.find((a) => a.id === ctx.settings.accent) ?? ACCENTS[0];
    accentName.textContent = `${cur.name}: used for the pill, buttons and highlights.`;
    swatches.replaceChildren(...ACCENTS.map((a) => h("button", {
      type: "button", className: "swatch", style: `background:${a.color}`, role: "radio", ariaLabel: a.name, title: a.name,
      ariaChecked: String(a.id === cur.id),
      onclick: async () => { await save({ accent: a.id }); applyAppearance(); drawAccents(); },
    })));
  };
  drawAccents();

  // The toggle shows the real Run entry; set_autostart also saves the choice so a reinstall can't undo it.
  const autostart = sw(false, async (v) => {
    try { await invoke("set_autostart", { on: v }); ctx.settings.start_with_windows = v; } catch (e) { autostart.checked = !v; ctx.toast(`Couldn't change ${isMac ? "open at login" : "start with Windows"}: ${e}`, true); }
  }, true, "g-auto");
  invoke<boolean>("get_autostart").then((on) => { autostart.checked = on; autostart.disabled = false; }).catch(() => {});

  return page("General", "How Yappaflow behaves day to day.",
    section("Dictation",
      row("AI cleanup", "Removes um and uh, applies your corrections and adds punctuation. Choose Off to paste the raw transcript.",
        select(o.cleanup_providers.map((p) => ({ value: p.id, label: p.label, disabled: !p.available })), s.cleanup_provider, async (v) => {
          const p = o.cleanup_providers.find((x) => x.id === v)!;
          await save({ cleanup_provider: v, cleanup_model: p.models[0] ?? ctx.settings.cleanup_model });
          renderModel();
        }, "g-clean")),
      models,
      row("Typing style", "How much punctuation and capitalisation cleanup adds.",
        select([{ value: "full", label: "Full punctuation and capitals" }, { value: "light", label: "Light: commas and periods only" }, { value: "none", label: "None: lowercase, no punctuation" }],
          s.typing_style || "full", (v) => save({ typing_style: v }), "g-typing", 300), "g-typing"),
      row("Language", "Auto-detect handles switching between English and Arabic mid-sentence.",
        select(lang.map(([value, label]) => ({ value, label })), s.language, (v) => save({ language: v }), "g-lang"), "g-lang"),
      row("Insert text by", "Typing is slower. Use it only for apps that block paste.",
        seg("Insert text by", [{ value: "paste", label: "Pasting" }, { value: "type", label: "Typing" }], s.insert_mode, async (v) => {
          await save({ insert_mode: v as InsertMode });
          restore.disabled = v !== "paste";
        })),
      row("Put my clipboard back after pasting", null, restore, "g-restore")),
    section("Sounds",
      row("Start and stop sounds", "A soft tick, so you know it's listening without looking. Placeholder: the sounds arrive in a later update.",
        sw(s.sounds_enabled, (v) => save({ sounds_enabled: v }), false, "g-snd"), "g-snd")),
    section("Appearance & system",
      row("Accent", accentName, swatches),
      row("Theme", null, seg("Theme", [{ value: "system", label: "Match system" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }],
        s.theme || "system", async (v) => { await save({ theme: v }); applyAppearance(); })),
      row(isMac ? "Open at login" : "Start with Windows", isMac ? "Yappaflow waits quietly in the menu bar, ready when you are." : "Yappaflow waits quietly in the tray, ready when you are.", autostart, "g-auto")),
  );
}
