import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { btn, ctx, h, page, row, save, section, select, sw } from "./common";

export function mic() {
  const s = ctx.settings;
  const bar = h("div", { className: "meter-fill" });
  const meter = h("div", { className: "meter", role: "meter", ariaLabel: "Microphone level", ariaValueMin: "0", ariaValueMax: "100" }, bar);
  const setLevel = (p: number) => {
    const v = Math.round(Math.min(1, p * 3) * 100);
    bar.style.width = `${v}%`;
    meter.setAttribute("aria-valuenow", String(v));
  };
  let decay = 0;
  const unlisten = listen<number>("level", (e) => { decay = Math.max(e.payload, decay * 0.6); setLevel(decay); });

  const micSel = h("div");
  invoke<string[]>("list_mics").then((mics) => {
    const items = [{ value: "", label: "System default" }, ...mics.map((m) => ({ value: m, label: m }))];
    if (s.mic_device && !mics.includes(s.mic_device)) items.push({ value: s.mic_device, label: `${s.mic_device} (not connected)` });
    micSel.replaceChildren(select(items, s.mic_device, (v) => save({ mic_device: v }), "m-dev", 260));
  }).catch(() => {});

  // 3 s test through the webview's own mic access (system default device only).
  const res = h("div", { className: "help", role: "status" });
  const test = btn("Test: record 3 s", "g", async () => {
    test.disabled = true;
    res.textContent = "Recording…";
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ac = new AudioContext();
      const an = ac.createAnalyser();
      an.fftSize = 512;
      ac.createMediaStreamSource(stream).connect(an);
      const buf = new Uint8Array(an.fftSize);
      let peak = 0;
      const tick = setInterval(() => {
        an.getByteTimeDomainData(buf);
        const p = Math.max(...buf.map((b) => Math.abs(b - 128))) / 128;
        peak = Math.max(peak, p);
        setLevel(p);
      }, 50);
      const rec = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      rec.ondataavailable = (e) => chunks.push(e.data);
      rec.onstop = () => {
        clearInterval(tick);
        stream.getTracks().forEach((t) => t.stop());
        void ac.close();
        setLevel(0);
        const db = 20 * Math.log10(Math.max(peak, 1e-4));
        const verdict = db < -45 ? "Too quiet. Move closer or raise the input volume." : db > -3 ? "Very loud, it may clip." : "Clear and loud enough.";
        res.textContent = `${verdict} Peak ${db.toFixed(0)} dB. Playing back…`;
        const a = new Audio(URL.createObjectURL(new Blob(chunks)));
        a.onended = () => { test.disabled = false; };
        a.play().catch(() => (test.disabled = false));
      };
      rec.start();
      setTimeout(() => rec.stop(), 3000);
    } catch (e) {
      res.textContent = `Couldn't use the microphone here: ${e}`;
      test.disabled = false;
    }
  });

  const gateVal = h("span", { className: "val" }, `${s.silence_threshold_dbfs} dB`);
  const gate = h("input", {
    id: "m-gate", className: "range", type: "range", min: "-70", max: "-20", step: "1", value: String(s.silence_threshold_dbfs),
    oninput: () => (gateVal.textContent = `${gate.value} dB`),
    onchange: () => save({ silence_threshold_dbfs: Number(gate.value) }),
  });

  return Object.assign(
    page("Microphone", "Pick a mic and check that it hears you clearly.",
      section("Input",
        row("Input device", null, micSel, "m-dev"),
        row("Level", "Moves while you dictate or run the test.", meter),
        row("Check your mic", "Records 3 seconds from the system default mic and plays it back.", h("div", { className: "stack" }, test, res))),
      section("Fine-tuning",
        row("Ignore quiet audio below", "Clips quieter than this are thrown away instead of sent, so you never get a random \"Thank you.\" from silence.",
          h("div", { className: "actions" }, gate, gateVal), "m-gate"),
        row("Keep the mic ready", "Starts about 0.1 s faster. Not available yet.", sw(false, () => {}, true)),
        row("Lower other audio while I talk", "Music and videos dip while you dictate. Not available yet.", sw(false, () => {}, true)))),
    { cleanup: () => void unlisten.then((f) => f()) },
  );
}
