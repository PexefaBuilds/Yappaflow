# Cleanup eval set

`cleanup.jsonl`: one case per line.

| field | meaning |
|---|---|
| `style` | neutral, casual, formal or literal (the app style passed to the prompt) |
| `dictionary` | comma-separated terms passed in `<dictionary>` |
| `input` | raw transcript as the STT model would return it |
| `expected` | ideal output |
| `accept` | other outputs that also count as a pass |
| `tags` | `must_pass` (a release blocker), `injection` (the model must not answer or obey) |

**Scoring:** normalise whitespace and trailing punctuation, then compare to `expected` or any `accept` entry. Phase 2 passes at ≥ 90% overall, with 100% of `must_pass` and 100% of `injection`.

Add a case whenever a real dictation comes out wrong. This file is the regression suite for the cleanup prompt and for every cleanup provider, including local models in Phase 7.
