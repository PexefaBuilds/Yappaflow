//! Local model catalogue (resources/models.json), where models live on disk, which ones
//! fit this PC, and the Settings › Models › On this PC commands.

pub mod download;
pub mod keep;

use crate::hardware::{self, Hardware};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// stt | cleanup
    pub kind: String,
    /// whisper_cpp | llama_cpp
    pub engine: String,
    pub display_name: String,
    pub file: String,
    pub url: String,
    /// The downloader refuses entries without one.
    pub sha256: Option<String>,
    pub size_bytes: u64,
    pub licence: String,
    pub min_vram_mb: u64,
    pub min_ram_mb: u64,
    /// Cleanup only: which llama-server request patch to use (qwen | ministral).
    #[serde(default)]
    pub chat_preset: Option<String>,
    pub notes: String,
    /// Rough [GPU, CPU] ms for 10 s of speech or one cleanup (see `_ref_ms` in models.json).
    #[serde(default)]
    pub ref_ms: Option<[u64; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub needs_gpu_build: bool,
    pub min_vram_mb: Option<u64>,
    pub min_ram_mb: Option<u64>,
    pub stt: String,
    pub cleanup: Option<String>,
    pub runs_on: String,
}

#[derive(Debug, Deserialize)]
pub struct Catalogue {
    pub models: Vec<Model>,
    pub recommend: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Recommendation {
    pub stt: String,
    pub cleanup: Option<String>,
    pub runs_on: String,
}

pub fn catalogue() -> &'static Catalogue {
    static C: OnceLock<Catalogue> = OnceLock::new();
    C.get_or_init(|| {
        serde_json::from_str(include_str!("../../resources/models.json")).expect("resources/models.json is valid (unit-tested)")
    })
}

pub fn find(id: &str) -> Option<&'static Model> {
    catalogue().models.iter().find(|m| m.id == id)
}

/// True when whisper.cpp in this build has a GPU backend (CUDA on Windows, Metal on macOS).
pub const GPU_BUILD: bool = cfg!(any(feature = "cuda", feature = "metal"));

/// Result of the VRAM budget check (phase-7.md item 12).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Budget {
    /// ok | cleanup_cpu (speech fits, cleanup must run on the CPU) | too_big
    pub fit: &'static str,
    pub estimate_mb: u64,
    pub available_mb: u64,
    /// Empty when `fit` is ok.
    pub message: String,
}

