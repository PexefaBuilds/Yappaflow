# Phase 4: Dictionary & snippets

## Goal
Names and jargon come out spelled right, the app learns from your corrections, and short spoken triggers expand into saved text.

## Scope
1. **Dictionary table** (migration): `dictionary(id, term, sounds_like, source['user'|'learned'], uses, created_at)`.
2. **STT hints.** Pass dictionary terms to the STT provider through the provider's keyword or vocabulary feature. The exact parameter per provider is in `docs/providers.md`. Providers without one get the terms in `prompt`. Cap at the provider's limit and prefer the most-used terms.
3. **Cleanup hint.** Pass terms in `<dictionary>` (already in the prompt template).
4. **Learn from corrections** (Windows): for 15 s after a paste, read the focused edit control's text through UI Automation (`TextPattern` / `ValuePattern`).
   - If a word we inserted is replaced by a similar-sounding word (edit distance or phonetic match) **twice**, show the "Add X to your dictionary?" toast.
   - Never read anything outside the inserted span. Never store the surrounding text.
   - If UIA isn't available in that app, do nothing.
5. **Snippets table:** `snippets(id, trigger, expansion, uses)`.
   - Match runs after STT, before cleanup.
   - Whole-utterance or trailing-phrase match, case- and punctuation-insensitive, with fuzzy tolerance of 1 edit per 8 characters.
   - Expansion text bypasses cleanup and is inserted exactly.
6. **UI:** Dictionary and Snippets pages as designed (`SetDictionary`, `SetSnippets`), import and export as CSV.

## Out of scope
Cloud sync of the dictionary.

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 4.1 | Add "Tauri" (sounds like "tory"). Say "build the tory app" | "Build the Tauri app" in 9 of 10 tries |
| 4.2 | Dictate "Kubernetes" → it comes out wrong → fix it by hand → repeat | The toast offers to add "Kubernetes" after the 2nd fix |
| 4.3 | Snippet "sign off" → say "thanks for the call, sign off" | The line, then the full expansion, exact including newlines |
| 4.4 | Say "my email" alone | Expansion only |
| 4.5 | Run the cleanup eval set again | No regression (≥ Phase 2 score) |
| 4.6 | Privacy check | The DB contains no text other than dictations, terms and snippets (inspect `yappaflow.db`) |

## Results
