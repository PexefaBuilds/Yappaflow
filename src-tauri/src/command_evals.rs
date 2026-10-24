//! Eval harness for docs/evals/command.jsonl (Phase 5 test 5.5). Needs the saved API key; costs cents.
//! Run: cargo test --features evals command_eval -- --ignored --nocapture
//! Rewrites have no single right answer, so each case checks properties (see docs/prompts/command.md).

use crate::providers::{registry, CleanupContext};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    instruction: String,
    selection: Option<String>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    not_contains: Vec<String>,
    #[serde(default)]
    arabic: bool,
    min_lines: Option<usize>,
    max_lines: Option<usize>,
    min_ratio: Option<f64>,
    max_ratio: Option<f64>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Every failed check, empty = pass.
fn check(c: &Case, got: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let low = got.to_lowercase();
    if got.trim().is_empty() {
        bad.push("empty".into());
    }
    if got.contains("```") {
        bad.push("fence".into());
    }
    bad.extend(c.contains.iter().filter(|s| !low.contains(&s.to_lowercase())).map(|s| format!("missing {s:?}")));
    bad.extend(c.not_contains.iter().filter(|s| low.contains(&s.to_lowercase())).map(|s| format!("has {s:?}")));
    if c.arabic {
        let letters: Vec<char> = got.chars().filter(|ch| ch.is_alphabetic()).collect();
        let ar = letters.iter().filter(|ch| ('\u{0600}'..='\u{06FF}').contains(*ch)).count();
        if letters.is_empty() || ar * 10 < letters.len() * 8 {
            bad.push("not Arabic".into());
        }
    }
    let lines = got.lines().filter(|l| !l.trim().is_empty()).count();
    if c.min_lines.is_some_and(|n| lines < n) || c.max_lines.is_some_and(|n| lines > n) {
        bad.push(format!("{lines} lines"));
    }
    if let Some(sel) = &c.selection {
        let r = got.chars().count() as f64 / sel.chars().count().max(1) as f64;
        if c.min_ratio.is_some_and(|m| r < m) || c.max_ratio.is_some_and(|m| r > m) {
            bad.push(format!("length ratio {r:.2}"));
        }
    }
    bad
}

fn cases() -> Vec<Case> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/evals/command.jsonl");
    std::fs::read_to_string(path)
        .expect("read command.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("bad jsonl line"))
        .collect()
}

/// Offline: the file parses, has 15 cases, and the checker accepts a good answer / rejects a bad one.
#[test]
fn command_eval_file_is_valid() {
    let cs = cases();
    assert_eq!(cs.len(), 15);
    let c = cs.iter().find(|c| c.id == "code-comments").unwrap();
    assert!(check(c, "def area(r):\n    # circle area\n    return 3.14159 * r * r").is_empty());
    assert!(!check(c, "```python\ndef area(r):\n    return 3.14159 * r * r\n```").is_empty());
}

#[test]
#[ignore]
fn command_eval_set() {
    let cases = cases();
    // EVAL_PROVIDER=<cleanup provider id> (default openai); EVAL_MODEL / EVAL_BASE_URL override.
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
    // edit() already lifts the cap to the 30 s edit timeout.
    let p = registry::build_cleanup(&s, &reqwest::Client::new());
    let ctx = CleanupContext { app_exe: "outlook.exe".into(), window_title: "New message".into(), ..Default::default() };

    let (mut pass, mut must_fail, mut inj_fail, mut lat) = (0, Vec::new(), Vec::new(), Vec::new());
    for c in &cases {
        let t0 = std::time::Instant::now();
        let got = tauri::async_runtime::block_on(p.edit(c.selection.as_deref(), &c.instruction, &ctx))
            .unwrap_or_else(|e| format!("<error: {e:#}>"));
        lat.push(t0.elapsed().as_millis());
        let bad = check(c, &got);
        println!("{} {:<30} {bad:?} got: {got:?}", if bad.is_empty() { "PASS" } else { "FAIL" }, c.id);
        if bad.is_empty() {
            pass += 1;
            continue;
        }
        if c.tags.iter().any(|t| t == "must_pass") {
            must_fail.push(c.id.clone());
        }
        if c.tags.iter().any(|t| t == "injection") {
            inj_fail.push(c.id.clone());
        }
    }
    let pct = pass as f64 * 100.0 / cases.len() as f64;
    println!("score {pass}/{} = {pct:.1}%  must_pass failures: {must_fail:?}  injection failures: {inj_fail:?}", cases.len());
    lat.sort();
    println!("latency ms: p50 {} p90 {} max {}", lat[lat.len() / 2], lat[lat.len() * 9 / 10], lat[lat.len() - 1]);
    assert!(pct >= 90.0 && must_fail.is_empty() && inj_fail.is_empty());
}
