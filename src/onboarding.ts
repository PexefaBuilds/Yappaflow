import "./onboarding.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isMac } from "./os";
import type { DownloadEvent, LocalInfo } from "./settings/local";

type Provider = { id: string; label: string; available: boolean; models: string[]; lang_note?: string | null; needs_language?: boolean; free_model?: boolean };
type Opts = { stt_providers: Provider[]; cleanup_providers: Provider[]; hotkey: string };
type Settings = Record<string, unknown> & { stt_provider: string; cleanup_provider: string; mic_device: string; typing_style: string; accent: string; theme: string };

// Screen order; macOS adds the permissions step after Connect (OnbMacPerms).
const ORDER = ["welcome", "connect", ...(isMac ? ["perms"] : []), "style", "mic", "try", "done"];
const TOTAL = ORDER.length;
const num = (id: string) => ORDER.indexOf(id) + 1;
const app = document.getElementById("app")!;
let step = 0;
let st: Settings;
let opts: Opts;
let keyOk = false;
let stopMeter: (() => void) | undefined;
let stopPoll: (() => void) | undefined;
let unlisten: (() => void) | undefined;
let local: LocalInfo | undefined;
/** Step 2 has a second screen ("On this PC" → download). */
let localScreen = false;
let unlistenDl: (() => void) | undefined;
const fmt = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.round(b / 1e6)} MB`);

const h = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const kbds = () => opts.hotkey.split("+").map((k) => `<kbd class="kbd">${h(k.trim())}</kbd>`).join(" + ");
const q = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
// Re-fetch before each write so changes made elsewhere (tray, hotkeys page) aren't clobbered.
const save = async (patch: Partial<Settings>) => {
  const cur = await invoke<Settings>("get_settings");
  st = { ...cur, ...patch };
  await invoke("save_settings", { settings: st });
};
const checked = (name: string) => document.querySelector<HTMLInputElement>(`input[name="${name}"]:checked`)?.value;

const brand = `<div class="brand"><i aria-hidden="true"><svg width="16" height="16" viewBox="0 0 16 16" fill="var(--accent)"><rect x="1" y="5" width="2" height="6" rx="1"/><rect x="5" y="2" width="2" height="12" rx="1"/><rect x="9" y="4" width="2" height="8" rx="1"/><rect x="13" y="6" width="2" height="4" rx="1"/></svg></i>Yappaflow</div>`;

function foot(next: string, opt: { back?: boolean; skip?: boolean } = { back: true }) {
  return `<div class="foot">${opt.back === false ? "" : '<button class="back" id="back" type="button">Back</button>'}<div class="row">${
    opt.skip ? '<button class="back" id="skip" type="button">Skip</button>' : ""
  }<button class="cta" id="next" type="button">${next}</button></div></div>`;
}
const head = (n: number, title: string, sub: string) =>
  `<div class="col"><div class="step">Step ${n} of ${TOTAL}</div><h1>${title}</h1><p class="sub">${sub}</p></div>`;

const STYLES = [
  ["full", "Full punctuation and capitals", "Hey Sara, can we move lunch to Friday?"],
  ["light", "Light, commas and periods only", "Hey Sara, can we move lunch to Friday."],
  ["none", "None, lowercase and no punctuation", "hey sara can we move lunch to friday"],
];

const screens: Record<string, () => string> = {
  welcome: () => `<div>${brand}</div><section class="col" style="justify-content:center"><h1 class="hero">Talk.<br>It types.</h1>
    <p class="sub">${isMac ? "Hold fn" : "Hold two keys"}, say what you mean, let go. Clean text lands wherever your cursor is: Slack, Outlook, VS Code, anywhere.</p>
    <div class="row" style="align-items:center">${kbds()}<span class="help">hold · speak · release</span></div></section>
    <div class="foot"><span class="help">Takes about a minute</span><button class="cta" id="next" type="button">Get started</button></div>`,
  connect: () => `<section>${head(num("connect"), "Where should your voice go?", "You can switch any time in Settings › Models.")}
    <div class="opts">
      <label class="opt"><input type="radio" name="where" value="local"${!local?.stt_engine ? " disabled" : st.stt_provider === "whisper_cpp" ? " checked" : ""}><b>On this ${isMac ? "Mac" : "PC"}</b><span class="tag">${
        !local?.stt_engine ? "Not in this build" : h(local.hardware.gpu_name || "No graphics card found")}</span><span class="help">Private and offline. Nothing leaves your computer.</span></label>
      <label class="opt"><input type="radio" name="where" value="cloud"${st.stt_provider === "whisper_cpp" && local?.stt_engine ? "" : " checked"}><b>Cloud, with your key</b><span class="help">Pick a provider and paste its key. You pay them directly, never us.</span></label>
    </div>
    <div class="col" id="cloudbox"><div class="col" id="provs" style="gap:6px"></div>
    <p class="help" id="langnote" dir="auto"></p>
    <div class="row"><input class="field grow" id="key" type="password" autocomplete="off" spellcheck="false" aria-label="API key" placeholder="Paste your API key" dir="auto">
    <button class="small" id="test" type="button">Test key</button></div><div class="status" id="kstat" role="status"></div></div></section>${foot("Next")}`,
  perms: () => `<section>${head(num("perms"), "Three switches in System Settings", "macOS asks you to allow these yourself. We'll open each page for you and tick it off when it's done.")}
    <div class="col" id="perms" style="gap:10px">${PERMS.map(([id, name, why]) =>
      `<div class="opt list perm"><span class="col" style="gap:4px"><b>${name}</b><span class="help">${why}</span></span><span class="status" id="p-${id}" role="status"></span><button class="small" id="b-${id}" type="button" aria-label="Open settings for ${name}">Open settings</button></div>`).join("")}</div>
    <div class="row" id="restart" hidden><span class="help">Input Monitoring may need a restart to take effect.</span><button class="small" id="relaunch" type="button">Restart Yappaflow</button></div>
    <div class="row" id="fnnote" hidden><span class="help">Set <b>Press 🌐 key to: Do Nothing</b> in System Settings › Keyboard, or the emoji picker opens along with dictation.</span><button class="small" id="kbd" type="button">Open Keyboard settings</button></div></section>${foot("Continue", { back: true, skip: true })}`,
  style: () => `<section>${head(num("style"), "How do you usually type?", "Yappaflow will match it. Spoken lists always become separate lines.")}
    <div class="col" role="radiogroup" aria-label="Typing style">${STYLES.map(
      ([v, t, ex]) =>
        `<label class="opt list"><span class="col" style="gap:4px"><b>${t}</b><span class="help" dir="auto">${ex}</span></span><input type="radio" name="ts" value="${v}"${st.typing_style === v ? " checked" : ""}></label>`,
    ).join("")}</div></section>${foot("Next")}`,
  mic: () => `<section>${head(num("mic"), "Let's hear you", "Say &quot;testing, one, two, three&quot; at your normal volume.")}
    <label class="col" style="gap:8px"><span>Microphone</span><select class="field" id="mic"></select></label>
    <div class="meter" id="meter" aria-hidden="true">${"<span></span>".repeat(40)}</div>
    <div class="status" id="mstat" role="status">Listening…</div></section>${foot("Sounds good")}`,
  try: () => `<section>${head(num("try"), "Try it, mistakes included", `Click the box, hold ${kbds()} and read this out, um and all:`)}
    <div class="script" dir="auto">"Um, hey Sara, can we move lunch to Thursday… no, actually Friday?"</div>
    <textarea class="practice" id="practice" dir="auto" aria-label="Practice box" placeholder="Practice box"></textarea>
    <div class="status" id="tstat" role="status"></div></section>${foot("It worked", { back: true, skip: true })}`,
  done: () => `<section>${head(num("done"), "You're set.", "Three things worth knowing:")}
    <div class="tips">
      <div class="tip"><span>${kbds()} <span class="help">×2</span></span><b>Long thoughts</b><span class="help">Double-tap for hands-free. Talk as long as you like, then tap once to finish.</span></div>
      <div class="tip"><span><kbd class="kbd">Esc</kbd></span><b>Changed your mind?</b><span class="help">Esc cancels while listening or transcribing. Nothing gets pasted.</span></div>
      <div class="tip"><span aria-hidden="true">▾</span><b>Lives in the ${isMac ? "menu bar" : "tray"}</b><span class="help">Settings, history and "paste last" are one click away, ${isMac ? "at the top of the screen" : "next to the clock"}.</span></div>
    </div></section>${foot("Start yapping", { back: true })}`,
};

const PERMS = [
  ["microphone", "Microphone", "To hear you. Only while the hotkey is held."],
  ["input_monitoring", "Input Monitoring", "To notice when you hold fn, in any app."],
  ["accessibility", "Accessibility", "To paste the text where your cursor is."],
];
type Perms = Record<string, boolean>;

async function setupPerms() {
  const next = q<HTMLButtonElement>("#next");
  let imBefore: boolean | undefined;
  PERMS.forEach(([id]) => q(`#b-${id}`).addEventListener("click", () => void invoke("request_permission", { kind: id }).catch(() => {})));
  q("#relaunch").addEventListener("click", () => void invoke("restart_app"));
  q("#kbd").addEventListener("click", () => void invoke("open_system_settings", { kind: "keyboard" }));
  if (await invoke<boolean>("fn_key_conflict").catch(() => false)) q("#fnnote").hidden = false;
  const tick = async () => {
    const p = await invoke<Perms>("permission_status").catch(() => ({}) as Perms);
    PERMS.forEach(([id]) => {
      const st = q(`#p-${id}`);
      const locked = id === "microphone" && p.microphone_restricted;
      st.className = "status" + (p[id] ? " ok" : locked ? " bad" : "");
      st.textContent = p[id] ? "Allowed" : locked ? "Blocked by your organisation. Use Skip to carry on." : "";
      q<HTMLButtonElement>(`#b-${id}`).hidden = !!p[id] || !!locked;
    });
    if (imBefore === false && p.input_monitoring) q("#restart").hidden = false;
    imBefore = !!p.input_monitoring;
    next.disabled = !PERMS.every(([id]) => p[id]);
  };
  await tick();
  const t = setInterval(tick, 1000);
  stopPoll = () => clearInterval(t);
}

