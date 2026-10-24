import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, h, page, seg } from "./common";
import { download, pickCsv, toCsv } from "./csv";

interface Term { id: number; term: string; sounds_like: string; source: "user" | "learned"; uses: number }
interface Suggestion { heard: string; term: string }

export function dictionary() {
  let terms: Term[] = [];
  let filter = "all";
  const list = h("div", { className: "card" }, h("p", { className: "help pad" }, "Loading…"));
  const tabs = h("div", {});
  const banner = h("div", {});

  const draw = () => {
    const mine = terms.filter((t) => t.source === "user").length;
    tabs.replaceChildren(seg("Filter", [
      { value: "all", label: `All · ${terms.length}` },
      { value: "user", label: `Added by you · ${mine}` },
      { value: "learned", label: `Learned · ${terms.length - mine}` },
    ], filter, (v) => { filter = v; draw(); }));
    const shown = terms.filter((t) => filter === "all" || t.source === filter);
    if (!shown.length) {
      list.replaceChildren(h("div", { className: "empty" }, h("p", { className: "lbl" }, "No words yet"),
        h("p", { className: "help" }, "Add names, brands or jargon the speech model gets wrong.")));
      return;
    }
    list.replaceChildren(...shown.map((t) => h("div", { className: "row term" },
      h("span", { className: "lbl", dir: "auto" }, t.term),
      h("span", { className: "help grow", dir: "auto" }, t.sounds_like ? `sounds like "${t.sounds_like}"` : ""),
      h("span", { className: "badge" }, t.source === "learned" ? "Learned" : "You"),
      h("span", { className: "meta num", title: "Times used" }, String(t.uses)),
      h("button", { type: "button", className: "x", ariaLabel: `Remove ${t.term}`, onclick: async () => {
        try { await invoke("delete_term", { id: t.id }); await load(); } catch (e) { ctx.toast(String(e), true); }
      } }, "×"))));
  };
  const load = async () => {
    try { terms = await invoke<Term[]>("list_terms"); draw(); } catch (e) { list.replaceChildren(h("p", { className: "help bad pad" }, String(e))); }
  };

  const showSuggestion = async () => {
    const s = await invoke<Suggestion | null>("get_term_suggestion").catch(() => null);
    if (!s) return banner.replaceChildren();
    const done = async (add: boolean) => {
      try { await invoke("answer_term_suggestion", { term: s.term, add }); } catch (e) { ctx.toast(String(e), true); }
      banner.replaceChildren();
      await load();
    };
    banner.replaceChildren(h("div", { className: "suggest", role: "status" },
      h("p", { dir: "auto" }, `You corrected "${s.heard}" to `, h("strong", {}, s.term), " twice. Add it?"),
      btn("Add", "p", () => done(true)), btn("Not now", "g", () => done(false))));
  };

  const term = h("input", { id: "dict-term", className: "input", type: "text", spellcheck: false, maxLength: 50, ariaLabel: "Word or name", placeholder: "Word or name, e.g. Al-Mansoori", dir: "auto" });
  const sounds = h("input", { id: "dict-sounds", className: "input", type: "text", spellcheck: false, maxLength: 80, ariaLabel: "Sounds like (optional)", placeholder: "Sounds like (optional)", dir: "auto" });
  const add = async () => {
    try {
      await invoke("add_term", { term: term.value, soundsLike: sounds.value });
      term.value = sounds.value = "";
      term.focus();
      await load();
    } catch (e) { ctx.toast(String(e), true); }
  };
  for (const i of [term, sounds]) i.addEventListener("keydown", (e) => { if (e.key === "Enter") void add(); });

  const exportCsv = () => download("yappaflow-dictionary.csv",
    toCsv([["term", "sounds_like"], ...terms.map((t) => [t.term, t.sounds_like])]));
  const importCsv = async () => {
    const rows = await pickCsv("term");
    let ok = 0;
    for (const [t, sl] of rows) {
      try { await invoke("add_term", { term: t ?? "", soundsLike: sl ?? "" }); ok++; } catch { /* skip bad rows */ }
    }
    if (rows.length) ctx.toast(`Imported ${ok} of ${rows.length} words`, ok < rows.length);
    await load();
  };

  void load();
  void showSuggestion();
  return page("Dictionary", "Names and jargon, spelled your way. Passed to the speech model as hints.",
    banner,
    h("div", { className: "actions addterm" },
      h("label", { className: "sr-only", htmlFor: "dict-term" }, "Word or name"), term,
      h("label", { className: "sr-only", htmlFor: "dict-sounds" }, "Sounds like (optional)"), sounds,
      btn("Add", "p", add)),
    h("div", { className: "actions spread" }, tabs,
      h("div", { className: "actions" }, btn("Import CSV", "s", importCsv), btn("Export CSV", "s", exportCsv))),
    list);
}
