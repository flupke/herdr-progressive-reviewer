# Time-boxed Jev prompt and threshold experiment

The [visual report](jev-prompt-experiment.html) presents the prompts, split
algorithm, interactive threshold comparison, and limitations.

On 2026-09-23, 153 paid calls to Jev `jev-1.13.0` completed without provider
errors on token-budgeted, recursively split hunks. The raw requests and
responses are in local
`target/jev-prompt-evals/ebc5c2f4-e8b2-45d6-825f-63a2a60dca9a/`.
The request plan is reproducible without Jev calls using the opt-in Rust
exporter, which uses the same token-budgeted recursive planner as the
original splitting evaluation:

```sh
JEV_EVAL_BUDGETS=14000 JEV_EVAL_REPEATS=1 \
  JEV_EVAL_OUTPUT="$(pwd)/target/jev-prompt-evals/offline-plan" \
  cargo test -p reviewer --features jev-evals export_recursive_requests -- --ignored
python3 crates/reviewer/testdata/jev-evals/prompt_experiment.py \
  --plan target/jev-prompt-evals/offline-plan/planned.jsonl --dry-run
```

The exporter was added after the paid run; it generates 17 candidates from
the current fixtures. The paid run used the earlier recursive plan for the
original cases and manually encoded windows with the same target rows for the
three new declaration cases. The import-target fixture's manual window had two
fewer context rows than the later Rust export. The offline export is the
supported reproduction path
for future runs. The script can score saved results without provider calls:
`python3 crates/reviewer/testdata/jev-evals/prompt_experiment.py --score-existing
target/jev-prompt-evals/ebc5c2f4-e8b2-45d6-825f-63a2a60dca9a/responses.jsonl`.

The audit corrected two originally insignificant `#[serde(default)]` lines:
they change deserialization compatibility. Those post-audit cases, a synthetic
run of uncalled functions, and a subjective wording case were excluded from
prompt tuning. `crates/reviewer/testdata/jev-evals/curation.json` records the
development/holdout split. Labels are agent-authored judgments, not independent
human ground truth. The reviewer defines imports, re-exports, module
declarations, comments, formatting, test-only changes, prose docs, generated
output and lockfiles as outside Explore review scope, even when an excluded
edit can affect behavior. Three focused declaration fixtures exercise that
policy.

Each frozen candidate is positive if any target line requires review;
otherwise it is negative. Mixed candidates stay required. There are 17
candidate hunks or recursive chunks, 51 judgments per prompt over three
repeats. The category query
asks three yes/no questions in one request: comment-only, import/module-only,
and formatting-only. Its exclusion score is the maximum of their probabilities.
Other newly excluded categories were not queried in this trial.

| Prompt at exclusion threshold 0.90 | Development negatives excluded | Development false exclusions | Holdout negatives excluded | Holdout false exclusions |
| --- | ---: | ---: | ---: | ---: |
| Current Choice | 9/12 | 0/21 positive judgments | 3/6 | 0/12 positive judgments |
| Scope-aware Choice | 12/12 | 0/21 | 6/6 | 0/12 |
| Three category Nouls | 12/12 | 0/21 | 6/6 | 0/12 |

The revised prompts excluded a deliberately behavior-changing import target
as the reviewer requested; the current prompt did not. On the recursively
planned request, the formatting-only holdout received 0.90–0.91 from the
category question, whereas the legacy changed-line block request gave only
0.62–0.66. An earlier 180-call legacy-block comparison is retained in
`target/jev-prompt-evals/224753d9-dd67-4650-9dd5-1cad1f8eadce/`; it is
not the basis for the table. A Noul returns its yes probability, not a
separate confidence field.

No production prompt or threshold was changed by this experiment. A 0.90 gate
avoided false exclusions on these curated candidates, but the sample is too
small and agent-labelled to establish a deployment safety bound. Test-only
code, docs, generated output, lockfiles, and uncertainty-driven splitting need
additional labelled cases before the full policy can be tested. Repeated
judgments and nearby old/new lines are correlated; the counts are not
independent observations.
