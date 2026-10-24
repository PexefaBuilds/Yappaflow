import { isMac } from "../os";
import { invoke } from "@tauri-apps/api/core";
import { btn, ctx, Dictation, h, page, row, save, section, select, sw } from "./common";
import { download, toCsv } from "./csv";

/** Modal yes/no (reuses the hotkey dialog styles). Resolves true only on the confirm button. */
function confirmDialog(title: string, body: string, ok: string, danger = false): Promise<boolean> {
  return new Promise((resolve) => {
    let yes = false;
    const dlg = h("dialog", { className: "hk-dialog", "aria-labelledby": "cf-title" });
    const go = btn(ok, danger ? "g" : "p", () => { yes = true; dlg.close(); });
    if (danger) go.classList.add("danger");
    dlg.append(h("h2", { id: "cf-title" }, title), h("p", { className: "help" }, body),
      h("div", { className: "actions hk-actions" }, btn("Cancel", "g", () => dlg.close()), go));
    dlg.addEventListener("close", () => { dlg.remove(); resolve(yes); });
    document.body.append(dlg);
    dlg.showModal();
  });
}

async function exportHistory(kind: "json" | "csv") {
  try {
    const rows = await invoke<Dictation[]>("recent_dictations", { limit: 4294967295 });
    if (kind === "json") return download("yappaflow-history.json", JSON.stringify(rows, null, 2), "application/json");
    const cols = ["id", "created_at", "app_exe", "window_title", "mode", "duration_ms", "raw_text", "final_text", "stt_model", "status", "error"] as const;
    download("yappaflow-history.csv", toCsv([[...cols], ...rows.map((d) => cols.map((c) =>
      c === "created_at" ? new Date(d.created_at).toISOString() : String(d[c] ?? "")))]));
  } catch (e) {
    ctx.toast(`Couldn't export history: ${e}`, true);
  }
}

export function advanced() {
  const s = ctx.settings;
  const delay = h("input", { id: "ad-delay", className: "input", type: "number", min: "0", max: "5000", step: "50", style: "width:110px", value: String(s.restore_delay_ms),
    onchange: () => save({ restore_delay_ms: Math.max(0, Math.min(5000, Number(delay.value) || 0)) }) });
  const keep = [[50, "Last 50"], [500, "Last 500"], [0, "Don't keep audio"]].map(([v, l]) => ({ value: String(v), label: String(l) }));
  const base = h("input", { id: "ad-base", className: "input", type: "text", spellcheck: false, style: "width:300px", value: s.openai_base_url,
    onchange: () => save({ openai_base_url: base.value.trim() }) });
  const adminMsg = h("div", { className: "help", role: "status" });
  const admin = sw(!!s.admin_mode, async (on) => {
    adminMsg.textContent = "";
    if (on && (await invoke<boolean>("is_standard_user"))) {
      admin.checked = false;
      adminMsg.textContent = "This Windows account isn't an administrator. Elevating would run Yappaflow as a different account, with its own settings, history and keys, so admin mode stays off. Sign in with an administrator account to use it.";
      return;
    }
    if (on && !(await confirmDialog("Restart with admin rights?",
      "Yappaflow will restart as administrator, and Windows will ask for permission each time it starts. Any app running as administrator can be typed into, so only turn this on if you need it.",
      "Restart as admin"))) { admin.checked = false; return; }
    try {
      await invoke("set_admin_mode", { on }); // on: this window closes as the app restarts
      ctx.settings.admin_mode = on;
      if (!on && (await invoke<boolean>("is_elevated"))) adminMsg.textContent = "Quit and reopen Yappaflow to drop admin rights.";
    } catch (e) {
      admin.checked = !on;
      adminMsg.textContent = String(e);
    }
  }, false, "ad-admin");
  const del = btn("Delete all history", "g", async () => {
    if (!(await confirmDialog("Delete all history?", "Every dictation and every saved recording on this PC is deleted. This can't be undone.", "Delete everything", true))) return;
    del.disabled = true;
    del.textContent = "Deleting…";
    try { await invoke("delete_all_history"); ctx.toast("History deleted."); } catch (e) { ctx.toast(String(e), true); }
    finally { del.disabled = false; del.textContent = "Delete all history"; }
  });
  del.classList.add("danger");

  return page("Advanced", "For when something doesn't paste quite right.",
    section("Pasting",
      row("Wait before restoring the clipboard (ms)", "Raise this if some apps paste your old clipboard instead of the dictation.", delay, "ad-delay"),
      ...(isMac ? [] : [row("Work in admin windows", h("div", {}, "Windows blocks normal apps from typing into windows running as administrator. Turning this on restarts Yappaflow with admin rights.", adminMsg), admin, "ad-admin")])),
    section("Privacy & data",
      row("Keep recordings", "So you can retry or replay. Stored only on this PC.",
        select(keep, String(keep.some((k) => k.value === String(s.keep_recordings)) ? s.keep_recordings : 50), (v) => save({ keep_recordings: Number(v) }), "ad-keep", 200), "ad-keep"),
      row("Recordings folder", null, btn("Open folder", "g", () => void invoke("open_recordings_folder")))),
    section("Your data",
      row("Data folder", "Settings, history and recordings. Nothing leaves this PC.", btn("Open data folder", "g", () => void invoke("open_app_folder", { which: "data" }))),
      row("Export history", "Every dictation with its text, app and timings.",
        h("div", { className: "actions" }, btn("JSON", "s", () => void exportHistory("json")), btn("CSV", "s", () => void exportHistory("csv")))),
      row("Delete all history", "Removes every dictation and recording.", del),
      row("Logs", "Something off? Logs help us fix it. Keys are never written to them.", btn("Open log folder", "g", () => void invoke("open_app_folder", { which: "logs" })))),
    section("Endpoints",
      row("OpenAI base URL", "Model IDs and endpoints are settings. Change only for a compatible proxy.", base, "ad-base")));
}

