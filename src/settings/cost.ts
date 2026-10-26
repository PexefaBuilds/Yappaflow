import { invoke } from "@tauri-apps/api/core";
import { isMac } from "../os";
import { btn, ctx, h, nextId, providerName } from "./common";

interface RowCost {
  created_at: number; duration_ms: number;
  stt_provider: string; stt_usd: number | null;
  cleanup_provider: string; cleanup_usd: number | null;
}
interface Estimate { prices_checked: string; version: string; rows: RowCost[] }
interface Bucket { n: number; ms: number; usd: number; unknown: number; old: number; by: Map<string, { usd: number; unknown: number }> }

const usd = (v: number) => (v > 0 && v < 0.005 ? "under $0.01" : `~$${v.toFixed(2)}`);
const name = (id: string) => {
  const p = [...(ctx.options.stt_providers ?? []), ...(ctx.options.cleanup_providers ?? [])].find((x) => x.id === id);
  return p ? providerName(p) : id || "Unknown provider";
};
const monthKey = (ms: number) => { const d = new Date(ms); return `${d.getFullYear()}-${d.getMonth()}`; };

/** "Estimated API cost this month" plus a 12-month breakdown, all from local History. */
export function costPanel() {
  const line = h("p", { className: "lbl", role: "status" }, "Estimated API cost this month: …");
  const months = h("div", {});
  const foot = h("p", { className: "help note" });
  const how = h("p", { className: "help note" });
  const oldNote = h("p", { className: "help note" });
  const real = h("p", { className: "lbl", role: "status", hidden: true });
  const spend = realSpend(real);
  const el = h("div", { className: "card cost" }, line, real,
    h("details", {}, h("summary", { className: "help" }, "How is this worked out?"), how),
    h("details", {}, h("summary", { className: "help" }, "Last 12 months"), months, oldNote, foot),
    spend.el);

  async function load() {
    const now = new Date();
    const since = new Date(now.getFullYear(), now.getMonth() - 11, 1).getTime();
    const est = await invoke<Estimate>("usage_estimate", { sinceMs: since });
    const buckets = new Map<string, Bucket>();
    const add = (b: Bucket, provider: string, v: number | null) => {
      const p = b.by.get(provider) ?? { usd: 0, unknown: 0 };
      if (v === null) { p.unknown++; } else { p.usd += v; b.usd += v; }
      b.by.set(provider, p);
    };
    for (const r of est.rows) {
      const k = monthKey(r.created_at);
      const b = buckets.get(k) ?? { n: 0, ms: 0, usd: 0, unknown: 0, old: 0, by: new Map() };
      b.n++; b.ms += r.duration_ms;
      // Pre-v5 rows have no cleanup record (provider "" and no price): counted separately.
      const old = !r.cleanup_provider && r.cleanup_usd === null;
      if (old) b.old++;
      if (r.stt_usd === null || (!old && r.cleanup_usd === null)) b.unknown++;
      add(b, r.stt_provider, r.stt_usd);
      if (r.cleanup_provider) add(b, r.cleanup_provider, r.cleanup_usd);
      buckets.set(k, b);
    }
    const unknownNote = (b: Bucket) => (b.unknown ? ` (${b.unknown} ${b.unknown === 1 ? "dictation" : "dictations"} with an unknown price not included)` : "");

    const cur = buckets.get(monthKey(now.getTime()));
    line.textContent = `Estimated API cost this month: ${usd(cur?.usd ?? 0)}${cur ? unknownNote(cur) : ""}`;
    const olds = [...buckets.values()].reduce((n, b) => n + b.old, 0);
    oldNote.textContent = olds ? `${olds} older ${olds === 1 ? "dictation has" : "dictations have"} no cleanup record, so only transcription is counted for ${olds === 1 ? "it" : "them"}.` : "";

    how.textContent = "Yappaflow is free. This is only an estimate of what your speech and cleanup providers may charge "
      + "for your dictations on this PC, worked out from your local History: audio length × each model's price per minute or hour "
      + "(Groq bills clips under 10 s as 10 s), plus an estimate of the text sent to and returned by the cleanup model, priced per million tokens. "
      + "Local models cost nothing. Not counted: free tiers, keyword add-ons, reasoning tokens, Gemini transcription output tokens, Deepgram's multilingual rate "
      + "(priced at the mono rate), failed cleanup calls the provider may still bill, the first attempt before a retry, and taxes. Models without a listed price "
      + "(Custom URL, other OpenRouter models) show as unknown. Dictations from before this version have no cleanup record, so only their "
      + "transcription is estimated. Check your provider's dashboard for the real bill. "
      + `Prices last checked ${new Date(`${est.prices_checked}T12:00`).toLocaleDateString(undefined, { dateStyle: "long" })}.`;

    const rows: Node[] = [];
    for (let i = 0; i < 12; i++) {
      const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
      const b = buckets.get(monthKey(d.getTime()));
      if (!b) continue;
      const parts = [...b.by].filter(([, p]) => p.usd > 0 || p.unknown).map(([id, p]) => `${name(id)} ${p.usd > 0 ? usd(p.usd) : ""}${p.unknown ? `${p.usd > 0 ? " + " : ""}${p.unknown} unknown` : ""}`);
      rows.push(h("tr", {},
        h("th", { scope: "row" }, d.toLocaleDateString(undefined, { month: "short", year: "numeric" })),
        h("td", {}, String(b.n)),
        h("td", {}, (b.ms / 60000).toFixed(1)),
        h("td", {}, usd(b.usd)),
        h("td", { className: "meta" }, parts.join(" · ") || "Free (local or no cleanup)")));
    }
    months.replaceChildren(rows.length
      ? h("table", { className: "costs" },
        h("thead", {}, h("tr", {}, ...["Month", "Dictations", "Audio min", "Estimate", "By provider"].map((t) => h("th", { scope: "col" }, t)))),
        h("tbody", {}, ...rows))
      : h("p", { className: "help" }, "No dictations in the last 12 months."));
    foot.textContent = `From this PC's History only (Yappaflow ${est.version}). Deleting all history resets these estimates.`;
  }

  return { el, load: () => { void spend.refresh(); return load().catch((e) => { line.textContent = `Couldn't work out the estimate: ${e}`; }); } };
}

