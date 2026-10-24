import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, h, page, save, sw } from "./common";

interface DefaultRule { id: string; text: string }
const MAX_RULES = 20;
const MAX_CHARS = 300;

const list = (k: "disabled_rules" | "custom_rules"): string[] => (Array.isArray(ctx.settings[k]) ? (ctx.settings[k] as string[]) : []);

export function rules() {
  const defaults = h("div", { className: "card" }, h("p", { className: "help pad" }, "Loading…"));
  const mine = h("div", { className: "card" });
  const msg = h("p", { className: "help note", role: "status" });
  let defs: DefaultRule[] = [];

  const defaultRow = (r: DefaultRule) =>
    h("div", { className: "row" },
      h("label", { className: "lbl", htmlFor: `rule-${r.id}`, dir: "auto" }, r.text),
      sw(!list("disabled_rules").includes(r.id), async (on) => {
        const off = new Set(list("disabled_rules"));
        if (on) off.delete(r.id); else off.add(r.id);
        await save({ disabled_rules: [...off] });
        msg.textContent = on ? "Rule on." : "Rule off. It won't be used until you switch it back on.";
      }, false, `rule-${r.id}`));

  function renderMine() {
    const items = list("custom_rules");
    const input = h("input", { id: "new-rule", className: "input grow", type: "text", maxLength: MAX_CHARS, dir: "auto", spellcheck: true,
      placeholder: "For example: Always write numbers as digits." });
    const add = async () => {
      const text = input.value.replace(/\s+/g, " ").trim();
      if (!text) return;
      if (items.length >= MAX_RULES) { msg.textContent = `You can have up to ${MAX_RULES} rules of your own.`; return; }
      await save({ custom_rules: [...items, text] });
      msg.textContent = "Rule added. Dictations now always go through AI cleanup, which adds about a second.";
      renderMine();
    };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") void add(); });
    mine.replaceChildren(
      ...(items.length ? items.map((t, i) =>
        h("div", { className: "row" }, h("span", { className: "lbl", dir: "auto" }, t),
          btn("Remove", "s", async () => { await save({ custom_rules: items.filter((_, j) => j !== i) }); renderMine(); }))) :
        [h("p", { className: "help pad" }, "No rules of your own yet.")]),
      h("div", { className: "row" }, h("label", { className: "lbl", htmlFor: "new-rule" }, "Add a rule"),
        h("div", { className: "actions", style: "flex:1" }, input, btn("Add", "p", () => void add()))));
  }

  let armed = 0;
  const reset = btn("Reset to defaults", "g", async () => {
    if (!armed) {
      reset.textContent = "Click again to confirm";
      armed = window.setTimeout(() => { armed = 0; reset.textContent = "Reset to defaults"; }, 4000);
      return;
    }
    clearTimeout(armed); armed = 0; reset.textContent = "Reset to defaults";
    await save({ disabled_rules: [], custom_rules: [] });
    msg.textContent = "Back to the default rules. Your own rules were removed.";
    paint();
    renderMine();
  });

  function paint() { defaults.replaceChildren(...defs.map(defaultRow)); }
  invoke<DefaultRule[]>("get_default_cleanup_rules").then((d) => { defs = d; paint(); })
    .catch(() => defaults.replaceChildren(h("p", { className: "help pad bad" }, "The default rules couldn't be loaded.")));
  renderMine();

  return page("Writing rules", "How Yappaflow cleans up what you say. Switch a default rule off, or add your own in plain English.",
    h("h2", { className: "cap" }, "Default rules"), defaults,
    h("h2", { className: "cap" }, "Your rules"), mine,
    h("div", { className: "row" }, h("span", { className: "help" }, "Spoken lists, dashes and fillers are handled by the defaults."), reset),
    msg);
}
