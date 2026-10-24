mod appicon;
mod audio;
mod cleanup;
#[cfg(all(test, feature = "evals"))]
mod cleanup_evals;
#[cfg(all(test, feature = "evals"))]
mod command_evals;
mod commands;
mod context;
mod hardware;
mod hotkey;
mod hotkey_cmds;
mod insert;
mod learn;
mod logging;
mod models;
mod onboarding;
mod pill;
mod pipeline;
mod providers;
mod secrets;
mod session;
mod settings;
mod settings_cmds;
mod snippets;
mod storage;
#[cfg(all(test, feature = "evals"))]
mod stt_bench;
mod system_cmds;
mod toast;
mod tray;
mod usage;

/// "Work in admin windows": relaunch elevated so the hook and SendInput reach elevated apps.
/// Lives here (not in the four OS modules) because it's about the app process itself.
#[cfg(windows)]
pub(crate) mod elevate {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevationType, TokenElevationTypeDefault, TOKEN_ELEVATION_TYPE, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    use windows::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    const WAIT_ARG: &str = "--wait-pid";
    pub const STANDARD_USER: &str = "This Windows account isn't an administrator. Elevating would run Yappaflow as a different account, with separate settings, history and keys";

    pub fn is_elevated() -> bool {
        unsafe { IsUserAnAdmin() }.as_bool()
    }

    /// True for a standard (non-admin) account: UAC would ask for an administrator's password and
    /// run the app as that other account. An admin running unelevated has a "limited" split token instead.
    pub fn is_standard_user() -> bool {
        if is_elevated() {
            return false;
        }
        let mut token = HANDLE::default();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
            return false; // unknown: keep the old behaviour
        }
        let mut kind = TOKEN_ELEVATION_TYPE::default();
        let mut len = 0u32;
        let size = std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32;
        let ok = unsafe { GetTokenInformation(token, TokenElevationType, Some(&mut kind as *mut _ as *mut _), size, &mut len) }.is_ok();
        unsafe {
            let _ = CloseHandle(token);
        }
        // Default + not elevated: no admin token of our own to switch to (standard user, or UAC off).
        ok && kind == TokenElevationTypeDefault
    }

    /// Starts an elevated copy (UAC prompt). Ok means it launched; the caller should exit.
    /// Refuses for a standard account (see `is_standard_user`).
    pub fn relaunch_elevated() -> Result<(), String> {
        if is_standard_user() {
            return Err(STANDARD_USER.into());
        }
        let exe = std::env::current_exe().map_err(|e| format!("can't find the app's exe: {e}"))?;
        let args = HSTRING::from(format!("{WAIT_ARG} {}", std::process::id()));
        let r = unsafe { ShellExecuteW(None, w!("runas"), &HSTRING::from(exe.as_os_str()), &args, None, SW_SHOWNORMAL) };
        // ShellExecute returns a value > 32 on success; UAC "No" gives ERROR_CANCELLED.
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err("Windows didn't grant admin rights".into())
        }
    }

    /// If started by `relaunch_elevated`, wait (max 10 s) for the old instance to exit.
    pub fn wait_for_parent() {
        let args: Vec<String> = std::env::args().collect();
        let Some(pid) = args.iter().position(|a| a == WAIT_ARG).and_then(|i| args.get(i + 1)?.parse::<u32>().ok()) else {
            return;
        };
        if let Ok(h) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
            unsafe {
                WaitForSingleObject(h, 10_000);
                let _ = CloseHandle(h);
            }
        }
    }
}

use settings::Settings;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;
use storage::Storage;
use tauri::{AppHandle, Manager, WindowEvent};

/// Display name and the id used for Credential Manager. Rename here + tauri.conf.json.
pub const APP_NAME: &str = "Yappaflow";
pub const APP_ID: &str = "yappaflow";

pub struct AppState {
    pub db: Storage,
    pub settings: RwLock<Settings>,
    /// One shared client = one pooled, pre-warmed HTTPS/2 connection.
    pub http: reqwest::Client,
    pub recordings_dir: PathBuf,
}

