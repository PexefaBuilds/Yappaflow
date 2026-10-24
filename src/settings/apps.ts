import { isMac } from "../os";
import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, h, page, select } from "./common";

interface AppStyle { id: number; match_kind: string; pattern: string; style: string }
const STYLES = ["casual", "formal", "neutral", "literal"].map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }));
const SAID = "um yeah so let's push the review to friday, sound good";
// Illustrative only: the real output comes from the cleanup model.
const PREVIEW: Record<string, string> = {
  casual: "yeah, let's push the review to Friday, sound good?",
  formal: "Let's push the review to Friday. Does that sound good?",
  neutral: "Let's push the review to Friday, sound good?",
  literal: "um yeah so let's push the review to friday, sound good",
};

export function apps() {
  const card = h("div", { className: "card" }, h("p", { className: "help pad" }, "Loading…"));
  const preview = h("p", { className: "tx", dir: "auto" });
  const previewTitle = h("h3", { className: "cap" }, "Preview");
  const show = (style: string) => { previewTitle.textContent = `Preview · ${style}`; preview.textContent = PREVIEW[style]; };
  show("casual");

  const load = () => invoke<AppStyle[]>("list_app_styles").then((rows) => {
    card.replaceChildren(...rows.map((r) => h("div", { className: "row" },
      h("div", { className: "app" },
        h("span", { className: "tile", ariaHidden: "true" }, r.pattern[0]?.toUpperCase() ?? "?"),
        h("div", {}, h("span", { className: "lbl", dir: "auto" }, r.pattern),
          h("div", { className: "help" }, r.match_kind === "exe" ? "App name" : r.match_kind === "bundle_id" ? "Bundle ID" : "Window title contains"))),
      select(STYLES, r.style, async (v) => {
        show(v);
        try { await invoke("set_app_style", { id: r.id, style: v }); } catch (e) { ctx.toast(String(e), true); }
      }, `as-${r.id}`, 160))));
  }).catch((e) => card.replaceChildren(h("p", { className: "help bad" }, String(e))));
  void load();

  const native = isMac ? { value: "bundle_id", label: "Bundle ID" } : { value: "exe", label: "App name" };
  const kind = select([native, { value: "title_contains", label: "Title contains" }], native.value, () => {}, "add-kind", 150);
  const pat = h("input", { id: "add-pat", className: "input", type: "text", spellcheck: false, placeholder: isMac ? "e.g. notion.id" : "e.g. notion", ariaLabel: "App name or title text", dir: "auto" });
  const sty = select(STYLES, "casual", () => {}, "add-style", 130);
  const add = async () => {
    try {
      await invoke("add_app_style", { matchKind: kind.value, pattern: pat.value, style: sty.value });
      pat.value = "";
      await load();
    } catch (e) { ctx.toast(String(e), true); }
  };
  pat.addEventListener("keydown", (e) => { if (e.key === "Enter") void add(); });

  return page("App styles", "The same words, written to fit where they land. The first matching rule wins; new rules go last.",
    h("section", {}, h("h2", { className: "cap" }, "Apps"), card),
    h("section", {}, h("h2", { className: "cap" }, "Add app"),
      h("div", { className: "card" }, h("div", { className: "row" }, h("div", { className: "actions" }, kind, pat, sty, btn("Add", "p", add))))),
    h("section", {}, previewTitle, h("div", { className: "detail" },
      h("div", {}, h("h3", { className: "cap" }, "You said"), h("p", { className: "tx", dir: "auto" }, SAID)), preview,
      h("p", { className: "help" }, "Example only; real results come from your cleanup model."))));
}
