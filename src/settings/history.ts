import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dictation, ago, btn, copy, ctx, h, page } from "./common";
import { costPanel } from "./cost";

export function history() {
  let rows: Dictation[] = [];
  let sig = "";
  let sel: number | null = null;
  let q = "";
  let audio: HTMLAudioElement | null = null;
  const cost = costPanel();
  const list = h("ul", { className: "hlist" });
  const detail = h("div", { className: "detail" });
  const search = h("input", { className: "input", type: "search", placeholder: "Search", ariaLabel: "Search history", dir: "auto",
    oninput: () => { q = search.value.toLowerCase(); drawList(); } });

  const text = (d: Dictation) => d.final_text ?? d.raw_text ?? "";
  const visible = () => rows.filter((d) => !q || `${text(d)} ${d.raw_text ?? ""} ${d.app_exe}`.toLowerCase().includes(q));

  function drawList() {
    const v = visible();
    list.replaceChildren(...v.map((d) => {
      const failed = d.status === "failed" || d.status === "transcribing";
      return h("li", {}, h("button", { type: "button", className: "item", ariaCurrent: d.id === sel ? "true" : "false", onclick: () => { sel = d.id; drawList(); drawDetail(); } },
        h("span", { className: `meta${failed ? " bad" : ""}` }, `${d.app_exe || "unknown app"} · ${failed ? (d.status === "failed" ? "failed" : "interrupted") : ago(d.created_at)}`),
        h("span", { className: "clip", dir: "auto" }, failed ? `Audio ${d.audio_path ? "saved" : "not kept"} · ${(d.duration_ms / 1000).toFixed(1)} s` : text(d) || "(empty)")));
    }));
    if (!v.length) list.append(h("li", { className: "help pad" }, rows.length ? "No matches." : "Nothing yet. Hold your hotkey and speak."));
  }

  function drawDetail() {
    audio?.pause();
    const d = rows.find((x) => x.id === sel);
    if (!d) { detail.replaceChildren(h("p", { className: "help pad" }, "Select a dictation to see the details.")); return; }
    const failed = d.status === "failed" || d.status === "transcribing";
    const t = d.timings;
    const steps: [string, number][] = t ? [["Prepare", t.prepare_ms], ["Transcribe", t.stt_ms], ["Cleanup", t.cleanup_ms], ["Insert", t.insert_ms]] : [];
    const total = Math.max(1, t?.total_ms ?? 1);
    const actions = h("div", { className: "actions" });
    if (text(d)) actions.append(btn("Copy", "s", (ev) => copy(text(d), ev.currentTarget as HTMLButtonElement)));
    if (d.audio_path) {
      const play = btn("Play audio", "s", async () => {
        try {
          const buf = await invoke<ArrayBuffer>("read_audio", { id: d.id });
          audio?.pause();
          audio = new Audio(URL.createObjectURL(new Blob([buf], { type: d.audio_path!.endsWith(".opus") ? "audio/ogg" : "audio/wav" })));
          await audio.play();
        } catch (e) { ctx.toast(`Couldn't play audio: ${e}`, true); }
      });
      actions.append(play, btn(failed ? "Retry" : "Retry with current model", failed ? "p" : "s", async (ev) => {
        const b = ev.currentTarget as HTMLButtonElement;
        b.disabled = true; b.textContent = "Retrying…";
        try { await invoke("retry_dictation", { id: d.id }); } catch (e) { ctx.toast(String(e), true); }
        await load(true);
      }));
    }
    detail.replaceChildren(...([
      h("div", { className: "meta" }, `${d.app_exe || "unknown app"} · ${new Date(d.created_at).toLocaleString()} · ${(d.duration_ms / 1000).toFixed(1)} s · ${d.stt_model}`),
      failed ? h("p", { className: "bad", dir: "auto" }, d.error ?? "Interrupted") : null,
      d.raw_text ? h("div", {}, h("h3", { className: "cap" }, "What you said"), h("p", { className: "tx", dir: "auto" }, d.raw_text)) : null,
      d.final_text ? h("div", {}, h("h3", { className: "cap" }, "What was typed"), h("p", { className: "tx", dir: "auto" }, d.final_text)) : null,
      t ? h("div", {}, h("h3", { className: "cap" }, `Release → text ${(t.total_ms / 1000).toFixed(2)} s`),
        ...steps.map(([n, ms]) => h("div", { className: "step" }, h("span", {}, n), h("div", { className: "bar" }, h("i", { style: `width:${Math.max(2, (ms / total) * 100)}%` })), h("span", { className: "ms" }, `${ms} ms`)))) : null,
      actions] as (Node | null)[]).filter((x): x is Node => !!x));
  }

  async function load(force = false) {
    const next = await invoke<Dictation[]>("recent_dictations", { limit: 200 });
    // Only touch the DOM when the data changed: rebuilding on focus ate the first click on Copy.
    const s = JSON.stringify(next);
    if (s === sig && !force) return;
    void cost.load();
    sig = s; rows = next;
    if (sel === null || !rows.some((r) => r.id === sel)) sel = rows[0]?.id ?? null;
    drawList(); drawDetail();
  }
  const onFocus = () => void load();
  window.addEventListener("focus", onFocus);
  const un = listen("dictation-updated", () => load());
  void load().catch((e) => ctx.toast(String(e), true));

  return Object.assign(
    page("History", "Everything you've said lately. Audio is kept for your most recent dictations.",
      cost.el,
      h("div", { className: "split" }, h("div", { className: "left" }, search, list), detail)),
    { cleanup: () => { window.removeEventListener("focus", onFocus); void un.then((f) => f()); audio?.pause(); } },
  );
}
