//! Local cleanup: a `llama-server` sidecar (llama.cpp's OpenAI-compatible server) bound to
//! 127.0.0.1 on a random port with a random per-launch API key, driven by the shared chat
//! client in `openai_cleanup.rs`. Why a sidecar: phase-7.md item 8 (no second ggml in-process,
//! crash/OOM isolation, prebuilt CUDA/Metal releases).
//!
//! Lifecycle: started on demand (prewarm at chord-down, or the first cleanup), health-checked
//! via GET /health, restarted when it has died, killed on app exit or model change. If the app
//! crashes, a Windows Job object (or a watchdog shell on macOS) takes it down too.
//! Bundled by the release workflow as a Tauri `externalBin` once its pinned hash is set
//! (see docs/phases/phase-7.md Results); otherwise the user places it next to the app.

use super::openai_cleanup::{ChatPreset, OpenAiCleanup, EDIT_TIMEOUT};
use super::{CleanupContext, CleanupProvider};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Qwen3.5 thinks by default; this switches it off (docs/providers.md).
pub const QWEN: ChatPreset = ChatPreset {
    id: "llama_cpp",
    max_tokens_field: "max_tokens",
    headroom: 0,
    extra_body: r#"{"chat_template_kwargs":{"enable_thinking":false}}"#,
};
/// Ministral wants temperature < 0.1 (docs/providers.md).
pub const MINISTRAL: ChatPreset =
    ChatPreset { id: "llama_cpp", max_tokens_field: "max_tokens", headroom: 0, extra_body: r#"{"temperature":0.05}"# };

pub fn preset(name: Option<&str>) -> &'static ChatPreset {
    match name {
        Some("ministral") => &MINISTRAL,
        _ => &QWEN,
    }
}

/// Where llama-server.exe is looked for when the setting is empty: next to the app's exe,
/// which is where Tauri puts an `externalBin` sidecar.
pub fn default_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    Some(std::env::current_exe().ok()?.parent()?.join(name))
}

pub fn resolve_binary(setting: &str) -> Result<PathBuf> {
    let p = if setting.trim().is_empty() { default_binary() } else { Some(PathBuf::from(setting.trim())) };
    match p {
        Some(p) if p.is_file() => Ok(p),
        _ => bail!("Local cleanup needs llama-server, which wasn't found. Put llama-server.exe next to Yappaflow.exe or set its path in Settings › Models › On this PC."),
    }
}

/// The command line we launch with. `--jinja` makes the chat template honour `chat_template_kwargs`.
/// `cpu` keeps every layer off the GPU (the VRAM budget check said it doesn't fit).
pub fn args(model: &Path, port: u16, key: &str, cpu: bool) -> Vec<String> {
    [
        "-m",
        &model.to_string_lossy(),
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--api-key",
        key,
        "--jinja",
        "-c",
        "4096",
        "-ngl",
        if cpu { "0" } else { "999" },
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// 128 bits from two OS-seeded SipHash keys. shortcut: not a CSPRNG, fine for a loopback-only
/// per-launch key; use getrandom if the server ever listens beyond 127.0.0.1.
pub fn random_key() -> String {
    use std::hash::BuildHasher;
    (0..2).map(|i| format!("{:016x}", std::collections::hash_map::RandomState::new().hash_one((i, Instant::now())))).collect()
}

/// Talks only to our own 127.0.0.1 server: never through a system proxy (reqwest's
/// system-proxy support doesn't promise to exempt loopback), never follows redirects.
fn local_client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap_or_default())
}

fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Kills llama-server when our process dies, even on a crash.
/// Exception to the "OS code lives in four places" rule (CLAUDE.md), like hardware.rs: the
/// sidecar's crash cleanup is OS-specific and belongs with the sidecar.
#[cfg(windows)]
fn tie_to_app(child: &Child) -> Option<Child> {
    use std::os::windows::io::AsRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // One job for the app's lifetime. Its handle is never closed, so the OS closes it when we
    // exit or crash, and KILL_ON_JOB_CLOSE then ends every process in it.
    static JOB: OnceLock<isize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let Ok(job) = CreateJobObjectW(None, PCWSTR::null()) else { return 0 };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        match SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const std::ffi::c_void, size) {
            Ok(()) => job.0 as isize,
            Err(_) => 0,
        }
    });
    if job == 0 || unsafe { AssignProcessToJobObject(HANDLE(job as _), HANDLE(child.as_raw_handle() as _)) }.is_err() {
        log::warn!("couldn't tie llama-server to the app; it may outlive a crash");
    }
    None
}

