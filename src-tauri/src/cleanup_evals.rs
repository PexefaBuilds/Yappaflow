//! Eval harness for docs/evals/cleanup.jsonl (Phase 2 test 2.1). Needs the saved API key.
//! Run: cargo test --features evals -- --ignored --nocapture

use crate::providers::{registry, CleanupContext};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    style: String,
    dictionary: String,
    input: String,
    expected: String,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    before_cursor: String,
}

/// Near-exact match: whitespace, case, quote style and `, . ! ? ; :` are ignored
/// (case counts for cases tagged "exact_case").
fn norm(s: &str, exact_case: bool) -> String {
    let s = if exact_case { s.to_string() } else { s.to_lowercase() };
    s.replace(['’', '‘'], "'").replace([',', '.', '!', '?', ';', ':'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
#[ignore]
fn cleanup_eval_set() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/evals/cleanup.jsonl");
    let cases: Vec<Case> = std::fs::read_to_string(path)
        .expect("read cleanup.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("bad jsonl line"))
        .collect();
    // EVAL_PROVIDER=<id> (groq, anthropic, gemini, mistral, openrouter, custom) uses that
    // provider's saved key and default endpoint; EVAL_MODEL / EVAL_BASE_URL override.
    let id = std::env::var("EVAL_PROVIDER").unwrap_or_else(|_| "openai".into());
    let info = registry::CLEANUP_PROVIDERS.iter().find(|p| p.id == id).expect("unknown EVAL_PROVIDER");
    let mut s = crate::settings::Settings { cleanup_provider: id.clone(), ..Default::default() };
    s.cleanup_model = std::env::var("EVAL_MODEL").unwrap_or_else(|_| info.models.first().copied().unwrap_or_default().into());
    if let Ok(base) = std::env::var("EVAL_BASE_URL") {
        match id.as_str() {
            "openai" => s.openai_base_url = base,
            "groq" => s.groq_base_url = base,
            "custom" => s.custom_base_url = base,
            _ => drop(s.base_urls.insert(id.clone(), base)),
        }
    }
    let p = registry::build_cleanup_with_timeout(&s, &reqwest::Client::new(), std::time::Duration::from_secs(20));

    let mut lat: Vec<u128> = Vec::new();
    let (mut pass, mut must_fail, mut inj_fail) = (0, Vec::new(), Vec::new());
    for c in &cases {
        let ctx = CleanupContext {
            tone: c.style.clone(),
            dictionary: c.dictionary.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
            before_cursor: c.before_cursor.clone(),
            ..Default::default()
        };
        let t0 = std::time::Instant::now();
        let got = tauri::async_runtime::block_on(p.clean(&c.input, &ctx)).unwrap_or_else(|e| format!("<error: {e:#}>"));
        lat.push(t0.elapsed().as_millis());
        let exact = c.tags.iter().any(|t| t == "exact_case");
        let ok = std::iter::once(&c.expected).chain(&c.accept).any(|e| norm(e, exact) == norm(&got, exact));
        println!("{} {:<16} got: {got:?}", if ok { "PASS" } else { "FAIL" }, c.id);
        if ok {
            pass += 1;
        } else {
            if c.tags.iter().any(|t| t == "must_pass") {
                must_fail.push(c.id.clone());
            }
            if c.tags.iter().any(|t| t == "injection") {
                inj_fail.push(c.id.clone());
            }
        }
    }
    let pct = pass as f64 * 100.0 / cases.len() as f64;
    println!("score {pass}/{} = {pct:.1}%  must_pass failures: {must_fail:?}  injection failures: {inj_fail:?}", cases.len());
    lat.sort();
    println!("latency ms: p50 {} p90 {} max {}", lat[lat.len() / 2], lat[lat.len() * 9 / 10], lat[lat.len() - 1]);
    assert!(pct >= 90.0 && must_fail.is_empty() && inj_fail.is_empty());
}
