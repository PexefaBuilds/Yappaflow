import { isMac } from "../os";
import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, h, page, row, seg } from "./common";

interface Binding { action: string; label: string; keys: string[]; display: string; default_keys: string[]; enabled: boolean }

const HELP: Record<string, string> = {
  dictate: "Hold to talk, release to paste.",
  hands_free: "Keep listening without holding. Press it again, or the dictate shortcut, to finish. Esc cancels.",
  cancel: "While listening or transcribing. Nothing is pasted.",
  paste_last: "For when the text went to the wrong window.",
  toggle_pause: "Stop listening for hotkeys until you press it again.",
  command_mode: "Select text, hold the keys and say how to change it (\"make it friendlier\"). With nothing selected it writes new text at the cursor. If it contains your dictate shortcut, add the extra key while dictating.",
};
const keyName = (e: KeyboardEvent) =>
  isMac && e.code === "AltRight" ? "RightOption" : isMac && e.key === "Meta" ? "Cmd" : isMac && e.key === "Fn" ? "Fn" :
  e.key === "Control" ? "Ctrl" : e.key === "Meta" ? "Win" : e.key.length === 1 ? e.key.toUpperCase() : e.key.replace(/^Arrow/, "");

export function hotkeys() {
  const body = h("div", { className: "card" }, h("p", { className: "help pad" }, "Loading…"));
  const msg = h("p", { className: "help note", role: "status" });
  const say = (t: string, bad = false) => { msg.textContent = t; msg.className = `help note${bad ? " bad" : ""}`; };
  let capturing = false;
  let doubleTap = true;
  let dictateDisplay = "Ctrl + Win";

  async function load() {
    try { doubleTap = (await invoke<{ hands_free_double_tap?: boolean }>("get_settings")).hands_free_double_tap !== false; } catch { /* default */ }
    let list: Binding[];
    try { list = await invoke<Binding[]>("get_hotkeys"); }
    catch {
      try { // older backend: dictate only
        const d = await invoke<{ display: string; keys: string[] }>("get_hotkey");
        list = [{ action: "dictate", label: "Dictate", keys: d.keys, display: d.display, default_keys: ["Ctrl", "Win"], enabled: true }];
      } catch { body.replaceChildren(h("p", { className: "help pad bad" }, "Hotkey settings aren't available in this build.")); return; }
    }
    dictateDisplay = list.find((b) => b.action === "dictate")?.display ?? dictateDisplay;
    body.replaceChildren(...list.map(rowFor));
  }

  // Hands-free: double-tap the dictate shortcut (default), or its own shortcut (double-tap is then off).
  const setDoubleTap = async (on: boolean) => {
    try {
      await invoke("set_hands_free_double_tap", { on });
      ctx.settings.hands_free_double_tap = on;
      say(on ? `Hands-free: double-tap ${dictateDisplay}.` : "Hands-free: its own shortcut. A quick tap of the dictate shortcut is now a short dictation.");
    } catch (err) { say(String(err), true); }
    await load();
  };

  function rowFor(b: Binding) {
    const kbd = h("kbd", { className: "kbd" }, b.enabled ? b.display : "Off");
    const actions = h("div", { className: "actions" });
    const same = (a: string[], c: string[]) => a.join("+").toLowerCase() === c.join("+").toLowerCase();
    const optional = b.action !== "dictate";

    const apply = async (keys: string[]) => {
      try {
        await invoke("set_hotkey_for", { action: b.action, keys }).catch((err) => {
          if (b.action === "dictate" && keys.length && String(err).includes("not found")) return invoke("set_hotkey", { keys });
          throw err;
        });
        say(keys.length ? "Saved." : `${b.label} turned off.`);
      } catch (err) { say(String(err), true); }
      await load();
    };
    const idle = () => {
      actions.replaceChildren(...[
        btn("Change", "s", () => startCapture()),
        !same(b.keys, b.default_keys) ? btn("Reset", "s", async () => { try { await invoke("reset_hotkey", { action: b.action }); say("Reset to default."); } catch (e) { say(String(e), true); } await load(); }) : null,
        optional && b.enabled ? btn("Clear", "s", () => apply([])) : null,
      ].filter((x): x is HTMLButtonElement => !!x));
      actions.querySelectorAll("button").forEach((x) => x.setAttribute("aria-label", `${x.textContent} ${b.label} hotkey`));
    };

    // A dialog (not inline): "press the keys", see them, then Done saves and returns here.
    function startCapture() {
      if (capturing) return;
      capturing = true;
      void invoke("set_hotkey_capture", { on: true }).catch(() => {});
      let chord: string[] = [];
      const held = new Set<string>();
      let locked = false; // all keys released: chord is final, buttons take over
      let warn: string[] = [];

      const dlg = document.createElement("dialog");
      dlg.className = "hk-dialog";
      dlg.setAttribute("aria-labelledby", "hk-title");
      const title = h("h2", { id: "hk-title" }, `Set the ${b.label} hotkey`);
      const hint = h("p", { className: "help" }, "Click here, then press and hold the keys you want to use, and release. Up to 3 keys.");
      const keys = h("div", { className: "hk-keys", role: "status", "aria-live": "polite" }, "Waiting for keys…");
      const note = h("p", { className: "help note", role: "status" });
      const retry = btn("Try again", "s", () => reset());
      const cancel = btn("Cancel", "g", () => dlg.close());
      const done = btn("Done", "p", () => void save(), true);
      // fn can't be recorded from a web page on macOS, so the Mac chords are one click away.
      const presets = isMac ? h("div", { className: "actions" }, h("span", { className: "help" }, "Or pick:"),
        ...[["fn", ["Fn"]], ["Right ⌥", ["RightOption"]], ["⌃ ⌥", ["Control", "Option"]], ["fn ⌃", ["Control", "Fn"]]].map(([label, ks]) =>
          btn(label as string, "s", () => void pick(ks as string[])))) : null;
      dlg.append(title, hint, keys, ...(presets ? [presets] : []), note, h("div", { className: "actions hk-actions" }, retry, cancel, done));

      const render = () => {
        keys.replaceChildren(...(chord.length ? chord.map((k) => h("kbd", { className: "kbd" }, k)) : ["Waiting for keys…"]));
        note.textContent = warn.length ? `${warn.join(" ")} You can still use it.` : "";
        note.className = `help note${warn.length ? " bad" : ""}`;
        done.disabled = !(locked && chord.length);
        done.textContent = warn.length ? "Use anyway" : "Done";
      };
      const reset = () => { chord = []; held.clear(); locked = false; warn = []; render(); keys.focus(); };

      const down = (e: KeyboardEvent) => {
        if (locked) return; // let Tab/Enter operate the buttons
        e.preventDefault(); e.stopPropagation();
        if (e.repeat) return;
        const n = keyName(e);
        if (e.key === "Escape" && !chord.length && b.action !== "cancel") { dlg.close(); return; }
        if (!held.has(n) && chord.length < 3) chord.push(n);
        held.add(n);
        render();
      };
      const up = async (e: KeyboardEvent) => {
        if (locked) return;
        e.preventDefault();
        held.delete(keyName(e));
        if (held.size || !chord.length) return;
        locked = true;
        try {
          warn = await invoke<string[]>("check_hotkey_conflict_for", { action: b.action, keys: chord })
            .catch(() => invoke<string[]>("check_hotkey_conflict", { keys: chord }));
        } catch (err) { warn = [String(err)]; }
        render();
        done.focus();
      };
      const pick = async (ks: string[]) => {
        chord = ks; held.clear(); locked = true;
        try { warn = await invoke<string[]>("check_hotkey_conflict_for", { action: b.action, keys: chord }); } catch (err) { warn = [String(err)]; }
        render();
        done.focus();
      };
      const save = async () => {
        const picked = chord;
        dlg.close();
        await apply(picked);
      };
      dlg.addEventListener("keydown", down, true);
      dlg.addEventListener("keyup", up, true);
      dlg.addEventListener("close", () => {
        capturing = false;
        void invoke("set_hotkey_capture", { on: false }).catch(() => {});
        dlg.remove();
      });
      keys.tabIndex = 0;
      document.body.append(dlg);
      dlg.showModal();
      render();
      keys.focus();
    }
    idle();
    if (b.action === "hands_free") {
      const mode = seg("Start hands-free with", [
        { value: "double", label: "Double-tap dictate" },
        { value: "own", label: "Its own shortcut" },
      ], doubleTap ? "double" : "own", (v) => void setDoubleTap(v === "double"));
      const help = doubleTap
        ? `Double-tap ${dictateDisplay} and keep talking. Tap it again to finish. Esc cancels.`
        : `${HELP.hands_free}${b.enabled ? "" : " Click Change to pick the keys."}`;
      return row(b.label, help, h("div", { className: "actions" }, mode, ...(doubleTap ? [] : [kbd, actions])));
    }
    return row(b.label, HELP[b.action] ?? null, h("div", { className: "actions" }, kbd, actions));
  }

  void load();
  // macOS: 🌐/fn bound to emoji, input source or dictation fires alongside us. We never change it ourselves.
  const fnNote = h("div", { role: "status" });
  if (isMac) void invoke<boolean>("fn_key_conflict").then((bad) => {
    if (bad) fnNote.replaceChildren(h("div", { className: "card" }, h("div", { className: "row" },
      h("p", { className: "help" }, "Set Press 🌐 key to: Do Nothing in System Settings › Keyboard, or the emoji picker or input switcher opens along with dictation."),
      btn("Open Keyboard settings", "s", () => void invoke("open_system_settings", { kind: "keyboard" })))));
  }).catch(() => {});
  return page("Hotkeys", "Click Change, then press the new keys. Works in every app, including full-screen ones.",
    fnNote, h("h2", { className: "cap" }, "Shortcuts"), body, msg,
    h("p", { className: "help note" }, isMac ? "Mac shortcuts still work. If you press fn + an arrow key, Yappaflow notices the extra key, cancels quietly and lets the shortcut through." : "Windows shortcuts still work. If you press Ctrl + Win + → to switch desktops, Yappaflow notices the extra key, cancels quietly and lets the shortcut through."));
}
