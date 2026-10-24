//! Vocabulary hints as data: each STT provider declares how it takes dictionary
//! terms and with what limits (docs/providers.md). Input is the dictionary,
//! most-used first, so every cap keeps the most-used terms.

/// How a provider accepts vocabulary, with its caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VocabStrategy {
    /// OpenAI `gpt-transcribe`: repeated `keywords[]` (no documented max; we cap).
    KeywordsField { max_terms: usize },
    /// Deepgram: repeated `keyterm` query params, ≤ `max_tokens` in total.
    QueryKeyterm { max_terms: usize, max_tokens: usize },
    /// ElevenLabs `keyterms`: each shorter than `max_chars` + 1 chars and ≤ `max_words` words.
    Keyterms { max_terms: usize, max_chars: usize, max_words: usize },
    /// AssemblyAI `keyterms_prompt`: each ≤ `max_words` words.
    KeytermsPrompt { max_terms: usize, max_words: usize },
    /// Mistral `context_bias` (array of strings).
    ContextBias { max_terms: usize },
    /// No vocabulary field: "Vocabulary: a, b" appended to the prompt.
    Prompt { max_tokens: usize },
}

pub const OPENAI_KEYWORDS: VocabStrategy = VocabStrategy::KeywordsField { max_terms: 100 };
/// Whisper-style prompts cap at 224 tokens (Groq docs).
pub const WHISPER_PROMPT: VocabStrategy = VocabStrategy::Prompt { max_tokens: 224 };
pub const DEEPGRAM: VocabStrategy = VocabStrategy::QueryKeyterm { max_terms: 50, max_tokens: 500 };
pub const ELEVENLABS: VocabStrategy = VocabStrategy::Keyterms { max_terms: 1000, max_chars: 49, max_words: 5 };
pub const ASSEMBLYAI: VocabStrategy = VocabStrategy::KeytermsPrompt { max_terms: 1000, max_words: 6 };
// UNVERIFIED: Mistral documents no context_bias limit; 100 mirrors our OpenAI cap.
pub const MISTRAL: VocabStrategy = VocabStrategy::ContextBias { max_terms: 100 };

impl VocabStrategy {
    /// Sanitised terms that fit this provider's limits. For `Prompt`, the budget
    /// is applied by `fold_into_prompt`, so all clean terms are returned.
    pub fn apply(self, raw: &[String]) -> Vec<String> {
        let clean = strip(raw);
        let words = |t: &String| t.split_whitespace().count();
        match self {
            Self::KeywordsField { max_terms } | Self::ContextBias { max_terms } => clean.take(max_terms).collect(),
            Self::Keyterms { max_terms, max_chars, max_words } => {
                clean.filter(|t| t.chars().count() <= max_chars && words(t) <= max_words).take(max_terms).collect()
            }
            Self::KeytermsPrompt { max_terms, max_words } => clean.filter(|t| words(t) <= max_words).take(max_terms).collect(),
            Self::QueryKeyterm { max_terms, max_tokens } => {
                let mut used = 0;
                clean
                    .take(max_terms)
                    .take_while(|t| {
                        used += approx_tokens(t);
                        used <= max_tokens
                    })
                    .collect()
            }
            Self::Prompt { .. } => clean.collect(),
        }
    }
}

/// Strips `<`, `>`, CR and LF (any of them fails a whole gpt-transcribe request,
/// and they are never useful elsewhere), trims, and drops empty terms.
fn strip(raw: &[String]) -> impl Iterator<Item = String> + '_ {
    raw.iter()
        .map(|k| k.chars().filter(|c| !matches!(c, '<' | '>' | '\r' | '\n')).collect::<String>())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

/// Conservative token estimate: 600 ASCII bytes stay under Whisper's 224 tokens;
/// non-ASCII (Arabic etc.) counts ~1 token per char.
pub fn approx_tokens(s: &str) -> usize {
    let ascii = s.bytes().filter(u8::is_ascii).count();
    let other = s.chars().filter(|c| !c.is_ascii()).count();
    (ascii * 224 + other * 600).div_ceil(600)
}