async function setupConnect() {
  const box = q("#provs");
  box.append(
    ...opts.stt_providers.map((p) => {
      const l = document.createElement("label");
      l.className = "opt list";
      const name = document.createElement("span");
      // A fixed language (AssemblyAI) or a server URL (Custom) is set in Settings › Models, not here.
      const later = p.needs_language || p.free_model;
      name.textContent = p.label.replace(/\s*\((cloud|coming later)\)/i, "") + (!p.available ? " · coming later" : later ? " · set up in Settings" : "");
      const r = document.createElement("input");
      r.type = "radio";
      r.name = "prov";
      r.value = p.id;
      r.disabled = !p.available || !!later;
      r.checked = p.id === st.stt_provider;
      l.append(name, r);
      return l;
    }),
  );
  const kstat = q("#kstat");
  const next = q<HTMLButtonElement>("#next");
  const prov = () => checked("prov") ?? st.stt_provider;
  const refresh = async () => {
    const m = await invoke<string>("provider_key_status", { provider: prov() }).catch(() => "");
    keyOk = !!m;
    q("#langnote").textContent = opts.stt_providers.find((x) => x.id === prov())?.lang_note ?? "";
    kstat.className = "status" + (m ? " ok" : "");
    kstat.textContent = m ? `Key saved (${m})` : "";
    next.disabled = !keyOk && checked("where") !== "local";
  };
  const where = () => {
    q("#cloudbox").hidden = checked("where") === "local";
    void refresh();
  };
  document.querySelectorAll('input[name="where"]').forEach((r) => r.addEventListener("change", where));
  q("#cloudbox").hidden = checked("where") === "local";
  box.addEventListener("change", refresh);
  q("#key").addEventListener("input", () => (next.disabled = !keyOk && !q<HTMLInputElement>("#key").value.trim()));
  q("#test").addEventListener("click", async () => {
    const p = prov();
    const k = q<HTMLInputElement>("#key").value.trim();
    kstat.className = "status";
    kstat.textContent = "Testing…";
    try {
      if (k) await invoke("set_provider_key", { provider: p, key: k });
      const msg = await invoke<string>("test_provider_key", { provider: p });
      q<HTMLInputElement>("#key").value = "";
      await refresh();
      kstat.className = "status ok";
      kstat.textContent = msg;
    } catch (e) {
      keyOk = false;
      kstat.className = "status bad";
      kstat.textContent = String(e);
    }
  });
  await refresh();
}