/// llama-server doesn't exit on stdin EOF, so a tiny watchdog shell does: it blocks reading a pipe
/// whose write end only we hold (kept open in the returned Child's stdin). The OS closes it when we
/// exit or crash, `read` returns, and the server is killed. Polling our PID instead could be fooled
/// by PID reuse. The server's PID can't be reused while the watchdog runs: every path that reaps
/// the server drops its Sidecar, and Sidecar::drop kills the watchdog first.
#[cfg(unix)]
fn tie_to_app(child: &Child) -> Option<Child> {
    let script = format!("read _; kill {} 2>/dev/null", child.id());
    let w = Command::new("/bin/sh").args(["-c", &script]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if w.is_err() {
        log::warn!("couldn't start the llama-server watchdog; it may outlive a crash");
    }
    w.ok()
}

#[cfg(not(any(windows, unix)))]
fn tie_to_app(_: &Child) -> Option<Child> {
    None
}

struct Sidecar {
    child: Child,
    watchdog: Option<Child>,
    model: PathBuf,
    cpu: bool,
    port: u16,
    key: String,
    ready: bool,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(w) = self.watchdog.as_mut() {
            let _ = w.kill();
            let _ = w.wait();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static SIDECAR: Mutex<Option<Sidecar>> = Mutex::new(None);

/// Starts llama-server for `model` unless it's already running with it.
fn start(bin: &Path, model: &Path, cpu: bool) -> Result<()> {
    crate::models::keep::touch();
    let mut g = SIDECAR.lock().map_err(|_| anyhow!("sidecar lock poisoned"))?;
    if let Some(s) = g.as_mut() {
        let alive = matches!(s.child.try_wait(), Ok(None));
        if alive && s.model == model && s.cpu == cpu {
            return Ok(());
        }
        if !alive {
            log::warn!("llama-server exited, restarting");
        }
    }
    *g = None; // kills the old one
    let (port, key) = (free_port()?, random_key());
    let mut cmd = Command::new(bin);
    cmd.args(args(model, port, &key, cpu)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // Process creation flag only (no console window); not OS integration, so it stays here.
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().with_context(|| format!("can't start {}", bin.display()))?;
    let watchdog = tie_to_app(&child);
    if let Some(err) = child.stderr.take() {
        // llama-server logs to stderr. Our key is on its command line, not in its log output.
        let _ = std::thread::Builder::new().name("llama-server-log".into()).spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                let line: String = line.chars().take(300).collect();
                if line.to_lowercase().contains("error") {
                    log::warn!("llama-server: {line}");
                } else {
                    log::debug!("llama-server: {line}");
                }
            }
        });
    }
    log::info!("llama-server started on 127.0.0.1:{port}{}", if cpu { " (CPU only: models don't fit in graphics memory)" } else { "" });
    *g = Some(Sidecar { child, watchdog, model: model.to_path_buf(), cpu, port, key, ready: false });
    Ok(())
}

enum Wait {
    Ready(String, String),
    /// The process exited before it was ready (e.g. the port got taken).
    Exited,
}

/// Waits until /health is OK (model loaded) or `deadline`.
async fn wait_ready(http: &reqwest::Client, deadline: Instant) -> Result<Wait> {
    loop {
        let (port, key, ready) = {
            let mut g = SIDECAR.lock().map_err(|_| anyhow!("sidecar lock poisoned"))?;
            let s = g.as_mut().ok_or_else(|| anyhow!("llama-server isn't running"))?;
            if !matches!(s.child.try_wait(), Ok(None)) {
                *g = None;
                log::warn!("llama-server exited");
                return Ok(Wait::Exited);
            }
            (s.port, s.key.clone(), s.ready)
        };
        let base = format!("http://127.0.0.1:{port}/v1");
        if ready {
            return Ok(Wait::Ready(base, key));
        }
        let ok = http
            .get(format!("http://127.0.0.1:{port}/health"))
            .timeout(Duration::from_millis(300))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        if ok {
            if let Ok(mut g) = SIDECAR.lock() {
                if let Some(s) = g.as_mut().filter(|s| s.port == port) {
                    s.ready = true;
                }
            }
            return Ok(Wait::Ready(base, key));
        }
        if Instant::now() >= deadline {
            bail!("local cleanup model is still loading");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Kills the sidecar (app exit, model deleted).
pub fn shutdown() {
    if let Ok(mut g) = SIDECAR.lock() {
        *g = None;
    }
}

pub struct LlamaCppCleanup {
    binary_setting: String,
    model_id: String,
    timeout: Duration,
    /// The VRAM budget check failed: run every layer on the CPU.
    cpu: bool,
    /// "Keep models loaded" = after each use: stop the server once a request is done.
    unload_after_use: bool,
}

impl LlamaCppCleanup {
    pub fn from_settings(s: &crate::settings::Settings, timeout: Duration) -> Self {
        Self {
            binary_setting: s.llama_server_path.clone(),
            model_id: s.cleanup_model.clone(),
            timeout,
            cpu: crate::models::budget_for_settings(s).is_some_and(|b| b.fit != "ok"),
            unload_after_use: s.keep_models_loaded == "after_use",
        }
    }

    /// Start-up warm-up: start the server and wait (up to `max`) until the model is loaded.
    pub async fn wait_loaded(&self, max: Duration) -> Result<()> {
        self.client(Instant::now() + max, max).await.map(|_| ())
    }

    fn done(&self) {
        crate::models::keep::touch();
        if self.unload_after_use {
            shutdown();
        }
    }

    /// Starts the server if needed and waits until `deadline`; one retry (new port) if it exits early.
    async fn client(&self, deadline: Instant, timeout: Duration) -> Result<OpenAiCleanup> {
        let model = crate::models::installed_path(&self.model_id)?;
        let bin = resolve_binary(&self.binary_setting)?;
        for _ in 0..2 {
            start(&bin, &model, self.cpu)?;
            if let Wait::Ready(base, key) = wait_ready(local_client(), deadline).await? {
                let p = preset(crate::models::find(&self.model_id).and_then(|m| m.chat_preset.as_deref()));
                return Ok(OpenAiCleanup::new(p, local_client().clone(), &base, &self.model_id, timeout).with_local_key(key));
            }
        }
        bail!("llama-server keeps stopping; check the log")
    }
}

#[async_trait]
impl CleanupProvider for LlamaCppCleanup {
    fn id(&self) -> &'static str {
        "llama_cpp"
    }

    /// Chord-down: start the server (loads the model) while the user talks; don't wait.
    async fn prewarm(&self) {
        let r = crate::models::installed_path(&self.model_id).and_then(|m| start(&resolve_binary(&self.binary_setting)?, &m, self.cpu));
        if let Err(e) = r {
            log::warn!("local cleanup prewarm: {e:#}");
        }
    }

    /// One budget for start-up wait + request; on timeout the pipeline pastes the raw text.
    async fn clean(&self, text: &str, ctx: &CleanupContext) -> Result<String> {
        let t = self.timeout;
        let work = async { self.client(Instant::now() + t, t).await?.clean(text, ctx).await };
        let r = tokio::time::timeout(t, work).await;
        self.done();
        r.map_err(|_| anyhow!("local cleanup timed out"))?
    }

    async fn edit(&self, selection: Option<&str>, instruction: &str, ctx: &CleanupContext) -> Result<String> {
        let t = self.timeout.max(EDIT_TIMEOUT);
        let work = async { self.client(Instant::now() + t, t).await?.edit(selection, instruction, ctx).await };
        let r = tokio::time::timeout(t, work).await;
        self.done();
        r.map_err(|_| anyhow!("local command edit timed out"))?
    }
}

#[cfg(test)]
mod tests {
    use super::super::openai_cleanup::body;
    use super::*;
    use serde_json::json;

    #[test]
    fn presets_switch_thinking_off_and_set_temperature() {
        let b = body(preset(Some("qwen")), "m", "s".into(), "u".into(), 100);
        assert_eq!(b["chat_template_kwargs"], json!({ "enable_thinking": false }));
        assert_eq!(b["max_tokens"], json!(100));
        let b = body(preset(Some("ministral")), "m", "s".into(), "u".into(), 100);
        assert_eq!(b["temperature"], json!(0.05));
        assert!(b.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn launch_args_bind_loopback_with_key() {
        let a = args(Path::new("C:/m/q.gguf"), 4321, "k", false);
        let at = |a: &[String], f: &str| a[a.iter().position(|x| x == f).unwrap() + 1].clone();
        assert_eq!(
            (at(&a, "--host"), at(&a, "--port"), at(&a, "--api-key"), at(&a, "-m")),
            ("127.0.0.1".into(), "4321".into(), "k".into(), "C:/m/q.gguf".into())
        );
        assert_eq!((at(&a, "-ngl"), at(&args(Path::new("m"), 1, "k", true), "-ngl")), ("999".into(), "0".into()));
        let (k1, k2) = (random_key(), random_key());
        assert!(k1.len() == 32 && k1 != k2);
    }

    #[test]
    fn missing_binary_is_a_clear_error() {
        assert!(resolve_binary("Z:/nope/llama-server.exe").unwrap_err().to_string().contains("llama-server"));
    }

    #[cfg(unix)]
    #[test]
    fn watchdog_kills_the_server_when_the_app_goes() {
        let mut server = Command::new("sleep").arg("30").spawn().unwrap();
        let mut w = tie_to_app(&server).unwrap();
        drop(w.stdin.take()); // what the OS does when we exit or crash
        let t = Instant::now();
        while server.try_wait().unwrap().is_none() {
            assert!(t.elapsed() < Duration::from_secs(5), "server outlived the app");
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = w.wait();
    }
}
