# Jev significance and splitting evaluations

The `reviewer` Cargo feature `jev-evals` enables an experimental evaluator and
its labelled corpus. It is absent from default builds and `make check`. The live
test is also ignored, including under `--all-features`: requesting live calls
requires both the feature and `--ignored`.

Validate the dataset, target ownership, splitting and scoring without API calls:

```sh
cargo test -p reviewer --features jev-evals jev::evals
```

Export the current token-budgeted recursive requests without calling Jev:

```sh
JEV_EVAL_BUDGETS=14000 JEV_EVAL_REPEATS=1 \
  JEV_EVAL_OUTPUT="$(pwd)/target/jev-evals/offline-plan" \
  cargo test -p reviewer --features jev-evals export_recursive_requests -- --ignored
```

This writes `planned.jsonl` with request bodies, target units and token
estimates. It does not write label text into the requests. The
[prompt experiment](jev-prompt-experiment-results.md) consumes that export.

Run the comparison against Jev, using `TYPESAFE_API_KEY` from the environment:

```sh
cargo test -p reviewer --features jev-evals jev::evals::live_compare_strategies -- --ignored --nocapture
```

These are paid requests to TypeSafe. They send only the checked-in fixture code
and the evaluator's rubric. Labels, rationales and provenance are never included
in model input. They do not open a reviewer pane or change a saved review.

## Experiment

The four strategies are:

| Strategy | Target and context |
| --- | --- |
| `LegacyBlocks` | Production candidate preparation: consecutive added/deleted rows, three unchanged rows on each side, current 32-candidate and 10 KiB limits. |
| `WholeHunk` | One judgment for every changed line in a complete hunk; retain it as oversized if it cannot fit. |
| `Recursive` | Recursively divide an oversized hunk near its token midpoint. Keep each fitting leaf's local context. |
| `RecursiveOverlap` | Use exactly the same target leaves as `Recursive`, then extend their context into the original hunk within the token budget. |

Every request uses the production Choice rubric, HTTP transport and response
validator. Hunk strategies append the same scope clarification and use the same
row representation. A row's old/new line numbers and added/deleted status survive
when it is included as context. Only owned target lines receive that request's
judgment. A significant target makes the whole target range significant: the
line-level score therefore reveals over-retention in mixed hunks.

Only the recursive pair isolates the effect of overlap. Comparisons with legacy
also change representation and prompt scope; they are comparisons of complete
strategies, not a causal measurement of chunk size alone. The eval does not apply
the production 30-second job deadline; it measures all planned requests.

Cuts prefer unchanged-row boundaries. A coarse Rust replacement with matching
function names on both sides is first arranged into paired function sections,
preserving every old/new line coordinate. Each function's removal and addition
stay together; pure addition or deletion runs may also split on blank lines.
These are textual heuristics, not an AST parser. Unrecognized or unmatched
replacements remain paired as one edit. An indivisible oversized edit
remains required and is scored as oversized. Context expansion uses a balanced
window followed by remaining space on either side, stopping at allowed cut
points. It seeks a locally maximal window, not a globally optimal request count
or a guarantee that all dependencies are present. No merge pass is applied, so
both recursive variants retain identical targets.