async function setupMic() {
  const sel = q<HTMLSelectElement>("#mic");
  const names = await invoke<string[]>("list_mics").catch(() => [] as string[]);
  sel.add(new Option("System default", ""));
  names.forEach((n) => sel.add(new Option(n, n)));
  sel.value = names.includes(st.mic_device) ? st.mic_device : "";
  const bars = [...document.querySelectorAll<HTMLElement>("#meter span")];
  const mstat = q("#mstat");
  let heard = false;
  const start = async () => {
    stopMeter?.();
    try {
      // The webview's device labels usually contain the OS device name; otherwise use its default mic.
      const probe = await navigator.mediaDevices.getUserMedia({ audio: true });
      probe.getTracks().forEach((t) => t.stop());
      const dev = (await navigator.mediaDevices.enumerateDevices()).find(
        (d) => d.kind === "audioinput" && sel.value && d.label && (d.label.includes(sel.value) || sel.value.includes(d.label)),
      );
      const stream = await navigator.mediaDevices.getUserMedia({ audio: dev ? { deviceId: { exact: dev.deviceId } } : true });
      const ac = new AudioContext();
      const an = ac.createAnalyser();
      an.fftSize = 512;
      ac.createMediaStreamSource(stream).connect(an);
      const buf = new Uint8Array(an.fftSize);
      const hist: number[] = new Array(bars.length).fill(0);
      const id = setInterval(() => {
        an.getByteTimeDomainData(buf);
        const peak = Math.min(1, (buf.reduce((m, v) => Math.max(m, Math.abs(v - 128)), 0) / 128) * 3);
        hist.shift();
        hist.push(peak);
        bars.forEach((b, i) => {
          b.style.height = `${6 + hist[i] * 58}px`;
          b.classList.toggle("on", hist[i] > 0.08);
        });
        if (peak > 0.25 && !heard) {
          heard = true;
          mstat.className = "status ok";
          mstat.textContent = "Heard you clearly";
        }
      }, 50);
      stopMeter = () => {
        clearInterval(id);
        stream.getTracks().forEach((t) => t.stop());
        void ac.close();
      };
    } catch {
      mstat.className = "status bad";
      mstat.textContent = isMac ? "Couldn't open the microphone. Check System Settings › Privacy & Security › Microphone." : "Couldn't open the microphone. Check Windows Settings › Privacy › Microphone.";
    }
  };
  sel.addEventListener("change", () => {
    heard = false;
    mstat.className = "status";
    mstat.textContent = "Listening…";
    void save({ mic_device: sel.value });
    void start();
  });
  await start();
}

