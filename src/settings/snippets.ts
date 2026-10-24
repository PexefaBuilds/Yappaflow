import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, h, page } from "./common";
import { download, pickCsv, toCsv } from "./csv";

interface Snippet { id: number; trigger: string; expansion: string; uses: number }

export function snippets() {
  let all: Snippet[] = [];
  let sel: number | null = null; // null = a new, unsaved snippet
  const list = h("ul", { className: "hlist" });
  const trig = h("input", { id: "sn-t", className: "input", type: "text", spellcheck: false, maxLength: 60, dir: "auto", placeholder: "e.g. sign off" });
  const exp = h("textarea", { id: "sn-e", className: "input area", rows: 8, dir: "auto", maxLength: 4000, placeholder: "The text to type, exactly as written" });
  const del = btn("Delete", "g", async () => {
    if (sel === null) return fill(null);
    try { await invoke("delete_snippet", { id: sel }); sel = null; await load(); } catch (e) { ctx.toast(String(e), true); }
  });
  del.classList.add("danger");

  const fill = (s: Snippet | null) => {
    sel = s?.id ?? null;
    trig.value = s?.trigger ?? "";
    exp.value = s?.expansion ?? "";
    drawList();
  };
  const drawList = () => {
    if (!all.length) {
      list.replaceChildren(h("li", {}, h("p", { className: "help pad" }, "No snippets yet. Write one on the right.")));
      return;
    }
    list.replaceChildren(...all.map((s) => h("li", {}, h("button", {
      type: "button", className: "item", ariaCurrent: s.id === sel ? "true" : "false", onclick: () => fill(s),
    }, h("span", { className: "lbl", dir: "auto" }, `"${s.trigger}"`),
    h("span", { className: "help clip", dir: "auto" }, s.expansion.split("\n")[0])))));
  };
  const load = async () => {
    try {
      all = await invoke<Snippet[]>("list_snippets");
      fill(all.find((s) => s.id === sel) ?? null);
    } catch (e) { list.replaceChildren(h("li", {}, h("p", { className: "help bad pad" }, String(e)))); }
  };
  const saveIt = async () => {
    try {
      sel = await invoke<number>("save_snippet", { id: sel, trigger: trig.value, expansion: exp.value });
      ctx.toast("Snippet saved");
      await load();
    } catch (e) { ctx.toast(String(e), true); }
  };

  const exportCsv = () => download("yappaflow-snippets.csv",
    toCsv([["trigger", "expansion"], ...all.map((s) => [s.trigger, s.expansion])]));
  const importCsv = async () => {
    const rows = await pickCsv("trigger");
    let ok = 0;
    for (const [t, e] of rows) {
      try { await invoke("save_snippet", { id: null, trigger: t ?? "", expansion: e ?? "" }); ok++; } catch { /* skip bad rows */ }
    }
    if (rows.length) ctx.toast(`Imported ${ok} of ${rows.length} snippets`, ok < rows.length);
    await load();
  };

  void load();
  return page("Snippets", "Say a short phrase, get the long version.",
    h("div", { className: "actions spread" }, btn("+ New snippet", "p", () => { fill(null); trig.focus(); }),
      h("div", { className: "actions" }, btn("Import CSV", "s", importCsv), btn("Export CSV", "s", exportCsv))),
    h("div", { className: "split" }, list,
      h("section", { className: "detail" },
        h("h2", { className: "cap" }, "Edit snippet"),
        h("div", { className: "field" }, h("label", { className: "lbl", htmlFor: "sn-t" }, "When I say"), trig),
        h("div", { className: "field" }, h("label", { className: "lbl", htmlFor: "sn-e" }, "Type this"), exp),
        h("p", { className: "help" }, 'Works mid-sentence too: "thanks for the call, sign off" becomes the line, then the full block.'),
        h("div", { className: "actions spread" }, del, btn("Save", "p", saveIt)))));
}
