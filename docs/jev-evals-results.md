# Initial Jev splitting results

On 2026-09-23, the `jev-evals` live test completed 684 requests with no provider
failures. It used Jev `jev-1.13.0`, 17 frozen cases, 173 changed-line labels, three
repeats, and estimated request budgets of 1,024, 2,048 and 14,000 tokens. The
dataset fingerprint was
`4fd63c82396bd10df7dc2f36cfd8a230cee8a579cb98407b5d0152c043455c94`.
Raw requests, responses, per-case scores and the aggregate summary are in the
local `target/jev-evals/90fc7075-332a-4905-8db7-980b2dd81cec/` run directory.
This is the fingerprint of the labels used for that run. A later audit corrected
two `serde(default)` labels because those annotations change deserialization of
saved data, and recorded the prompt-tuning subset in
`crates/reviewer/testdata/jev-evals/curation.json`. The original results remain
historical measurements of the original labels.

The experiment supports recursive splitting as a way to judge an oversized
hunk without dropping its changed lines. It does **not** show that overlapping
context improves Jev's significance decisions on this corpus. The recursive
pair had identical target ownership and request counts at each budget:

| Estimated budget | Paired chunks with added context | Changed decisions on those chunks | False exclusions, recursive → overlap | Reported input tokens, recursive → overlap |
| --- | ---: | ---: | ---: | ---: |
| 1,024 | 36 of 75 | 0 | 168 → 168 | 99,036 → 110,583 |
| 2,048 | 6 of 57 | 0 | 168 → 166 | 109,587 → 116,394 |
| 14,000 | 3 of 54 | 0 | 166 → 164 | 140,079 → 168,660 |

Each false-exclusion count is over 324 significant line judgments (108 labelled
old/new lines, each repeated three times). The two apparent improvements at
2,048 and 14,000 came from `multiple-hunks`: the plain and overlapping
requests were byte-for-byte identical for that hunk. Jev gave different
answers on one repeat at each budget, so those differences cannot be
attributed to added context. At 1,024 tokens, the probability-based Brier
score improved slightly with overlap, mostly on one synthetic long addition;
its chosen significance decisions did not change.

At 14,000 estimated tokens, `WholeHunk` left 18 labelled line judgments
oversized (six lines across three repeats). Both recursive variants covered
those lines and had no oversized or failed judgments. The legacy strategy used
63 requests and 49,851 input tokens at that budget; recursive splitting used
54 requests and 140,079 input tokens, and overlap used the same 54 requests
and 168,660 input tokens. Legacy uses a different state shape and rubric
scope, so those counts compare complete strategies rather than isolating the
effect of splitting.

The scores are strongly shaped by the synthetic `large-added-run`: its 18
repetitive, uncalled functions account for 162 of the false exclusions in
every strategy. Whether each such line warrants an independent explanation
needs human review. All labels were authored for this experiment, not
independently checked, and old/new lines and repeated calls are correlated.
These results do not justify choosing an overlap policy for production.

The initial 24,000-token budget was too high: a request estimated at 23,978
tokens returned `max_tokens_exceeded`. Jev reported 29,246 input tokens for
another request estimated at 16,763. The corrected 14,000-token ceiling passed
the largest-hunk check and the full run; its largest overlapping request in
that check used 24,610 reported tokens. This observed margin is not a
guarantee for other source text. See [the eval guide](jev-evals.md) for the
dataset, scoring rules, commands and research sources.

A later [prompt and threshold experiment](jev-prompt-experiment-results.md)
uses audited labels and the reviewer's explicit category policy.
