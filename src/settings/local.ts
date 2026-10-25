import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { badge, btn, ctx, h, nextId, row, save, section, select } from "./common";

export interface LocalModel {
  id: string; kind: "stt" | "cleanup"; display_name: string; size_bytes: number; notes: string; licence: string;
  state: "none" | "downloading" | "paused" | "installed"; done_bytes: number;
  est_ms: [number, number] | null; avg_ms: number | null; avg_n: number;
}
export interface LocalInfo {
  hardware: { gpu_name: string; gpu_vendor: string; vram_mb: number; ram_mb: number; cores: number };
  recommended: { stt: string; cleanup: string | null; runs_on: string } | null;
  models: LocalModel[]; dir: string; gpu_build: boolean; stt_engine: boolean;
}
export interface DownloadEvent { id: string; done: number; total: number; state: LocalModel["state"]; error: string | null }

export const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(2)} GB` : `${Math.round(b / 1e6)} MB`);

/** One listener for the whole window; the visible page registers its handler here. */
let onEvent: ((e: DownloadEvent) => void) | null = null;
let listening = false;
export function onDownload(f: (e: DownloadEvent) => void) {
  onEvent = f;
  if (!listening) {
    listening = true;
    void listen<DownloadEvent>("model_download", (e) => onEvent?.(e.payload));
  }
}

/** Progress bar + "356 of 574 MB". Returns the element and an updater. */
export function progress(done: number, total: number) {
  const fill = h("div", { className: "meter-fill" });
  const bar = h("div", { className: "meter", role: "progressbar", ariaLabel: "Download progress", ariaValueMin: "0", ariaValueMax: "100" }, fill);
  const txt = h("span", { className: "help" });
  const set = (d: number) => {
    const pct = Math.floor((d * 100) / Math.max(1, total));
    fill.style.width = `${pct}%`;
    bar.setAttribute("aria-valuenow", String(pct));
    txt.textContent = `${fmtSize(d)} of ${fmtSize(total)}`;
  };
  set(done);
  return { el: h("div", { className: "stack" }, bar, txt), set };
}

const secs = (ms: number) => (ms < 10_000 ? (ms / 1000).toFixed(1) : String(Math.round(ms / 1000)));

/** "Your average: 0.9 s per 10 s of speech (14 dictations)", or the rough range before that. */
export function speedText(m: LocalModel) {
  const per = m.kind === "stt" ? "per 10 s of speech" : "per cleanup";
  if (m.avg_ms != null) return `Your average: ${secs(m.avg_ms)} s ${per} (${m.avg_n} dictations)`;
  return m.est_ms ? `About ${secs(m.est_ms[0])}–${secs(m.est_ms[1])} s ${per} on this PC (estimate)` : "";
}

const inUse = (m: LocalModel) =>
  m.kind === "stt"
    ? ctx.settings.stt_provider === "whisper_cpp" && ctx.settings.stt_model === m.id
    : ctx.settings.cleanup_provider === "llama_cpp" && ctx.settings.cleanup_model === m.id;

export function localModels() {
  const root = h("div", { className: "col-gap" });
  const updaters = new Map<string, (d: number) => void>();
  let info: LocalInfo | null = null;

  const act = async (cmd: string, id: string) => {
    try { await invoke(cmd, { id }); } catch (e) { ctx.toast(String(e), true); }
    await draw();
  };
  const use = async (m: LocalModel) => {
    // VRAM budget check (Rust decides): too big = refuse; cleanup_cpu = switch, cleanup runs on the CPU.
    const s = ctx.settings;
    const pair = m.kind === "stt"
      ? { stt: m.id, cleanup: s.cleanup_provider === "llama_cpp" ? s.cleanup_model : "" }
      : { stt: s.stt_provider === "whisper_cpp" ? s.stt_model : "", cleanup: m.id };
    try {
      const b = await invoke<{ fit: string; message: string } | null>("local_budget", pair);
      if (b?.fit === "too_big" && m.kind === "stt") return ctx.toast(b.message, true);
      if (b && b.fit !== "ok") ctx.toast(b.message);
    } catch { /* no check possible; the sidecar still falls back */ }
    await save(m.kind === "stt" ? { stt_provider: "whisper_cpp", stt_model: m.id } : { cleanup_provider: "llama_cpp", cleanup_model: m.id });
    await draw();
  };

  const modelRow = (m: LocalModel) => {
    const rec = info?.recommended && (info.recommended.stt === m.id || info.recommended.cleanup === m.id);
    const label = h("span", { className: "lbl" }, m.display_name);
    const help = h("div", { className: "help" }, `${fmtSize(m.size_bytes)} · ${m.notes} · ${m.licence}`);
    const speed = speedText(m);
    const ctl: Node[] = [];
    if (m.state === "installed") {
      if (inUse(m)) ctl.push(h("span", { className: "badge on" }, "In use"));
      else {
        const noEngine = m.kind === "stt" && !info?.stt_engine;
        ctl.push(btn("Use", "p", () => void use(m), noEngine), btn("Delete", "g", () => void act("delete_model", m.id)));
      }
    } else if (m.state === "downloading" || m.state === "paused") {
      const p = progress(m.done_bytes, m.size_bytes);
      updaters.set(m.id, p.set);
      ctl.push(p.el,
        m.state === "downloading" ? btn("Pause", "g", () => void act("pause_download", m.id)) : btn("Resume", "p", () => void act("download_model", m.id)),
        btn("Cancel", "g", () => void act("cancel_download", m.id)));
    } else {
      ctl.push(btn("Download", rec ? "p" : "g", () => void act("download_model", m.id)));
    }
    return h("div", { className: "row" },
      h("div", {}, h("div", { className: "actions" }, label, rec ? badge("Recommended") : null), help, speed ? h("div", { className: "help" }, speed) : null),
      h("div", { className: "actions" }, ...ctl));
  };

  const hwCard = (i: LocalInfo) => {
    const hw = i.hardware;
    const gpu = hw.gpu_name ? `${hw.gpu_name} · ${(hw.vram_mb / 1024).toFixed(0)} GB graphics memory` : "No graphics card found";
    const runs = i.recommended ? `Runs on ${i.recommended.runs_on}` : "";
    const engine = i.gpu_build ? "GPU speech engine" : "CPU speech engine (this build has no GPU support)";
    // VRAM bar: models in use vs dedicated memory (estimate; free VRAM isn't measured yet).
    const used = (k: "stt" | "cleanup") => i.models.find((m) => m.kind === k && inUse(m))?.size_bytes ?? 0;
    const total = hw.vram_mb * 1024 * 1024;
    const rows: Node[] = [row(gpu, `${(hw.ram_mb / 1024).toFixed(0)} GB memory · ${hw.cores} cores · ${engine}${runs ? ` · ${runs}` : ""}`, null)];
    if (total > 0 && hw.gpu_vendor === "nvidia") {
      const [s, c] = [used("stt") * 1.1, used("cleanup") * 1.1];
      const free = Math.max(0, total - s - c - 0.9e9);
      const pct = (b: number) => `${Math.min(100, (b * 100) / total)}%`;
      rows.push(h("div", { className: "row" },
        h("div", { className: "vram grow", role: "img", ariaLabel: `Graphics memory: speech ${fmtSize(s)}, cleanup ${fmtSize(c)}, about ${fmtSize(free)} free` },
          h("span", { className: "vram-s", style: `width:${pct(s)}` }), h("span", { className: "vram-c", style: `width:${pct(c)}` })),
        h("span", { className: "help" }, `Speech ${fmtSize(s)} · Cleanup ${fmtSize(c)} · about ${fmtSize(free)} free`)));
    }
    return section("This PC", ...rows);
  };

  const serverRow = () => {
    const id = nextId("llama");
    const input = h("input", { id, className: "input grow", spellcheck: false, dir: "auto", placeholder: "Next to Yappaflow.exe", value: String(ctx.settings.llama_server_path ?? ""),
      onchange: () => void save({ llama_server_path: input.value.trim() }) });
    return row("llama-server", "Local cleanup runs through llama.cpp's llama-server. Leave empty to use the copy next to Yappaflow.exe, or paste the full path to llama-server.exe.", input, id);
  };

  const keepRow = () => {
    const id = nextId("keep");
    const opts = [
      { value: "always", label: "Always" },
      { value: "idle", label: "Unload after 15 min idle" },
      { value: "after_use", label: "Unload after each use" },
    ];
    return row("Keep models loaded", "Loaded models answer fastest but hold graphics memory and RAM. After unloading, the next dictation warms them up again (slower). With \"Unload after each use\", local cleanup often can't start in time and pastes the raw text.",
      select(opts, String(ctx.settings.keep_models_loaded ?? "always"), (v) => void save({ keep_models_loaded: v }), id), id);
  };

  async function draw() {
    try { info = await invoke<LocalInfo>("local_models"); }
    catch (e) { root.replaceChildren(h("p", { className: "help bad" }, String(e))); return; }
    updaters.clear();
    const list = (k: "stt" | "cleanup") => info!.models.filter((m) => m.kind === k).map(modelRow);
    const folder = h("button", { type: "button", className: "btn ghost small", onclick: () => invoke("open_models_folder").catch((e) => ctx.toast(String(e), true)) }, "Open folder");
    root.replaceChildren(
      hwCard(info),
      section("Speech to text · whisper.cpp", ...(info.stt_engine ? [] : [row("Not in this build", h("span", { className: "bad" }, "This copy of Yappaflow was built without the local speech engine. You can download models now; they work once the engine is included."), null)]), ...list("stt")),
      section("Cleanup · llama.cpp", ...list("cleanup"), serverRow()),
      section("Memory", keepRow()),
      h("div", { className: "actions" }, h("span", { className: "help" }, `Models live in ${info.dir}`), folder));
  }

  onDownload((e) => {
    if (!root.isConnected) return;
    const up = updaters.get(e.id);
    if (e.error) ctx.toast(e.error, true);
    if (e.state === "downloading" && up) up(e.done);
    else void draw();
  });
  void draw();
  return root;
}
