//! Spoken snippets: "sign off" said alone, or at the end of a dictation, becomes the saved text.
//! Matching ignores case and punctuation and tolerates 1 edit per 8 characters of the trigger.
//! Runs after STT, before cleanup; the expansion itself is inserted exactly.

use crate::storage::SnippetRow;

#[derive(Debug, PartialEq)]
pub struct Hit<'a> {
    pub id: i64,
    /// What was said before the trigger (original text, trailing `,;:-` trimmed). Empty = trigger alone.
    pub prefix: String,
    pub expansion: &'a str,
}

/// The longest trigger that matches the whole utterance or its last words.
pub fn find<'a>(text: &str, snippets: &'a [SnippetRow]) -> Option<Hit<'a>> {
    let words = words_with_offsets(text);
    let mut best: Option<(usize, Hit)> = None;
    for s in snippets {
        let trig: Vec<String> = words_with_offsets(&s.trigger).into_iter().map(|(w, _)| w).collect();
        let n = trig.len();
        if n == 0 || n > words.len() {
            continue;
        }
        let tail = words[words.len() - n..].iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>().join(" ");
        let trig = trig.join(" ");
        let tolerance = trig.chars().count() / 8;
        if levenshtein(&tail, &trig) > tolerance || best.as_ref().is_some_and(|(len, _)| *len >= trig.len()) {
            continue;
        }
        let start = words[words.len() - n].1;
        let prefix = text[..start].trim_end().trim_end_matches([',', ';', ':', '-']).trim_end().to_string();
        best = Some((trig.len(), Hit { id: s.id, prefix, expansion: &s.expansion }));
    }
    best.map(|(_, h)| h)
}

/// Lowercase words joined by single spaces, punctuation dropped.
pub fn normalize(text: &str) -> String {
    words_with_offsets(text).into_iter().map(|(w, _)| w).collect::<Vec<_>>().join(" ")
}

/// Lowercase alphanumeric words, each with its byte offset ("sign-off," → "sign" @0, "off" @5).
fn words_with_offsets(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for token in text.split_whitespace() {
        let start = pos + text[pos..].find(token).unwrap_or(0);
        pos = start + token.len();
        let mut word: Option<(String, usize)> = None;
        for (i, c) in token.char_indices() {
            if c.is_alphanumeric() {
                word.get_or_insert_with(|| (String::new(), start + i)).0.extend(c.to_lowercase());
            } else if let Some(w) = word.take() {
                out.push(w);
            }
        }
        out.extend(word);
    }
    out
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snips() -> Vec<SnippetRow> {
        [(1, "sign off", "Best regards,\nSam"), (2, "my email", "me@example.com"), (3, "off", "x")]
            .into_iter()
            .map(|(id, t, e)| SnippetRow { id, trigger: t.into(), expansion: e.into(), uses: 0 })
            .collect()
    }

    #[test]
    fn trailing_trigger_keeps_the_line() {
        let s = snips();
        let h = find("Thanks for the call, sign off.", &s).unwrap();
        assert_eq!((h.id, h.prefix.as_str(), h.expansion), (1, "Thanks for the call", "Best regards,\nSam"));
    }

    #[test]
    fn whole_utterance_is_expansion_only() {
        let s = snips();
        assert_eq!(find("My email.", &s).unwrap().prefix, "");
        assert_eq!(find("My email.", &s).unwrap().id, 2);
    }

    #[test]
    fn fuzzy_and_punctuation() {
        let s = snips();
        assert_eq!(find("Sign-off!", &s).unwrap().id, 1);
        assert_eq!(find("sign of", &s).unwrap().id, 1); // 1 edit in 8 chars
        assert!(find("my mail box", &s).is_none());
        assert!(find("sign it off now", &s).is_none());
        assert_eq!(find("switch it on-off", &s).unwrap().prefix, "switch it on");
    }

    #[test]
    fn longest_trigger_wins() {
        assert_eq!(find("please sign off", &snips()).unwrap().id, 1);
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "ab"), 2);
    }
}
