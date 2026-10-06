# Jev history study

The method and commands of the opt-in study that compares prompts, metadata, exclusion rules and token windows for [Jev](jev.md)'s significance judgement on an audited corpus of this repository's own commits, and of its compact query comparison.

It needs the `jev-evals` Cargo feature ([Jev evaluations](jev-evals.md)) and
never runs during `make check`. Every result it produces (raw responses, scores,
the interactive HTML report and its JSON) is written to the run directory under
`target/`; do not commit results.

## Corpus and annotations

The frozen [corpus](../../crates/reviewer/testdata/jev-evals/history/labels.json)
contains 180 exact historical file diffs from 52 commits, with 18,997 changed
old/new lines. Its [index](../../crates/reviewer/testdata/jev-evals/history/audit.json)
records commit, path, split, categories and line counts.

Sampling considers the most recent 70 commits ending at
`21d15023a5ed4039e7940005c178d7e49cea2434`, selects both positive and negative
cases and size bands, and caps contributions from a single commit. Exact
duplicate changed-text payloads are removed. Adjacent groups of three commits
remain together in development or validation: 115 development cases and 65
validation cases. This is deliberately stratified, not a prevalence sample.
The dataset includes Rust, TOML, runtime Markdown prompts, prose docs and Makefiles.
Generated artifacts other than lockfiles and other languages are underrepresented.

Initial annotations use `tree-sitter==0.25.2` and `tree-sitter-rust==0.24.0`.
Comments, imports/re-exports/module declarations, formatting, test-only code,
prose documentation, generated output and lockfiles are outside the reviewer's
scope. Other code, attributes, configuration, runtime strings and embedded
agent instructions are significant. An excluded chunk must contain no significant
target line. Scope is a reviewer preference, not a claim of semantic inertness.

Three GPT-6-sol agents at medium effort audited separate 60-case batches.
A focused follow-up checked blank lines within Rust literals. The
[annotation record](../../crates/reviewer/testdata/jev-evals/history/annotation-review.json)
preserves their evidence and all 36 corrections: ten manifest/Makefile blank
separators became insignificant, and 26 blank lines inside runtime Markdown
strings became significant. Labels were frozen before inference. No Jev answers
were used to set labels. These are still agent-authored labels, not independent
human gold annotations.

### Post-run erratum

The first validation score reported two false-excluded lines in h054. Inspecting
the source showed that `#[cfg(all(test, unix))]` and its adjacent `#[path]`
attribute both belong to a test-only module. Under the explicit policy, both
labels should be zero. A further GPT-6-sol audit checked all 180 cases for this
error class and found exactly these two corrections; the annotation helper now
recognizes the compound test gate.

The [erratum](../../crates/reviewer/testdata/jev-evals/history/errata.json) was first
applied as an explicitly labelled sensitivity analysis of the original ten
validation configurations. Frozen labels, original scores, selected configurations
and model responses are preserved. The timed rerun knew these corrections in
advance, repeats the same frozen input corpus, and reports both corrected and
original-label scores for its new finalists. Within each run there is no retuning
or reordering on validation. This is not a fresh independent holdout. Apply the
corrections to any newly generated corpus; do not overwrite this frozen dataset.

A separate [worst-example audit](../../crates/reviewer/testdata/jev-evals/history/worst-examples-audit.json)
confirmed that all 31 distinct significant coordinates in the displayed failure
examples are genuine errors. That check is limited to the listed examples, which
are capped at five per worst configuration.

## Frozen design

- Prompts: terse policy Choice, structured checklist Choice, contrastive-example
  Choice, or five category/review Nouls. Exact text is in
  [design.py](../../crates/reviewer/testdata/jev-evals/study/design.py).
- Metadata: rows and omission note; add original path and language; additionally
  add the first twelve lines of each old/new source file. No commit description,
  gold category, significance label, annotation rationale or split enters state.
- Windows: 4k, 8k, 12k, 16k and 20k estimated tokens, respectively 12.5%, 25%,
  37.5%, 50% and 62.5% of the nominal 32k per-question limit.
- Rules: Choice argmax, exclusion probability, probability margin, probability
  times confidence, or confidence gates of 0.8/0.95; Noul maximum, clipped sum,
  noisy-or, maximum with review veto, or review-probability complement.
- Thresholds: 0.50, 0.60, 0.70, 0.80, 0.85, 0.90, 0.95, 0.97, 0.99, 0.995.

