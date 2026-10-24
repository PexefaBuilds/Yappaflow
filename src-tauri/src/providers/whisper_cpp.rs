//! Local speech to text: whisper.cpp in-process via `whisper-rs`, fed the 16 kHz PCM
//! already in `AudioClip`. The engine is behind the `local-stt` cargo feature (building it
//! needs CMake + libclang); without it this provider explains how to get it.
//! The loaded model is cached across dictations (providers are rebuilt per dictation).

use super::{vocab, AudioClip, SttOptions, SttProvider, Transcript};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

pub struct WhisperCppStt {
    model_id: String,
    /// "Keep models loaded" = after each use: free the model once a transcript is done.
    unload_after_use: bool,
}

impl WhisperCppStt {
    pub fn new(model_id: &str) -> Self {
        Self { model_id: model_id.to_string(), unload_after_use: false }
    }

    pub fn unload_after_use(mut self, yes: bool) -> Self {
        self.unload_after_use = yes;
        self
    }
}

/// whisper.cpp language code: the first hint, or "auto".
#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn language(opts: &SttOptions) -> String {
    opts.languages
        .first()
        .map(|l| l.split('-').next().unwrap_or(l).to_lowercase())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "auto".into())
}

/// Dictionary terms go into whisper's initial prompt, like Groq's (224-token cap).
#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn initial_prompt(opts: &SttOptions) -> Option<String> {
    let vocab::VocabStrategy::Prompt { max_tokens } = vocab::WHISPER_PROMPT else { return opts.prompt.clone() };
    vocab::fold_into_prompt(opts.prompt.as_deref(), &vocab::WHISPER_PROMPT.apply(&opts.keywords), max_tokens)
}

#[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
pub fn pcm_to_f32(pcm: &[i16]) -> Vec<f32> {
    pcm.iter().map(|&s| s as f32 / 32768.0).collect()
}

#[cfg(feature = "local-stt")]
mod engine {
    use super::*;
    use std::sync::Mutex;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    static MODEL: Mutex<Option<(PathBuf, WhisperContext)>> = Mutex::new(None);

    fn load(path: &PathBuf) -> Result<WhisperContext> {
        let p = path.to_str().ok_or_else(|| anyhow::anyhow!("model path isn't valid UTF-8"))?;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(crate::models::GPU_BUILD);
        match WhisperContext::new_with_params(p, params) {
            Ok(c) => Ok(c),
            // GPU backend failed to init (driver, VRAM): fall back to CPU.
            Err(e) if crate::models::GPU_BUILD => {
                log::warn!("whisper.cpp GPU init failed ({e:?}), using CPU");
                let mut cpu = WhisperContextParameters::default();
                cpu.use_gpu(false);
                WhisperContext::new_with_params(p, cpu).map_err(|e| anyhow::anyhow!("can't load the speech model: {e:?}"))
            }
            Err(e) => Err(anyhow::anyhow!("can't load the speech model: {e:?}")),
        }
    }

    /// Loads `path` if it isn't the cached model. Blocking: call from spawn_blocking.
    pub fn ensure(path: &PathBuf) -> Result<()> {
        let mut m = MODEL.lock().map_err(|_| anyhow::anyhow!("speech model lock poisoned"))?;
        if m.as_ref().map_or(true, |(p, _)| p != path) {
            *m = None; // free the old model first
            *m = Some((path.clone(), load(path)?));
        }
        Ok(())
    }

    pub fn run(path: &PathBuf, samples: Vec<f32>, lang: String, prompt: Option<String>) -> Result<Transcript> {
        ensure(path)?;
        let m = MODEL.lock().map_err(|_| anyhow::anyhow!("speech model lock poisoned"))?;
        let (_, ctx) = m.as_ref().ok_or_else(|| anyhow::anyhow!("speech model not loaded"))?;
        let mut state = ctx.create_state().map_err(|e| anyhow::anyhow!("whisper state: {e:?}"))?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some(&lang));
        if let Some(pr) = prompt.as_deref() {
            p.set_initial_prompt(pr);
        }
        p.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_single_segment(false);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        state.full(p, &samples).map_err(|e| anyhow::anyhow!("whisper.cpp failed: {e:?}"))?;
        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                text.push_str(&seg.to_str_lossy().map_err(|e| anyhow::anyhow!("{e:?}"))?);
            }
        }
        let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
        Ok(Transcript { text: text.trim().to_string(), language })
    }

    pub fn unload() {
        if let Ok(mut m) = MODEL.lock() {
            *m = None;
        }
    }
}

/// Frees the cached model (before deleting its file, or to save memory).
pub fn unload() {
    #[cfg(feature = "local-stt")]
    engine::unload();
}

fn model_path(id: &str) -> Result<PathBuf> {
    crate::models::installed_path(id)
}

#[async_trait]
impl SttProvider for WhisperCppStt {
    fn id(&self) -> &'static str {
        "whisper_cpp"
    }

    async fn prewarm(&self) {
        crate::models::keep::touch();
        #[cfg(feature = "local-stt")]
        if let Ok(p) = model_path(&self.model_id) {
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = engine::ensure(&p) {
                    log::warn!("speech model prewarm failed: {e:#}");
                }
            })
            .await;
        }
    }

    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript> {
        let path = model_path(&self.model_id)?;
        #[cfg(feature = "local-stt")]
        {
            let (samples, lang, prompt) = (pcm_to_f32(&audio.pcm16k), language(opts), initial_prompt(opts));
            let r = tauri::async_runtime::spawn_blocking(move || engine::run(&path, samples, lang, prompt)).await?;
            crate::models::keep::touch();
            if self.unload_after_use {
                engine::unload();
            }
            return r;
        }
        #[cfg(not(feature = "local-stt"))]
        {
            let _ = (path, audio, opts, self.unload_after_use);
            anyhow::bail!("This build of Yappaflow has no local speech engine (built without the local-stt feature). Choose a cloud provider in Settings › Models.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_and_prompt() {
        let mut o = SttOptions::default();
        assert_eq!(language(&o), "auto");
        o.languages = vec!["AR-ae".into()];
        assert_eq!(language(&o), "ar");
        o.keywords = vec!["Yappaflow".into(), "bad<term>".into()];
        let p = initial_prompt(&o).unwrap();
        assert!(p.contains("Yappaflow") && !p.contains('<'), "{p}");
        assert_eq!(pcm_to_f32(&[0, 16384, -32768]), vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn missing_model_is_a_clear_error() {
        let a = AudioClip { bytes: vec![], mime: "audio/wav", filename: "a.wav", pcm16k: vec![0; 160], duration_ms: 10 };
        let e = tauri::async_runtime::block_on(WhisperCppStt::new("nope").transcribe(&a, &SttOptions::default())).unwrap_err();
        assert!(e.to_string().contains("Unknown local model"), "{e}");
    }
}
