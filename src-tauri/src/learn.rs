//! Learn from corrections: for 15 s after a paste, watch the focused text field. When words we
//! inserted are replaced by similar-sounding ones, count it; the second time, offer to add the
//! fixed spelling to the dictionary. Only our inserted words are compared; nothing read from the
//! field is stored or logged.

use crate::snippets::levenshtein;
use crate::toast::{self, Toast, ToastAction};
use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const WATCH: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(500);

/// A newer dictation stops the previous watcher.
static GEN: AtomicU64 = AtomicU64::new(0);
/// Fixed spelling (lower case) → (what we inserted, times corrected).
/// shortcut: in memory only, so counts reset on restart; persist if users miss suggestions.
static COUNTS: Mutex<Option<HashMap<String, (String, u32)>>> = Mutex::new(None);
static PENDING: Mutex<Option<Suggestion>> = Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    pub heard: String,
    pub term: String,
}

pub fn watch(app: AppHandle, inserted: String) {
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = std::thread::Builder::new().name("learn".into()).spawn(move || {
        std::thread::sleep(POLL);
        // Unreadable field (no UIA, password box, elevated app): do nothing.
        let Some(first) = crate::context::focused_text() else { return };
        let mut last = first;
        let mut found = None;
        let t0 = Instant::now();
        while t0.elapsed() < WATCH && GEN.load(Ordering::SeqCst) == gen {
            std::thread::sleep(POLL);
            let Some(now) = crate::context::focused_text() else { continue };
            if now != last {
                // Keep the latest: mid-typing "Kube" is superseded by the finished "Kubernetes".
                if let Some(c) = correction(&inserted, &now) {
                    found = Some(c);
                }
                last = now;
            }
        }
        if let Some(c) = found {
            record(&app, c);
        }
    });
}

fn record(app: &AppHandle, c: Suggestion) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };
    let known = state.db.list_terms().unwrap_or_default();
    if known.iter().any(|t| t.term.eq_ignore_ascii_case(&c.term)) {
        return;
    }
    let count = {
        let mut g = COUNTS.lock().unwrap();
        let e = g.get_or_insert_with(HashMap::new).entry(c.term.to_lowercase()).or_insert((c.heard.clone(), 0));
        e.0 = c.heard.clone();
        e.1 += 1;
        e.1
    };
    if count < 2 {
        return;
    }
    *PENDING.lock().unwrap() = Some(c.clone());
    toast::show(
        app,
        Toast {
            dot: "accent",
            title: format!("Add {} to your dictionary?", c.term),
            body: format!("You corrected \"{}\" to {} twice.", c.heard, c.term),
            action: Some(ToastAction { label: "Add", kind: "add_term", arg: Some(c.term.clone()) }),
            secondary: Some(ToastAction { label: "Not now", kind: "skip_term", arg: Some(c.term.clone()) }),
        },
    );
}

#[tauri::command]
pub fn get_term_suggestion() -> Option<Suggestion> {
    PENDING.lock().unwrap().clone()
}

/// "Add" stores the term as learned, with what we misheard as its "sounds like". Either answer clears it.
#[tauri::command]
pub fn answer_term_suggestion(state: tauri::State<'_, Arc<AppState>>, term: String, add: bool) -> Result<(), String> {
    let s = {
        let mut p = PENDING.lock().unwrap();
        // A newer suggestion replaced the one this answer is for: leave it alone.
        match p.take() {
            Some(s) if s.term == term => s,
            other => {
                *p = other;
                return Ok(());
            }
        }
    };
    if let Some(m) = COUNTS.lock().unwrap().as_mut() {
        m.remove(&s.term.to_lowercase());
    }
    if add {
        use crate::settings_cmds::clean_term;
        state.db.add_term(&clean_term(&s.term, 50), &clean_term(&s.heard, 80), "learned").map_err(|e| format!("{e:#}"))?;
    }
    Ok(())
}

/// Words with surrounding punctuation stripped; `.0` original, `.1` lower-case for matching.
fn words(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(|w| (w.to_string(), w.to_lowercase()))
        .collect()
}

