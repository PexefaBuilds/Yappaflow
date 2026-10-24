//! STT latency benchmark on saved recordings. Run:
//! cargo test --features evals -- --ignored --nocapture stt_bench

use crate::audio::{resample, wav};
use crate::providers::{registry, AudioClip, SttOptions};
use std::time::Instant;

/// STT_PROVIDER=<id> benchmarks every listed model of that provider; default OpenAI.
fn bench_target(p: Option<&str>) -> &'static registry::ProviderInfo {
    let id = p.unwrap_or("openai");
    registry::STT_PROVIDERS.iter().find(|x| x.id == id).unwrap_or(&registry::STT_PROVIDERS[0])
}

#[test]
fn bench_target_selects_provider() {
    assert_eq!(bench_target(Some("groq")).id, "groq");
    assert_eq!(bench_target(Some("deepgram")).id, "deepgram");
    assert_eq!(bench_target(None).id, "openai");
}

#[test]
#[ignore]
fn stt_bench() {
    let dir = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.yappaflow.desktop/recordings");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    let files: Vec<_> = files.into_iter().rev().take(4).collect();
    let http = reqwest::Client::new();
    let target = bench_target(std::env::var("STT_PROVIDER").ok().as_deref());
    let id = target.id;
    if crate::secrets::provider_key(id).is_none() {
        println!("no stored {id} key; skipping");
        return;
    }
    for &model in target.models {
        let s = crate::settings::Settings { stt_provider: id.into(), stt_model: model.into(), ..Default::default() };
        let stt = registry::build_stt(&s, &http);
        let mut ms = Vec::new();
        for f in &files {
            let bytes = std::fs::read(f).unwrap();
            let (pcm16k, _) = wav::decode(&bytes).unwrap();
            let clip =
                AudioClip { duration_ms: resample::duration_ms(pcm16k.len()), bytes, mime: "audio/wav", filename: "audio.wav", pcm16k };
            for _ in 0..2 {
                let t = Instant::now();
                let r = tauri::async_runtime::block_on(stt.transcribe(&clip, &SttOptions::default()));
                let d = t.elapsed().as_millis();
                if let Ok(tr) = &r {
                    println!("{model} {:>5} ms clip {} ms: {}", d, clip.duration_ms, tr.text.chars().take(50).collect::<String>());
                    ms.push(d);
                } else {
                    println!("{model} error {:?}", r.err());
                }
            }
        }
        ms.sort();
        if !ms.is_empty() {
            println!("== {model}: p50 {} min {} max {}", ms[ms.len() / 2], ms[0], ms[ms.len() - 1]);
        }
    }
}