/// A 16:9 window of `base` logical width, shrunk to fit small screens (80% of the monitor at most).
/// Applied once per run, so a window the user resized stays as they left it.
pub(crate) fn fit_16x9(w: &tauri::WebviewWindow, base: f64) {
    static DONE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut done) = DONE.lock() else { return };
    if done.iter().any(|l| l == w.label()) {
        return;
    }
    done.push(w.label().to_string());
    let monitor = w.current_monitor().ok().flatten().or_else(|| w.primary_monitor().ok().flatten());
    let (mw, mh) =
        monitor.map(|m| (m.size().width as f64 / m.scale_factor(), m.size().height as f64 / m.scale_factor())).unwrap_or((1920.0, 1080.0));
    let width = base.min(mw * 0.8).min(mh * 0.8 * 16.0 / 9.0);
    let _ = w.set_size(tauri::LogicalSize::new(width, width * 9.0 / 16.0));
    let _ = w.center();
}

pub(crate) fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.unmaximize();
        fit_16x9(&w, 1152.0);
        let _ = w.set_focus();
    }
}

/// macOS: a toast with Open settings when Accessibility or Input Monitoring is switched off while
/// running (insert.rs already falls back to the clipboard). Only after onboarding, once per revoke.
#[cfg(target_os = "macos")]
fn watch_permissions(app: AppHandle) {
    let _ = std::thread::Builder::new().name("perm-watch".into()).spawn(move || {
        let mut last = context::perms::status();
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let now = context::perms::status();
            let onboarded = app.state::<Arc<AppState>>().settings.read().map(|s| s.onboarded).unwrap_or(false);
            if onboarded {
                if last.accessibility && !now.accessibility {
                    log::warn!("Accessibility was revoked");
                    toast::permission_revoked(&app, "accessibility", "Accessibility");
                } else if last.input_monitoring && !now.input_monitoring {
                    log::warn!("Input Monitoring was revoked");
                    toast::permission_revoked(&app, "input_monitoring", "Input Monitoring");
                } else if last.microphone && !now.microphone {
                    toast::permission_revoked(&app, "microphone", "Microphone access");
                }
            }
            last = now;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // An elevated relaunch waits for the old instance to exit, or single-instance would hand off to it.
    #[cfg(windows)]
    elevate::wait_for_parent();

    let builder = tauri::Builder::default();
    // The pill is an NSPanel on macOS (pill.rs).
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_settings(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            // Menu-bar app: no Dock icon (LSUIElement in Info.plist does the same for bundled builds).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            logging::init(&app.path().app_log_dir()?);
            let data_dir = app.path().app_data_dir()?;
            // One-time move from the pre-release identifier so history, settings and recordings survive.
            // (Local data such as logs, models and the web cache start fresh.)
            if let Some(old) = data_dir.parent().map(|p| p.join("com.yappaflow.app")) {
                if old.is_dir() && !data_dir.exists() {
                    if let Err(e) = std::fs::rename(&old, &data_dir) {
                        log::warn!("couldn't move the old data folder: {e}");
                    }
                }
            }
            let recordings_dir = data_dir.join("recordings");
            std::fs::create_dir_all(&recordings_dir)?;
            // Local data (not roaming): models are gigabytes.
            models::init_dir(app.path().app_local_data_dir()?.join("models"));

            let db = Storage::open(&data_dir.join("yappaflow.db"))?;
            #[allow(unused_mut)]
            let mut settings = db.load_settings();
            #[cfg(windows)]
            if settings.admin_mode && !elevate::is_elevated() {
                match elevate::relaunch_elevated() {
                    Ok(()) => std::process::exit(0),
                    Err(e) => log::warn!("admin mode is on but {e}; running without admin rights"),
                }
                // Saved before standard accounts were refused: switch it off so the toggle tells the truth.
                if elevate::is_standard_user() {
                    settings.admin_mode = false;
                    let _ = db.save_settings(&settings);
                }
            }
            let http = reqwest::Client::builder()
                .user_agent(concat!("Yappaflow/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(5))
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_keepalive(Duration::from_secs(30))
                // Never follow redirects: key headers (x-api-key, xi-api-key, …) must not reach another host.
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            let state = Arc::new(AppState { db, settings: RwLock::new(settings), http, recordings_dir });
            app.manage(state.clone());

            tray::build(app)?;

            // hotkey hook → forwarder → controller
            let (ctl_tx, ctl_rx) = mpsc::channel();
            hotkey_cmds::apply_all(&state.settings.read().unwrap());
            hotkey::start()?;
            let fwd = ctl_tx.clone();
            let fwd_state = state.clone();
            let fwd_app = app.handle().clone();
            std::thread::Builder::new().name("hotkey-fwd".into()).spawn(move || loop {
                match hotkey::next_event() {
                    hotkey::HookEvent::Chord(c) => {
                        if fwd.send(pipeline::CtlMsg::Chord(c)).is_err() {
                            break;
                        }
                    }
                    hotkey::HookEvent::Action(a) => hotkey_cmds::run_action(&fwd_app, &fwd_state, &fwd, a),
                }
            })?;
            app.manage(pill::CtlTx(std::sync::Mutex::new(ctl_tx.clone())));
            pill::create(app.handle())?;
            toast::create(app.handle())?;
            let accent = app.state::<Arc<AppState>>().settings.read().unwrap().accent.clone();
            appicon::apply(app.handle(), &accent);
            models::keep::spawn(state.clone());
            pipeline::spawn(app.handle().clone(), state, ctl_tx, ctl_rx)?;
            models::download::resume_all(app.handle());

            // First run: no key yet → open settings so the user knows what to do.
            // Existing users (a key already saved) skip onboarding.
            if secrets::api_key().is_some() {
                let _ = onboarding::mark_onboarded(&app.state::<Arc<AppState>>());
            }
            if !app.state::<Arc<AppState>>().settings.read().unwrap().onboarded {
                onboarding::show(app.handle())?;
            }
            #[cfg(target_os = "macos")]
            watch_permissions(app.handle().clone());
            log::info!(
                "{APP_NAME} ready, hold {} to dictate",
                hotkey_cmds::dictate_display(&app.state::<Arc<AppState>>().settings.read().unwrap())
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window only hides it; the app lives in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                match window.label() {
                    "main" => {
                        let _ = window.hide();
                        api.prevent_close();
                    }
                    // Closing onboarding with X counts as finishing it.
                    "onboarding" => {
                        let app = window.app_handle();
                        let _ = onboarding::finish(app, &app.state::<Arc<AppState>>());
                        api.prevent_close();
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_options,
            commands::get_settings,
            commands::save_settings,
            commands::api_key_status,
            commands::set_api_key,
            commands::test_api_key,
            commands::set_provider_key,
            commands::provider_key_status,
            commands::test_provider_key,
            commands::check_custom_url,
            commands::list_mics,
            commands::recent_dictations,
            commands::usage_estimate,
            commands::retry_dictation,
            commands::copy_text,
            commands::open_recordings_folder,
            system_cmds::get_autostart,
            system_cmds::set_autostart,
            system_cmds::set_admin_mode,
            system_cmds::is_elevated,
            system_cmds::is_standard_user,
            system_cmds::read_licence,
            system_cmds::open_app_folder,
            system_cmds::permission_status,
            system_cmds::request_permission,
            system_cmds::open_system_settings,
            system_cmds::fn_key_conflict,
            system_cmds::restart_app,
            system_cmds::delete_all_history,
            settings_cmds::list_app_styles,
            settings_cmds::set_app_style,
            settings_cmds::add_app_style,
            settings_cmds::list_terms,
            settings_cmds::add_term,
            settings_cmds::delete_term,
            settings_cmds::list_snippets,
            settings_cmds::save_snippet,
            settings_cmds::delete_snippet,
            learn::get_term_suggestion,
            learn::answer_term_suggestion,
            settings_cmds::read_audio,
            pill::pill_stop,
            settings_cmds::set_hotkey_capture,
            settings_cmds::get_default_cleanup_rules,
            toast::toast_hide,
            toast::toast_fit,
            tray::tray_fit,
            tray::tray_state,
            tray::tray_hide,
            tray::tray_pause,
            tray::tray_paste_again,
            tray::tray_hands_free,
            tray::open_settings,
            tray::quit_app,
            hotkey_cmds::get_hotkey,
            hotkey_cmds::set_hotkey,
            hotkey_cmds::check_hotkey_conflict,
            hotkey_cmds::get_hotkeys,
            hotkey_cmds::set_hotkey_for,
            hotkey_cmds::reset_hotkey,
            hotkey_cmds::check_hotkey_conflict_for,
            hotkey_cmds::set_hands_free_double_tap,
            onboarding::show_onboarding,
            onboarding::finish_onboarding,
            models::local_models,
            models::local_budget,
            commands::retry_dictation_cloud,
            models::download_model,
            models::pause_download,
            models::cancel_download,
            models::delete_model,
            models::open_models_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building app")
        .run(|_, e| {
            if let tauri::RunEvent::Exit = e {
                providers::llama_cpp::shutdown();
            }
        });
}