The opt-in Rust planner reuses the existing eval splitter. A fitting full hunk
is one target. Oversized hunks recursively split near their token midpoint at
legal boundaries; old/new replacement pairs stay together. Split target ownership
is disjoint, and each leaf gains neighboring context while it fits. Indivisible
oversized targets remain required. Response-driven uncertainty splitting is not
part of this study.

`o200k_base` estimates serialized state plus that prompt family's longest question
and 32 overhead tokens. It is not Jev's tokenizer, so provider-limit failures are
measured, not silently discarded. Jev documents 32k for state plus longest question
and 64k for state plus all questions. See [model limits](https://docs.typesafe.ai/models).

Development batches independent questions only when their source states are
identical. TypeSafe documents that these questions run independently and cannot
see one another's answers ([building guide](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)).
Finalist validation sends each prompt family's exact questions. Categories remain
a five-question request. Exact repeated payloads across window sizes are cached;
they are not additional independent observations. The run uses one observation
per unique payload. The model is pinned to `jev-1.13.0`.

## Selection and accounting

Rank by fewer false-excluded cases, then fewer significant lines hidden, then
fewer failed/oversized chunks. Among equal safety results, maximize correctly
excluded insignificant lines per request, then total useful lines, fewer requests,
fewer estimated tokens, and a stricter threshold. Useful line credit requires the
whole target to be correctly excluded; mixed erroneous exclusions earn no utility.

Cells with provider failures or oversized targets cannot enter the shortlist or
worst-quality ranking; they remain visible as ineligible cells. Choose one best
rule per complete prompt × metadata × window cell and show the best ten.
The shortlist is saved before validation and cannot change on resume. Validation
scores that exact list without sorting or retuning. The worst ten development
approaches are distinct prompt/metadata/rule families, ranked by false cases and
lines, with actual input and answer examples.

Validation cells with a failure or oversized target are displayed as incomplete,
never as zero-error successes. The window table chooses a complete cell where one
exists; otherwise it displays the incomplete cell only as a sizing diagnostic.

Per-configuration requests count distinct exact payloads needed to run that
configuration alone, including its own cache reuse.
Physical experimental calls deduplicate payloads and may share questions.
Development provider input-token usage is reported only at the physical run level;
per-configuration token figures are estimates. Validation uses exact question sets
and records their measured tokens. Failed calls
stay required and receive no safe-exclusion credit; retries and failures remain
in raw records. Correlated lines, related commits and cached responses do not
justify an independent-line confidence interval.

## Reproduce

### Runtime measurements

The HTML and JSON reports (`jev-history-study.html` and
`jev-history-results.json` in the run directory) include median/p95 request latency for the finalists
and token-window comparisons, plus measured API execution wall time for each
phase. Four HTTP requests can run concurrently. Request-time sums overlap and
must not be read as wall time; development requests can also contain questions
shared by several prompt configurations. Validation sends each finalist's exact
question set. Counts and latency deduplicate exact physical payloads per cell.

Latency includes client serialization, HTTP, validation, retries and backoff,
but not waiting for a worker. The p95 uses nearest rank. Wall time includes
dispatch and response recording. Loading/verifying existing request plans is
timed separately. Tokenization/export, offline scoring, report generation and
pauses between phases are excluded. A resumed run sums complete execution
sessions; missing or interrupted timing is reported as unknown. A cached-only
invocation does not replace the measurements with a near-zero duration.

Four workers is an experiment setting, not a measured optimum. TypeSafe's
[model limits](https://docs.typesafe.ai/models) list 1,200 requests/minute and
250,000 tokens/second. Its [HTTP guidance](https://docs.typesafe.ai/api#handling-rate-limits)
requires backoff on 429/529; the [SDK retry policy](https://docs.typesafe.ai/sdk/python/api/retries)
supports server retry-delay headers. The experimental runner currently retries
429 and selected 5xx responses, but lacks 529 and retry-header handling. A future
concurrency sweep should add request/token pacing and these retry behaviors first.

Use a new output directory for fresh requests. Once the request plans exist
(see [Corpus and request generation](#corpus-and-request-generation); here under
`target/jev-history-study/plans`), these commands rerun the full grid, freeze a
new top ten, validate it and build the report in `$JEV_RUN` (requires
`TYPESAFE_API_KEY`):

```sh
export PYTHONDONTWRITEBYTECODE=1
JEV_RUN=$(mktemp -d "$PWD/target/jev-timed.XXXXXX")
ln -s "$PWD/target/jev-history-study/plans" "$JEV_RUN/plans"
python3 crates/reviewer/testdata/jev-evals/study/execute.py --plan "$JEV_RUN/plans" --output "$JEV_RUN" --split development
python3 crates/reviewer/testdata/jev-evals/study/score.py --plan "$JEV_RUN/plans" --output "$JEV_RUN" --split development
python3 crates/reviewer/testdata/jev-evals/study/execute.py --plan "$JEV_RUN/plans" --output "$JEV_RUN" --split validation --shortlist "$JEV_RUN/shortlist.json"
python3 crates/reviewer/testdata/jev-evals/study/score.py --plan "$JEV_RUN/plans" --output "$JEV_RUN" --split validation
python3 crates/reviewer/testdata/jev-evals/study/report.py --run "$JEV_RUN"
```

### Corpus and request generation

Offline corpus generation is separate from paid evaluation. It refuses an existing
output directory to preserve completed audits. To regenerate proposals, install
the two pinned parser packages in an isolated environment and run:

```sh
python crates/reviewer/testdata/jev-evals/study/history.py --output /tmp/jev-history-proposals --limit 180
```

The tracked dataset already contains reviewed corrections. To reapply the recorded
contributions to a fresh proposal corpus, extract the `contributions` objects from
`annotation-review.json` to temporary JSON files and pass them to `annotate.py`
with `--dataset /tmp/jev-history-proposals`. Review any changed proposals before
using them; do not overwrite a frozen experiment's labels.

Generate one configuration per prompt family with `design.py --family NAME
--output /absolute/run/plans/NAME --config /tmp/NAME.json`, then export offline:

```sh
JEV_STUDY_CONFIG=/tmp/NAME.json cargo test -p reviewer --features jev-evals export_study_requests -- --ignored
```

After all four exports (`terse`, `checklist`, `contrast`, `categories`) complete,
run the five stages of [Runtime measurements](#runtime-measurements) on those plans.
Add `--dry-run` to `execute.py` to check ownership, corpus hashes and planned call
counts for free.

The runner resumes only the same frozen experiment. Completed successes and
failures are retained. It makes at most three attempts for transient errors, using
four workers. Never put credentials in corpus files, logs or HTML.

Offline annotation/rule checks:

```sh
python -m unittest discover -s crates/reviewer/testdata/jev-evals/study -p 'test_*.py'
cargo test -p reviewer --features jev-evals runtime::jev::evals
```

## Compact query comparison

`compact.py` compares two request formats under the fixed production policy
(checklist prompt, headers metadata, 16,000-token window, exclusion probability of
at least 0.85) on the same history corpus: `rows_legacy`, one JSON row per diff
line, and `unified_compact`, a unified diff with the rows to judge listed in
`target_rows`. Each arm runs with one sequential HTTP worker, in a fixed order, so
whole-arm wall time includes provider-load drift.

Changed old/new line coordinates are counted once; context and overlapping rows
do not enter the throughput denominators. Provider failures and oversized chunks
stay required and count as not evaluated. Useful excluded lines count only wholly
insignificant chunks. Request latency percentiles include failures and retries. At the
16,000-token budget every compact window of this corpus covers a full hunk, so the
comparison does not exercise `target_rows`. The h054 erratum affects scoring only; the original-label
counts stay in the JSON.

Write the two-arm configuration, export its requests offline, and keep the two
arms in one plan:

```sh
python3 -B crates/reviewer/testdata/jev-evals/study/compact.py config \
  --output /tmp/jev-compact.json --plan-output "$PWD/target/jev-compact-export"
JEV_STUDY_CONFIG=/tmp/jev-compact.json cargo test -p reviewer --features jev-evals export_study_requests -- --ignored
python3 -B crates/reviewer/testdata/jev-evals/study/compact.py two-arm-plan \
  --plan target/jev-compact-export/planned.jsonl --output target/jev-compact-two-arm-plan
```

Check the plan for free, run it (requires `TYPESAFE_API_KEY`), and build the
report:

```sh
python3 -B crates/reviewer/testdata/jev-evals/study/compact.py audit \
  --plan target/jev-compact-two-arm-plan/planned.jsonl
python3 -B crates/reviewer/testdata/jev-evals/study/compact.py run \
  --plan target/jev-compact-two-arm-plan/planned.jsonl --output target/jev-compact-run
python3 -B crates/reviewer/testdata/jev-evals/study/compact.py report \
  --plan target/jev-compact-two-arm-plan/planned.jsonl --output target/jev-compact-run
```

`audit --previous-plan <planned.jsonl>` also checks that the `rows_legacy` requests
are byte-identical to the checklist/headers/16k requests of an earlier history
study export. A fresh `run` requires an empty output directory; its manifest
permits resuming only the same exact plan. `report` writes
`jev-compact-query.md`, `.html` and `-results.json` into the output directory.