async function setupTry() {
  const box = q<HTMLTextAreaElement>("#practice");
  box.focus();
  const tstat = q("#tstat");
  unlisten = await listen<{ state: string; text?: string; total_ms?: number }>("status", (e) => {
    const s = e.payload;
    tstat.className = "status";
    if (s.state === "listening") tstat.textContent = "Listening…";
    else if (s.state === "processing") tstat.textContent = "Transcribing…";
    else if (s.state === "done") {
      tstat.className = "status ok";
      tstat.textContent = `Done · ${((s.total_ms ?? 0) / 1000).toFixed(2)} s`;
      // The paste only lands if the box kept focus; make sure the text shows either way.
      if (s.text && !box.value.includes(s.text)) box.value += s.text;
    } else if (s.state === "error") {
      tstat.className = "status bad";
      tstat.textContent = "That didn't work. Check your key, then try again.";
    }
  });
}

/** Onb2Local: the recommended speech model downloads while the user carries on. */
const localHtml = () => {
  const rec = local!.recommended;
  const stt = local!.models.filter((m) => m.kind === "stt");
  const cl = local!.models.find((m) => m.id === rec?.cleanup);
  const runs = rec?.runs_on === "CPU" ? "runs on your processor" : "runs on your graphics card";
  return `<section>${head(num("connect"), "Downloading your model", `One-time download. We picked the best fit for ${local!.hardware.gpu_name ? "your graphics card" : isMac ? "this Mac" : "this PC"}.`)}
    <div class="col card">
      <div class="row" style="justify-content:space-between;align-items:flex-start"><div class="col" style="gap:6px"><b id="lname"></b><span class="help">Speech to text · 99 languages · ${runs}</span></div>
      <select class="field" id="lmodel" aria-label="Choose another speech model" style="width:auto">${stt.map((m) => `<option value="${h(m.id)}"${m.id === st.stt_model ? " selected" : ""}>${h(m.display_name)} · ${fmt(m.size_bytes)}</option>`).join("")}</select></div>
      <div class="prog" role="progressbar" aria-label="Download progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow="0"><div id="lfill"></div></div>
      <div class="row help" style="justify-content:space-between"><span id="ldone"></span><span id="leta"></span></div>
      <div class="status" id="lstat" role="status"></div>
    </div>
    ${cl ? `<label class="opt list" style="flex-direction:row;gap:14px;align-items:flex-start"><input type="checkbox" id="lclean" checked style="position:static;opacity:1;width:18px;height:18px;flex:none;accent-color:var(--accent)">
      <span class="col" style="gap:6px"><span>Also download a local cleanup model · ${fmt(cl.size_bytes)}</span><span class="help">Removes fillers, fixes self-corrections and punctuation, all offline.</span></span></label>` : ""}
    </section><div class="foot"><button class="back" id="back" type="button">Back</button><div class="row"><span class="help">You can continue while it downloads</span><button class="cta" id="next" type="button">Continue</button></div></div>`;
};