/** Optional: OpenAI's real spend this month for the saved key's project, via an admin key. */
function realSpend(headline: HTMLElement) {
  const id = nextId("admin");
  const input = h("input", { id, className: "input grow", type: "password", autocomplete: "off", spellcheck: false, placeholder: "sk-admin-…" });
  const res = h("p", { className: "help", role: "status" });
  const refresh = async () => {
    const masked = await invoke<string>("provider_key_status", { provider: "openai_admin" }).catch(() => "");
    input.value = "";
    input.placeholder = masked ? `Saved: ${masked}` : "sk-admin-…";
    headline.hidden = !masked;
    if (!masked) { res.textContent = ""; return; }
    res.textContent = "Asking OpenAI…";
    headline.textContent = "Real OpenAI spend this month: …";
    const now = new Date();
    try {
      const r = await invoke<{ usd: number; project: string | null }>("openai_real_cost", { sinceMs: new Date(now.getFullYear(), now.getMonth(), 1).getTime() });
      headline.textContent = `Real OpenAI spend this month: $${r.usd.toFixed(2)}`;
      res.textContent = r.project ? `Real spend covers project "${r.project}": all its keys and devices.` : "Real spend covers the whole organisation: the saved OpenAI key's project wasn't found.";
    } catch (e) { res.textContent = String(e); headline.hidden = true; }
  };
  const store = async (v: string) => {
    try { await invoke("set_provider_key", { provider: "openai_admin", key: v }); await refresh(); }
    catch (e) { res.textContent = String(e); }
  };
  input.addEventListener("keydown", (e) => { if (e.key === "Enter" && input.value.trim()) void store(input.value.trim()); });
  const details = h("details", {}, h("summary", { className: "help" }, "Real OpenAI spend (optional)"),
    h("p", { className: "help note" }, "Paste an OpenAI Admin key (sk-admin-…, Organization › Admin keys; read-only is enough) to see what OpenAI actually billed this month "
      + `for the project of your saved OpenAI key. Stored in ${isMac ? "your Mac's Keychain" : "Windows Credential Manager"}, sent only to OpenAI, used only when this panel opens.`),
    h("label", { htmlFor: id, className: "lbl" }, "OpenAI admin key"),
    h("div", { className: "actions" }, input,
      btn("Save", "p", () => { if (input.value.trim()) void store(input.value.trim()); }),
      btn("Remove", "g", () => void store(""))),
    res);
  details.addEventListener("toggle", () => { if (details.open) void refresh(); });
  return { el: details, refresh };
}
