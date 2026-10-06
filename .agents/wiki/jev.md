# Jev

What Jev does in the reviewer: when it runs, what it is sent, how its answers become review marks, and the settings production uses with the evidence that chose them.

Jev is the classifier that marks changed lines too insignificant to need the reviewer's
attention ([CONTEXT.md](../../CONTEXT.md)). It is a model of TypeSafe AI, pinned to
`jev-1.13.0`; an answer from another model counts as a failure.

## When it runs

- Jev is enabled when the reviewer process has a nonempty `TYPESAFE_API_KEY`. A missing or
  whitespace-only key disables it.
- It runs on the reviewer's "mark insignificant" command in Files
  ([Review marks in Files](review-marks.md)) and at the start of each Explore round, before
  the kickoff, with the same classification policy. The Explore agent sees only what is
  left, and the Explore prompts do not mention Jev.
- The reviewer sends bounded before/after code snippets and repository-relative paths to
  TypeSafe AI.
- Lines Jev judges insignificant get a review mark that names Jev. Uncertain, failed and
  oversized checks leave their lines unreviewed. The reviewer can reopen a hunk Jev marked.
- A `make vision` session never reaches the paid classifier: its `jev` command stands in
  for it ([Terminal UI exploration](tui-vision.md)).

## Production settings

Production (`crates/reviewer/src/runtime/jev.rs` and `jev/optimized.rs`) asks one
checklist Choice question per hunk window, with the path, the language and the
first twelve lines of the old and new file as metadata, a window of 16,000
estimated tokens, and keeps an exclusion only when its probability is at least
0.85. Rows are sent one JSON row per diff line, without omission notes
(`Format::RowsNoOmissions`).

The prompt, metadata, window and threshold are the development winner of the
[history study](jev-history-study.md) of 2026-09-23: on its validation split,
with the h054 erratum applied, it excluded 1,393 of 1,688 out-of-scope lines and
no significant line. The corpus is this repository's own history with
agent-authored labels, so this is provisional evidence, not a safety guarantee.
The two request formats the studies measured were rows with omission notes
(`rows_legacy`) and the compact unified diff (`unified_compact`): compact used 55.6% fewer
input tokens and excluded 978 validation lines where rows excluded 1,390, so it stays an
evaluation arm. No recorded run compares rows without omission notes, the format
production sends, with the other two.

## Evaluations

[Jev evaluations](jev-evals.md) and the [history study](jev-history-study.md) say how to
measure a change to the prompt, the window, the threshold or the request format before it
reaches production.