async function setupLocal() {
  const sel = q<HTMLSelectElement>("#lmodel");
  const stat = q("#lstat");
  let started = Date.now();
  let first = -1;
  const show = (done: number, total: number, state: string) => {
    const pct = Math.floor((done * 100) / Math.max(1, total));
    q("#lfill").style.width = `${pct}%`;
    q(".prog").setAttribute("aria-valuenow", String(pct));
    q("#ldone").textContent = state === "installed" ? `Ready · ${fmt(total)}` : `${fmt(done)} of ${fmt(total)}`;
    if (first < 0) { first = done; started = Date.now(); }
    const rate = (done - first) / Math.max(1, (Date.now() - started) / 1000);
    q("#leta").textContent = state === "downloading" && rate > 0 ? `about ${Math.max(1, Math.round((total - done) / rate))} s left` : state === "paused" ? "Paused" : "";
  };
  let prev = "";
  const pick = async () => {
    // Switching models: pause the one we were fetching (it can resume later in Settings).
    if (prev && prev !== sel.value) await invoke("pause_download", { id: prev }).catch(() => {});
    prev = sel.value;
    local = await invoke<LocalInfo>("local_models").catch(() => local);
    const m = local!.models.find((x) => x.id === sel.value) ?? local!.models[0];
    q("#lname").textContent = `Whisper ${m.display_name}`;
    await save({ stt_provider: "whisper_cpp", stt_model: m.id });
    first = -1;
    show(m.state === "installed" ? m.size_bytes : m.done_bytes, m.size_bytes, m.state);
    if (m.state !== "installed") await invoke("download_model", { id: m.id }).catch((e) => { stat.className = "status bad"; stat.textContent = String(e); });
  };
  sel.addEventListener("change", pick);
  unlistenDl = await listen<DownloadEvent>("model_download", (e) => {
    const d = e.payload;
    if (d.id !== sel.value) return;
    show(d.done, d.total, d.state);
    if (d.error) { stat.className = "status bad"; stat.textContent = d.error; }
  });
  await pick();
}