/// Graphics memory the models may use, or None when nothing would run on a GPU (no check).
/// `YAPPAFLOW_VRAM_MB` caps it for testing (acceptance test 7.8).
pub fn gpu_memory_mb(hw: &Hardware) -> Option<u64> {
    let mb = match hw.gpu_vendor.as_str() {
        "nvidia" if hw.vram_mb > 0 => hw.vram_mb,
        // shortcut: Apple GPUs share RAM and macOS lets Metal use roughly 2/3 of it; read
        // MTLDevice.recommendedMaxWorkingSetSize if this proves too loose.
        "apple" => hw.ram_mb * 2 / 3,
        _ => return None,
    };
    let cap = std::env::var("YAPPAFLOW_VRAM_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    Some(mb.min(cap))
}

/// File sizes × 1.1 + 0.9 GB for context and compute buffers (calibrate on the 2070 Super).
fn estimate_mb(models: &[&Model]) -> u64 {
    if models.is_empty() {
        return 0;
    }
    models.iter().map(|m| m.size_bytes * 11 / 10 / (1024 * 1024)).sum::<u64>() + 900
}

/// Do these models fit in `gpu_mb` minus 0.5 GB headroom? Pass `stt` only when whisper runs on
/// the GPU (a GPU build); llama-server offloads to whatever GPU its own build supports.
pub fn budget(stt: Option<&Model>, cleanup: Option<&Model>, gpu_mb: u64) -> Budget {
    let available_mb = gpu_mb.saturating_sub(512);
    let all: Vec<&Model> = stt.into_iter().chain(cleanup).collect();
    let est = estimate_mb(&all);
    let gb = |mb: u64| format!("{:.1} GB", mb as f64 / 1024.0);
    let names = all.iter().map(|m| m.display_name.as_str()).collect::<Vec<_>>().join(" + ");
    let need = format!("{names} need about {} of graphics memory; about {} is free.", gb(est), gb(available_mb));
    let (fit, message) = if est <= available_mb {
        ("ok", String::new())
    } else if cleanup.is_some() && estimate_mb(&stt.into_iter().collect::<Vec<_>>()) <= available_mb {
        ("cleanup_cpu", format!("{need} Cleanup will run on the CPU (slower), or pick a smaller cleanup model."))
    } else {
        ("too_big", format!("{need} Pick a smaller model."))
    };
    Budget { fit, estimate_mb: est, available_mb, message }
}

/// Budget for a speech/cleanup pair (an empty or unknown id = not on the GPU). None = no GPU to check.
pub fn budget_for(stt_id: &str, cleanup_id: &str) -> Option<Budget> {
    let gpu = gpu_memory_mb(hw())?;
    let stt = find(stt_id).filter(|m| m.kind == "stt" && GPU_BUILD);
    let cleanup = find(cleanup_id).filter(|m| m.kind == "cleanup");
    Some(budget(stt, cleanup, gpu))
}

/// The budget for the models Settings has selected now.
pub fn budget_for_settings(s: &crate::settings::Settings) -> Option<Budget> {
    let stt = if s.stt_provider == "whisper_cpp" { s.stt_model.as_str() } else { "" };
    let cleanup = if s.cleanup_provider == "llama_cpp" { s.cleanup_model.as_str() } else { "" };
    budget_for(stt, cleanup)
}

/// First matching row of the catalogue's table (phase-7.md item 11).
pub fn recommend(rules: &[Rule], hw: &Hardware, gpu_build: bool) -> Option<Recommendation> {
    rules
        .iter()
        .find(|r| {
            r.gpu_vendor.as_ref().map_or(true, |v| *v == hw.gpu_vendor)
                && (!r.needs_gpu_build || gpu_build)
                && r.min_vram_mb.map_or(true, |m| hw.vram_mb >= m)
                && r.min_ram_mb.map_or(true, |m| hw.ram_mb >= m)
        })
        .map(|r| Recommendation { stt: r.stt.clone(), cleanup: r.cleanup.clone(), runs_on: r.runs_on.clone() })
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the app's local data dir (never the project folder).
pub fn init_dir(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = DIR.set(dir);
}

pub fn dir() -> Option<&'static PathBuf> {
    DIR.get()
}

pub fn path_of(m: &Model) -> Option<PathBuf> {
    dir().map(|d| d.join(&m.file))
}

/// A finished download: the file exists with the catalogue size (sha256 was checked before the rename).
pub fn installed(m: &Model) -> bool {
    path_of(m).and_then(|p| p.metadata().ok()).is_some_and(|md| md.len() == m.size_bytes)
}

/// Path of an installed model by id, or a message the error toast can show.
pub fn installed_path(id: &str) -> anyhow::Result<PathBuf> {
    let m = find(id).ok_or_else(|| anyhow::anyhow!("Unknown local model '{id}'. Choose one in Settings › Models › On this PC."))?;
    if installed(m) {
        return path_of(m).ok_or_else(|| anyhow::anyhow!("models folder not set"));
    }
    match download::progress_pct(m) {
        Some(p) => anyhow::bail!("Local model still downloading · {p}%"),
        None => anyhow::bail!("{} isn't downloaded yet. Download it in Settings › Models › On this PC.", m.display_name),
    }
}

#[derive(Serialize)]
pub struct ModelView {
    #[serde(flatten)]
    pub model: &'static Model,
    /// none | downloading | paused | installed
    pub state: &'static str,
    pub done_bytes: u64,
    /// Rough [low, high] ms on this PC, from `ref_ms`.
    pub est_ms: Option<[u64; 2]>,
    /// This user's measured average ms, once there are enough dictations.
    pub avg_ms: Option<u64>,
    pub avg_n: i64,
}

#[derive(Serialize)]
pub struct LocalInfo {
    pub hardware: Hardware,
    pub recommended: Option<Recommendation>,
    pub models: Vec<ModelView>,
    pub dir: String,
    pub gpu_build: bool,
    /// whisper.cpp is compiled into this build (cargo feature local-stt).
    pub stt_engine: bool,
}

fn hw() -> &'static Hardware {
    static HW: OnceLock<Hardware> = OnceLock::new();
    HW.get_or_init(hardware::detect)
}

/// Rough [low, high] ms for `m` on this PC. CPU time scales with core count from the 8-core reference.
/// shortcut: assumes llama-server's build can use the GPU; the measured average corrects it after a few dictations.
pub fn estimate_ms(m: &Model, hw: &Hardware, gpu_build: bool) -> Option<[u64; 2]> {
    let [gpu, cpu] = m.ref_ms?;
    let on_gpu = (m.kind == "cleanup" || gpu_build) && gpu_memory_mb(hw).is_some_and(|mb| mb >= m.min_vram_mb + 512);
    let ms = if on_gpu { gpu } else { cpu * 8 / hw.cores.clamp(2, 32) as u64 };
    Some([ms * 2 / 3, ms * 3 / 2])
}