/// Appends "Vocabulary: a, b, c" to the prompt, within `max_tokens`.
/// Expects already-sanitised terms. None when there is nothing to send.
pub fn fold_into_prompt(prompt: Option<&str>, terms: &[String], max_tokens: usize) -> Option<String> {
    let prompt = prompt.map(str::trim).filter(|p| !p.is_empty());
    if terms.is_empty() {
        return prompt.map(str::to_string);
    }
    let mut vocab = String::from("Vocabulary: ");
    for (i, k) in terms.iter().enumerate() {
        // Counts ", " even for the first term (keeps the Phase 4 output: 600 ASCII bytes at 224).
        if approx_tokens(&format!("{vocab}, {k}")) > max_tokens {
            break;
        }
        if i > 0 {
            vocab.push_str(", ");
        }
        vocab.push_str(k);
    }
    Some(match prompt {
        Some(p) => format!("{p}\n{vocab}"),
        None => vocab,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn strips_forbidden_chars_and_trims() {
        let out = OPENAI_KEYWORDS.apply(&s(&["<Yappa>flow", "Tauri\r\n", " a\nb ", "x>y<z"]));
        assert_eq!(out, s(&["Yappaflow", "Tauri", "ab", "xyz"]));
        // Every strategy strips.
        for v in [WHISPER_PROMPT, DEEPGRAM, ELEVENLABS, ASSEMBLYAI, MISTRAL] {
            assert_eq!(v.apply(&s(&["<a>\r\n", "", "  "])), s(&["a"]), "{v:?}");
        }
    }

    #[test]
    fn keywords_cap_at_100_after_dropping_empties() {
        let mut raw = vec![String::new(); 5];
        raw.extend((0..150).map(|i| format!("k{i}")));
        let out = OPENAI_KEYWORDS.apply(&raw);
        assert_eq!(out.len(), 100);
        assert_eq!((out[0].as_str(), out[99].as_str()), ("k0", "k99"));
        assert_eq!(MISTRAL.apply(&raw).len(), 100);
    }

    #[test]
    fn keeps_unicode() {
        assert_eq!(OPENAI_KEYWORDS.apply(&s(&["مرحبا", "Zürich"])), s(&["مرحبا", "Zürich"]));
    }

    #[test]
    fn deepgram_prefers_50_most_used_within_500_tokens() {
        let raw: Vec<String> = (0..80).map(|i| format!("term{i}")).collect();
        let out = DEEPGRAM.apply(&raw);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0], "term0");
        let long: Vec<String> = (0..50).map(|i| format!("{i}{}", "x".repeat(60))).collect();
        let out = DEEPGRAM.apply(&long);
        assert!(out.iter().map(|t| approx_tokens(t)).sum::<usize>() <= 500);
        assert!(!out.is_empty() && out.len() < 50);
        assert_eq!(out[0], long[0]);
    }

    #[test]
    fn elevenlabs_drops_long_or_wordy_terms() {
        let ok49 = "a".repeat(49);
        let bad50 = "b".repeat(50);
        let out = ELEVENLABS.apply(&s(&[&ok49, &bad50, "one two three four five", "one two three four five six", "Yappaflow"]));
        assert_eq!(out, s(&[&ok49, "one two three four five", "Yappaflow"]));
        let many: Vec<String> = (0..1200).map(|i| format!("t{i}")).collect();
        assert_eq!(ELEVENLABS.apply(&many).len(), 1000);
    }

    #[test]
    fn assemblyai_allows_six_words() {
        let out = ASSEMBLYAI.apply(&s(&["a b c d e f", "a b c d e f g", "x"]));
        assert_eq!(out, s(&["a b c d e f", "x"]));
        let many: Vec<String> = (0..1200).map(|i| format!("t{i}")).collect();
        assert_eq!(ASSEMBLYAI.apply(&many).len(), 1000);
    }

    #[test]
    fn folds_keywords() {
        let kw = s(&["a", "b", "c"]);
        let f = |p, k: &[String]| fold_into_prompt(p, k, 224);
        assert_eq!(f(None, &kw).as_deref(), Some("Vocabulary: a, b, c"));
        assert_eq!(f(Some("ctx"), &kw).as_deref(), Some("ctx\nVocabulary: a, b, c"));
        assert_eq!(f(Some("  "), &kw).as_deref(), Some("Vocabulary: a, b, c"));
        assert_eq!(f(Some("ctx"), &[]).as_deref(), Some("ctx"));
        assert_eq!(f(None, &[]), None);
        assert_eq!(f(Some(""), &[]), None);
    }

    #[test]
    fn prompt_budget_is_224_tokens_600_bytes() {
        let many: Vec<String> = (0..200).map(|i| format!("term{i}")).collect();
        let out = fold_into_prompt(None, &many, 224).unwrap();
        assert!(out.len() <= 600 && out.len() > 580, "{}", out.len());
        assert!(approx_tokens(&out) <= 224);
        let arabic: Vec<String> = (0..100).map(|i| format!("مرحبا{i}")).collect();
        let out = fold_into_prompt(None, &arabic, 224).unwrap();
        assert!(out.chars().filter(|c| !c.is_ascii()).count() <= 224);
        assert_eq!(approx_tokens("مرحبا"), 5);
    }
}