async function render() {
  unlistenDl?.();
  unlistenDl = undefined;
  stopMeter?.();
  stopMeter = undefined;
  stopPoll?.();
  stopPoll = undefined;
  unlisten?.();
  unlisten = undefined;
  const id = ORDER[step];
  const onLocal = id === "connect" && localScreen;
  app.innerHTML = onLocal ? localHtml() : screens[id]();
  q("#back")?.addEventListener("click", () => {
    if (!onLocal) return go(-1);
    localScreen = false;
    void render();
  });
  q("#skip")?.addEventListener("click", () => {
    // E.g. a managed Mac where the microphone is "restricted": carry on, clipboard-only.
    if (id === "perms" && !confirm("Without all three switches Yappaflow can't hear the hotkey, record or paste for you. Dictations that do work stay on the clipboard. Skip anyway?")) return;
    go(1);
  });
  q("#next").addEventListener("click", async () => {
    if (onLocal) {
      const cl = local!.recommended?.cleanup;
      const want = !!cl && !!document.querySelector<HTMLInputElement>("#lclean")?.checked;
      // No cloud cleanup on the offline path: it would need a key and the network.
      await save(want ? { cleanup_provider: "llama_cpp", cleanup_model: cl! } : { cleanup_provider: "none" });
      if (want) await invoke("download_model", { id: cl }).catch((e) => console.warn(e));
    } else if (id === "connect" && checked("where") === "local") {
      localScreen = true;
      if (st.stt_provider !== "whisper_cpp" && local!.recommended) await save({ stt_provider: "whisper_cpp", stt_model: local!.recommended.stt });
      void render();
      return;
    } else if (id === "connect") {
      const p = checked("prov") ?? st.stt_provider;
      const patch: Partial<Settings> = { stt_provider: p };
      const sp = opts.stt_providers.find((x) => x.id === p);
      if (sp?.models.length) patch.stt_model = sp.models[0];
      const cp = opts.cleanup_providers.find((c) => c.id === p && c.available);
      if (cp) {
        patch.cleanup_provider = p;
        if (cp.models.length) patch.cleanup_model = cp.models[0];
      }
      await save(patch);
    } else if (id === "style") {
      await save({ typing_style: checked("ts") ?? "full" });
    } else if (step === TOTAL - 1) {
      await invoke("finish_onboarding");
      return;
    }
    go(1);
  });
  if (onLocal) await setupLocal();
  else if (id === "connect") await setupConnect();
  if (id === "perms") await setupPerms();
  if (id === "mic") await setupMic();
  if (id === "try") await setupTry();
  else (document.querySelector<HTMLElement>("#next:not(:disabled)") ?? q("#next")).focus();
}
function go(d: number) {
  step = Math.max(0, Math.min(TOTAL - 1, step + d));
  void render();
}

(async () => {
  [st, opts, local] = await Promise.all([invoke<Settings>("get_settings"), invoke<Opts>("get_options"), invoke<LocalInfo>("local_models").catch(() => undefined)]);
  document.documentElement.dataset.accent = st.accent;
  if (st.theme === "light" || st.theme === "dark") document.documentElement.dataset.theme = st.theme;
  else document.documentElement.removeAttribute("data-theme");
  await render();
})();