/// Compare what we inserted with the field's current text: a run of 1–3 of our words replaced
/// by 1–3 similar-sounding words (between words that stayed) is a correction.
pub fn correction(inserted: &str, field: &str) -> Option<Suggestion> {
    let a = words(inserted);
    let b = words(field);
    if a.is_empty() || a.len() > 300 || b.len() > 5000 {
        return None;
    }
    // LCS table over words.
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i].1 == b[j].1 { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    // Matched pairs, in order.
    let (mut i, mut j, mut pairs) = (0, 0, Vec::new());
    while i < n && j < m {
        if a[i].1 == b[j].1 {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    // Not enough of our text left: probably a different field. Needs at least one anchor word.
    if pairs.is_empty() || pairs.len() * 2 < n {
        return None;
    }
    // Gaps: before the first anchor, between anchors, after the last. Edge gaps only take the
    // field words right next to the anchor (1-3, nearest first), never the rest of the field.
    let mut gaps = Vec::new();
    let (fi, fj) = pairs[0];
    gaps.extend((1..=3.min(fj)).map(|w| (0..fi, fj - w..fj)));
    for w in pairs.windows(2) {
        gaps.push((w[0].0 + 1..w[1].0, w[0].1 + 1..w[1].1));
    }
    let (li, lj) = *pairs.last().unwrap();
    gaps.extend((1..=3).filter(|w| lj + 1 + w <= m).map(|w| (li + 1..n, lj + 1..lj + 1 + w)));
    for (ga, gb) in gaps {
        if !(1..=3).contains(&ga.len()) || !(1..=3).contains(&gb.len()) {
            continue;
        }
        let heard = a[ga].iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" ");
        let term = b[gb].iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" ");
        if sounds_alike(&heard, &term) {
            return Some(Suggestion { heard, term });
        }
    }
    None
}

/// Different spelling, similar sound: equal consonant skeletons (±1), or a close raw spelling.
fn sounds_alike(a: &str, b: &str) -> bool {
    let flat = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let (fa, fb) = (flat(a), flat(b));
    if fa == fb || fa.is_empty() || fb.is_empty() {
        return false;
    }
    let (ka, kb) = (skeleton(&fa), skeleton(&fb));
    if ka.len() >= 3 && levenshtein(&ka, &kb) <= 1 || ka.len() == 2 && ka == kb {
        return true;
    }
    levenshtein(&fa, &fb) * 3 <= fa.chars().count().max(fb.chars().count())
}

/// Crude phonetic key: similar consonants merged, vowels and h/w/y dropped, repeats collapsed.
fn skeleton(s: &str) -> String {
    let s = s.replace("ph", "f");
    let mut out = String::new();
    for c in s.chars() {
        let k = match c {
            'c' | 'q' | 'k' | 'g' => 'k',
            'b' | 'p' => 'b',
            'd' | 't' => 't',
            's' | 'z' | 'x' => 's',
            'f' | 'v' => 'f',
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w' => continue,
            c if c.is_ascii_alphabetic() => c,
            _ => continue,
        };
        if !out.ends_with(k) {
            out.push(k);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(heard: &str, term: &str) -> Option<Suggestion> {
        Some(Suggestion { heard: heard.into(), term: term.into() })
    }

    #[test]
    fn finds_a_respelled_name() {
        let ins = "We deploy it on Cooper Netties every Friday.";
        let field = "Hi team, We deploy it on Kubernetes every Friday. Thanks";
        assert_eq!(correction(ins, field), s("Cooper Netties", "Kubernetes"));
        assert_eq!(correction("build the tory app", "build the Tauri app"), s("tory", "Tauri"));
    }

    #[test]
    fn edits_at_the_edges() {
        assert_eq!(correction("tory is great", "Tauri is great"), s("tory", "Tauri"));
        assert_eq!(correction("I like tory", "I like Tauri"), s("tory", "Tauri"));
        assert_eq!(correction("tory is great", "Hi team, Tauri is great"), s("tory", "Tauri"));
        assert_eq!(correction("I like tory", "I like Tauri. Thanks"), s("tory", "Tauri"));
    }

    #[test]
    fn ignores_rewrites_and_other_fields() {
        assert_eq!(correction("send it on Monday", "send it on Friday"), None); // not similar
        assert_eq!(correction("send it on Monday", "send it on monday"), None); // case only
        assert_eq!(correction("send the report to Dana today", "completely different text"), None);
        assert_eq!(correction("hello there", "hello there"), None);
    }

    #[test]
    fn skeleton_merges_sounds() {
        assert_eq!(skeleton("coopernetties"), skeleton("kubernetes"));
        assert_eq!(skeleton("tory"), skeleton("tauri"));
    }
}