/** Shows a bundled licence file (read by the backend from the app's resources) in a modal. */
async function showLicence(which: "notices" | "fonts", title: string) {
  let text: string;
  try { text = await invoke<string>("read_licence", { which }); } catch (e) { return ctx.toast(String(e), true); }
  const dlg = h("dialog", { className: "hk-dialog wide", "aria-labelledby": "lic-title" });
  dlg.append(h("h2", { id: "lic-title" }, title), h("pre", { className: "lic-text", tabIndex: 0, dir: "auto" }, text),
    h("div", { className: "actions hk-actions" }, btn("Close", "p", () => dlg.close())));
  dlg.addEventListener("close", () => dlg.remove());
  document.body.append(dlg);
  dlg.showModal();
}

type CatalogueModel = { id: string; display_name: string; kind: string; licence: string; state: string };

function licences() {
  const models = h("div", { className: "help" }, "Loading…");
  void invoke<{ models: CatalogueModel[] }>("local_models").then((info) => {
    models.replaceChildren(...info.models.map((m) => h("div", {},
      `${m.display_name} (${m.kind === "stt" ? "speech" : "cleanup"}): ${m.licence}${m.state === "installed" ? " · downloaded" : ""}`)));
  }).catch(() => { models.textContent = "Couldn't read the model catalogue."; });
  return section("Licences",
    row("Yappaflow", "MIT licence. Free to use, change and share.", null),
    row("Third-party software", "Licences of the Rust crates and npm packages inside this build.",
      btn("View notices", "g", () => void showLicence("notices", "Third-party notices"))),
    row("Fonts", "Unbounded and Reem Kufi, SIL Open Font License 1.1.",
      btn("View licence", "g", () => void showLicence("fonts", "SIL Open Font License 1.1"))),
    row("Local models", h("div", {}, "Downloaded from Hugging Face, not shipped with the app.", models), null));
}

function updates() {
  const status = h("div", { className: "help", role: "status", ariaLive: "polite" });
  const install = btn("Update now", "p", async () => {
    install.disabled = true;
    install.textContent = "Updating…";
    try { await invoke("install_update"); } catch (e) { status.textContent = String(e); install.disabled = false; install.textContent = "Update now"; }
  });
  install.hidden = true;
  const check = btn("Check for updates", "g", async () => {
    check.disabled = true;
    status.textContent = "Checking…";
    try {
      const v = await invoke<string | null>("check_update");
      status.textContent = v ? `Version ${v} is available.` : `You're on the latest version (${ctx.options.version}).`;
      install.hidden = !v;
      if (v) install.textContent = `Update to ${v}`;
    } catch (e) { status.textContent = String(e); }
    check.disabled = false;
  });
  const auto = sw(ctx.settings.auto_update, (v) => save({ auto_update: v }), false, "ab-auto");
  const channel = select([{ value: "stable", label: "Stable" }, { value: "beta", label: "Beta" }], ctx.settings.update_channel,
    (v) => save({ update_channel: v }), "ab-channel", 160);
  return section("Updates",
    row(`Version ${ctx.options.version}`, h("div", { className: "stack" }, status), h("div", { className: "actions" }, check, install)),
    row("Install updates automatically", "Downloads in the background, then asks before restarting.", auto, "ab-auto"),
    row("Update channel", "Beta gets new versions earlier, including test releases.", channel, "ab-channel"));
}

export function about() {
  const msg = h("div", { className: "help", role: "status" });
  return page("About", "Free and open source (MIT licence).",
    updates(),
    section("Yappaflow",
      row("Private by design", "Built on Tauri. No accounts, no analytics. Your keys and audio stay on your PC or go straight to the provider you chose.", null),
      row("Welcome tour", "Run the first-time setup again.", h("div", { className: "stack" }, btn("Re-run onboarding", "g", async () => {
        try { await invoke("show_onboarding"); msg.textContent = ""; } catch { msg.textContent = "Onboarding isn't available in this build yet."; }
      }), msg))),
    licences());
}

export const soon = (title: string, sub: string) => () =>
  page(title, sub, h("div", { className: "card empty" }, h("p", { className: "lbl" }, "Coming in an update"), h("p", { className: "help" }, "This page is a placeholder for now.")));
