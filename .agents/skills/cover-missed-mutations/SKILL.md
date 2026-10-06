---
name: cover-missed-mutations
description: Cover missed Rust mutants in one production file.
disable-model-invocation: true
argument-hint: "<filename>"
---

# Cover Missed Mutations

## 1. Establish the baseline

Resolve the argument to one existing Rust production file. Reject test files,
directories, and paths outside `crates/`. Read the complete file, its callers,
its collocated test file, and its related integration tests.

Run `make check`. Continue only with a passing baseline. This step is complete
when the current public behaviors and their existing tests are known.

## 2. Find missed mutations

Run all mutations in the file:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest -j2 \
  --file <filename>
```

`-j2` runs two mutants at a time: more jobs can cause false timeouts and flakes
in tests that share state. Raise `--timeout` only for a test that really needs
longer than the default, five times the baseline and at least 20 seconds.

Read all missed mutants from `mutants.out/missed.txt` and process them in the
next step.

This step is complete when the full file run finishes. If it has no misses,
report that result and stop.

## 3. Define the oracle

Group missed mutants by the observable behavior that they change. State the
public contract for each group. Give every missed mutant one classification:

- **Useful:** it breaks a public rule or stable invariant with a clear oracle.
- **Equivalent:** no observable behavior can distinguish it from the original.
- **Low value:** distinguishing it requires an unstable or private assertion.

Reject equivalent and low-value mutants. Add tests only for useful mutants and
observable behavior. State the oracle before editing tests. Prefer an
independent oracle such as protocol requirements, repository invariants, or
`git apply --check`. Treat the current implementation only as evidence.

Keep equivalent and low-value mutants visible for human confirmation. Add a
permanent skip only after a person confirms the classification. This step is
complete when every miss has a classification and every useful miss has a
stable oracle.

## 4. Add the smallest tests

Add tests at the nearest public or stable boundary. Put unit tests in the
existing collocated `<module>.tests.rs` file. Use an integration test when an
external command or a cross-crate behavior supplies the better oracle.

Prefer one test that catches a related class of mutations. Keep production
code unchanged. If a mutant reveals a separate real defect, report it; do not
change production code only to kill a mutant.

Keep each test deterministic: it depends on time, order or external state only
when that dependency is the behavior under test.

Run the file mutation test with `--iterate` after each edit. Give each group at
most four attempts: most tests that succeed appear in the first three or four.
Continue until every useful miss is caught, has no stable
oracle, or has used its four attempts; a group that used them goes to the report,
for a person to decide whether its oracle is weak, its mutants are equivalent, or
the test boundary is wrong.

## 5. Verify and report

Run the full file mutation test once without `--iterate`, then run the gate
(AGENTS.md, step 5).
Follow the repository's change workflow (AGENTS.md) for review, description, and
installation.

Report:

- the exact file;
- each missed mutant and its classification;
- the product rules protected by new tests;
- equivalent or low-value mutants that need human confirmation;
- groups still missed after four attempts;
- the full file mutation result and the gate's result.

The invocation is complete when every miss is caught, classified, or reported
at its attempt limit, all useful caught results are confirmed by a non-iterative
full file run, and the repository checks pass.