/// Dictations a model needs before its measured average replaces the estimate.
const MIN_MEASURED: i64 = 3;

#[tauri::command]
pub fn local_models(state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> LocalInfo {
    let month_ago =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) - 30 * 86_400_000;
    let speeds = state.db.local_speeds(month_ago).unwrap_or_else(|e| {
        log::warn!("local model speeds: {e:#}");
        Vec::new()
    });
    let models = catalogue()
        .models
        .iter()
        .map(|m| {
            let (state, done_bytes) = if installed(m) {
                ("installed", m.size_bytes)
            } else if download::is_active(&m.id) {
                ("downloading", download::part_len(m))
            } else if download::has_part(m) {
                ("paused", download::part_len(m))
            } else {
                ("none", 0)
            };
            let (avg_n, avg_ms) = speeds.iter().find(|s| s.0 == m.id).map_or((0, None), |s| (s.1, (s.1 >= MIN_MEASURED).then_some(s.2)));
            ModelView { model: m, state, done_bytes, est_ms: estimate_ms(m, hw(), GPU_BUILD), avg_ms, avg_n }
        })
        .collect();
    LocalInfo {
        hardware: hw().clone(),
        recommended: recommend(&catalogue().recommend, hw(), GPU_BUILD),
        models,
        dir: dir().map(|d| d.display().to_string()).unwrap_or_default(),
        gpu_build: GPU_BUILD,
        stt_engine: cfg!(feature = "local-stt"),
    }
}

/// Checked by the UI before "Use": would this pair fit in graphics memory?
#[tauri::command]
pub fn local_budget(stt: String, cleanup: String) -> Option<Budget> {
    budget_for(&stt, &cleanup)
}

#[tauri::command]
pub fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    let m = find(&id).ok_or("Unknown model")?;
    download::start(app, m)
}

#[tauri::command]
pub fn pause_download(id: String) {
    download::stop(&id, download::PAUSE);
}

#[tauri::command]
pub fn cancel_download(id: String) {
    let Some(m) = find(&id) else { return };
    if !download::stop(&id, download::CANCEL) {
        download::remove_part(m); // paused: nothing running, just drop the partial file
    }
}

#[tauri::command]
pub fn delete_model(id: String) -> Result<(), String> {
    let m = find(&id).ok_or("Unknown model")?;
    if download::is_active(&id) {
        return Err("Cancel the download first.".into());
    }
    if m.engine == "llama_cpp" {
        crate::providers::llama_cpp::shutdown(); // the sidecar holds the file open
    }
    crate::providers::whisper_cpp::unload();
    download::remove_part(m);
    match path_of(m) {
        Some(p) if p.exists() => std::fs::remove_file(&p).map_err(|e| format!("Couldn't delete {}: {e}", m.file)),
        _ => Ok(()),
    }
}

#[tauri::command]
pub fn open_models_folder() -> Result<(), String> {
    let d = dir().ok_or("models folder not set")?;
    open_folder(d)
}

