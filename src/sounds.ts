// Soft start/stop ticks, synthesised with WebAudio (no audio files). Kept quiet on purpose.
let ctx: AudioContext | undefined;

export function tick(kind: "start" | "stop") {
  try {
    ctx ??= new AudioContext();
    if (ctx.state === "suspended") void ctx.resume();
    const t = ctx.currentTime;
    const [f0, f1] = kind === "start" ? [620, 880] : [660, 440]; // up = begin, down = end
    const osc = ctx.createOscillator();
    const g = ctx.createGain();
    osc.type = "sine";
    osc.frequency.setValueAtTime(f0, t);
    osc.frequency.exponentialRampToValueAtTime(f1, t + 0.07);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.linearRampToValueAtTime(0.06, t + 0.008); // low volume, soft attack
    g.gain.exponentialRampToValueAtTime(0.0001, t + 0.11);
    osc.connect(g).connect(ctx.destination);
    osc.start(t);
    osc.stop(t + 0.12);
  } catch {
    /* no audio device or autoplay blocked: stay silent */
  }
}