`tiktoken-rs` with `o200k_base` counts the complete serialized request as a local
estimate. It is not Jev's exact input encoding. In a live run of this corpus,
Jev reported roughly 1.7–2 times the local estimate, and a 23,978-token estimate
received `max_tokens_exceeded`. The 14,000-token ceiling leaves empirical headroom
against the documented 32k state-plus-question limit; it is not a guarantee for
new content. The 1,024 and 2,048 budgets deliberately stress boundaries. No
request is silently truncated. Provider rejection is recorded as failure, not an
insignificant result.
See [Jev limits](https://docs.typesafe.ai/models) and
[Rust tokenizer documentation](https://docs.rs/tiktoken-rs/latest/tiktoken_rs/).

Configuration:

| Environment variable | Default | Meaning |
| --- | --- | --- |
| `JEV_EVAL_BUDGETS` | `1024,2048,14000` | Comma-separated complete-request token estimates, including headroom; accepted range 512–14000. |
| `JEV_EVAL_REPEATS` | `3` | Repeat every case, strategy and budget. Strategy order rotates between repeats. |
| `JEV_EVAL_CASE` | empty | Restrict cases by ID substring; no match is an error. |
| `JEV_EVAL_MAX_REQUESTS` | `2000` | Reject the complete plan before spending if it exceeds this API-call cap. |
| `JEV_EVAL_OUTPUT` | a new directory under `target/jev-evals/` | Results directory. An existing request log is never overwritten. |

For a small connectivity check:

```sh
JEV_EVAL_CASE=comment-only JEV_EVAL_REPEATS=1 JEV_EVAL_BUDGETS=2048 \
  cargo test -p reviewer --features jev-evals jev::evals::live_compare_strategies -- --ignored --nocapture
```

## Labels and scores

`crates/reviewer/testdata/jev-evals/labels.json` identifies each patch, provenance,
purpose, and every changed line on both sides of the diff. The numeric significance
label is `0` for no independent consequential review decision and `1` for a change
that requires an explanation. Each label includes a reason and category. This
does not label whether the code is correct. Unchanged context has no score.

The initial corpus contains agent-authored mutations and synthetic variations of
this repository's source at checkpoint
`21d15023a5ed4039e7940005c178d7e49cea2434`. They are development labels produced
before running Jev, not independent human ground truth or annotations of actual
historical commits. Fixture line numbers are local to the frozen excerpts. The
checked-in patches make runs independent of later source changes. The corpus
includes comments, formatting, display labels, explanatory versus operational
defaults, removed guards, mixed changes, distant consumers, Unicode, multiple
hunks, a full source-file hunk, paired function rewrites and indivisible long lines.

The dataset validator rejects missing, duplicate and out-of-diff labels. Reports
include a SHA-256 of the metadata and patch contents; labels must not be changed
merely to agree with a model's answer. Correcting a label changes that fingerprint.

`requests.jsonl` retains exact requests, responses, model/version, probabilities,
target coordinates, local estimates, reported usage, latency and per-line scores.
`summary.json` is updated as trials finish and groups results by strategy/budget.
An interrupted run has `complete: false`. Raw logs contain fixture source but no
API credentials.

Scores expose trade-offs separately:

- False-exclusion rate: significant lines incorrectly excluded / significant lines.
- Insignificant-exclusion recall: correctly excluded lines / insignificant lines.
- Exclusion precision: correctly excluded lines / all excluded lines.
- Decisive accuracy: correct significant or insignificant predictions / all labels.
  Uncertain, failed, oversized and unassigned predictions earn no accuracy credit.
- Multiclass Brier score: sum of squared errors across the three probabilities,
  averaged over lines with valid distributions. The binary reference labels give
  the uncertain class a target probability of zero; abstention is reported separately.
- Request count, reported input/output tokens and request latency. Compare reported
  tokens with local estimates before treating the tokenizer margin as adequate.

The [initial live results](jev-evals-results.md) record the first complete
comparison and the observed tokenizer underestimate.

Missing predictions remain visible in the denominator. A test pass means the
experiment completed without provider failures and accounting errors; it does
not assert a quality threshold or that overlap wins. Inspect per-case results,
especially mixed and boundary-dependent cases. Lines from a common change and
repeats of the same case are correlated; pooled line counts are not independent
statistical samples. Independent relabelling and a held-out corpus are needed
before choosing production policy or claiming general superiority.

## Research datasets

Research checked on 2026-09-23 found related tasks, but no verified drop-in corpus
with our exact per-line “needs an Explore explanation” labels:

| Dataset | Existing labels | Suitability |
| --- | --- | --- |
| [Herbold et al., fine-grained tangling dataset](https://link.springer.com/article/10.1007/s10664-021-10083-5) and [replication kit](https://github.com/sherbold/replication-kit-2020-line-validation) | Manual changed-line labels separating bug fixes from other changes, with multiple annotators and consensus. The kit includes `data/hunk_labels.json`; broader repository context is in SmartSHARK. | Closest granularity. Useful for importing mixed patches and preserving annotation disagreement. Bug-fix relevance is a different label from review significance: non-bug-fix changes can still be consequential. |
| [CodeReviewer](https://github.com/microsoft/CodeBERT/tree/master/CodeReviewer), [dataset archive](https://zenodo.org/records/6900648) | Diff quality estimation, review comments and refinements. | Useful changes and review context, but a change receiving no comment does not establish that each line is insignificant. Needs per-line relabelling. |
| [CasCADe / Identifying Casualty Changes](https://asejfia.github.io/cascade.github.io/) | Evaluation of changes incidental to the main intent of software patches. | Related to mixed-patch attribution. The project links an evaluation archive, but that download was unavailable through the research tool; its actual schema and reuse terms were not verified. |

No external dataset is downloaded by the test or bundled as presumed ground
truth. To add one, preserve its origin, license, original labels and disagreement;
map or relabel review significance explicitly, freeze the patches, and validate
both old and new line coordinates. Never map “refactoring,” “uncommented,” or
“unrelated to this bug fix” directly to insignificant.