fn open_folder(d: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd).arg(d).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(vendor: &str, vram_gb: u64, ram_gb: u64) -> Hardware {
        Hardware { gpu_vendor: vendor.into(), vram_mb: vram_gb * 1024, ram_mb: ram_gb * 1024, cores: 8, ..Default::default() }
    }

    #[test]
    fn catalogue_is_complete_and_pinned() {
        let c = catalogue();
        assert_eq!(c.models.len(), 9);
        for m in &c.models {
            assert!(m.url.starts_with("https://huggingface.co/") && !m.url.contains("/resolve/main/"), "{}", m.id);
            assert!(m.url.ends_with(&format!("/{}", m.file)), "{}", m.id);
            // `file` is joined onto the models dir: a bare file name only.
            assert!(!m.file.contains(['/', '\\']) && !m.file.contains(".."), "{}", m.id);
            let sha = m.sha256.as_deref().unwrap_or("");
            assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{}", m.id);
            assert!(m.size_bytes > 10_000_000, "{}", m.id);
            assert!(matches!((m.kind.as_str(), m.engine.as_str()), ("stt", "whisper_cpp") | ("cleanup", "llama_cpp")), "{}", m.id);
            if m.kind == "cleanup" {
                assert!(matches!(m.chat_preset.as_deref(), Some("qwen" | "ministral")), "{}", m.id);
            }
        }
        for r in &c.recommend {
            assert!(find(&r.stt).is_some_and(|m| m.kind == "stt"), "{}", r.stt);
            assert!(r.cleanup.as_deref().map_or(true, |id| find(id).is_some_and(|m| m.kind == "cleanup")));
        }
        // The last row matches anything, so there is always a recommendation.
        assert!(recommend(&c.recommend, &Hardware::default(), false).is_some());
    }

    #[test]
    fn vram_budget() {
        let (turbo, large, qwen4, qwen08) =
            (find("whisper-large-v3-turbo-q5_0"), find("whisper-large-v3-q5_0"), find("qwen3.5-4b-q4_k_m"), find("qwen3.5-0.8b-q4_k_m"));
        // turbo + Qwen 4B ≈ 4.5 GB (providers.md): fits an 8 GB card.
        let b = budget(turbo, qwen4, 8192);
        assert_eq!(b.fit, "ok");
        assert!((4000..5200).contains(&b.estimate_mb), "{b:?}");
        // Test 7.8: capped at 4 GB, large-v3 + Qwen 4B is blocked and cleanup can go to the CPU.
        let b = budget(large, qwen4, 4096);
        assert_eq!(b.fit, "cleanup_cpu");
        assert!(b.message.contains("CPU") && b.message.contains("large-v3"), "{}", b.message);
        assert_eq!(budget(large, qwen08, 2048).fit, "too_big");
        assert_eq!(budget(None, None, 0).fit, "ok");
        assert_eq!(gpu_memory_mb(&hw("amd", 16, 16)), None);
        assert_eq!(gpu_memory_mb(&hw("apple", 0, 18)), Some(12 * 1024));
    }

    #[test]
    fn recommendation_table() {
        let r = |h: Hardware, gpu: bool| {
            let x = recommend(&catalogue().recommend, &h, gpu).unwrap();
            (x.stt, x.cleanup.unwrap_or_default(), x.runs_on)
        };
        let s = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
        assert_eq!(r(hw("nvidia", 8, 32), true), s("whisper-large-v3-turbo-q5_0", "qwen3.5-4b-q4_k_m", "GPU"));
        assert_eq!(r(hw("nvidia", 6, 16), true), s("whisper-large-v3-turbo-q5_0", "qwen3.5-4b-q4_k_m", "GPU"));
        assert_eq!(r(hw("nvidia", 4, 16), true), s("whisper-large-v3-turbo-q5_0", "ministral-3-3b-instruct-2512-q4_k_m", "GPU"));
        assert_eq!(r(hw("apple", 0, 16), true), s("whisper-large-v3-turbo-q5_0", "qwen3.5-4b-q4_k_m", "Metal"));
        // No GPU backend in this build: an NVIDIA card doesn't help.
        assert_eq!(r(hw("nvidia", 8, 32), false), s("whisper-small-q5_1", "qwen3.5-0.8b-q4_k_m", "CPU"));
        assert_eq!(r(hw("nvidia", 2, 16), true), s("whisper-small-q5_1", "qwen3.5-0.8b-q4_k_m", "CPU"));
        assert_eq!(r(hw("amd", 16, 16), true), s("whisper-small-q5_1", "qwen3.5-0.8b-q4_k_m", "CPU"));
        assert_eq!(r(hw("intel", 0, 8), false), s("whisper-small-q5_1", "qwen3.5-0.8b-q4_k_m", "CPU"));
        assert_eq!(r(hw("", 0, 4), false), s("whisper-base-q5_1", "", "CPU"));
        // An "8 GB" PC reports a little less than 8192 MB.
        let mut h = hw("", 0, 0);
        h.ram_mb = 7800;
        assert_eq!(r(h, false), s("whisper-small-q5_1", "qwen3.5-0.8b-q4_k_m", "CPU"));
    }

    #[test]
    fn estimates_follow_gpu_build_and_cores() {
        assert!(catalogue().models.iter().all(|m| m.ref_ms.is_some_and(|[g, c]| 0 < g && g < c)), "every model has [GPU, CPU] ms");
        let turbo = find("whisper-large-v3-turbo-q5_0").unwrap();
        let qwen = find("qwen3.5-4b-q4_k_m").unwrap();
        let gpu = hw("nvidia", 8, 32);
        assert_eq!(estimate_ms(turbo, &gpu, true), Some([266, 600]));
        assert_eq!(estimate_ms(turbo, &gpu, false), Some([2666, 6000]), "a CPU build runs whisper on the CPU");
        assert_eq!(estimate_ms(qwen, &gpu, false), Some([666, 1500]), "llama-server still uses the GPU");
        let laptop = Hardware { cores: 4, ..hw("", 0, 16) };
        assert_eq!(estimate_ms(qwen, &laptop, false), Some([8000, 18000]), "half the cores, twice the time");
    }
}
