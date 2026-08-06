# Use an LLM with Rust mutation testing

## Recommendation

Do not leave an LLM agent in an unlimited loop that tries to kill all 1,885
mutants. Use the agent for small, selected groups of missed mutants. Review each
test patch before the next group. Run a full mutation check only after the
focused work is complete.

This recommendation is an inference from the evidence below. Mutation-guided
LLM systems improve tests through feedback and repeated attempts. However,
mutation tools can produce equivalent or low-value mutants, and an LLM can make
a test that is too specific to one synthetic change. A human must decide which
behavior is part of the product contract.

## Evidence

### What a missed mutant means

`cargo-mutants` says that a missed mutant can show a test gap, or it can be
indistinguishable from the correct code. Its official guidance says to inspect
important and surprising misses first. It also says to test the correct public
behavior instead of the exact private mutation. Tests that are too closely
targeted to one mutant can use the wrong level of abstraction
([cargo-mutants: Using the results](https://mutants.rs/using-results.html)).

Mutation score is useful, but it is a proxy. A study of 357 real faults in five
open-source programs found a statistically significant relation between mutant
detection and real-fault detection after it controlled for code coverage. The
same study also reports inherent limits to mutation analysis
([Just et al., FSE 2014](https://homes.cs.washington.edu/~mernst/pubs/mutation-effectiveness-fse2014-abstract.html)).

Therefore, “all mutants caught” is not the product goal. The goal is a small set
of stable tests that protect important observable behavior. This paragraph is
an inference from the two sources above.

### What LLM feedback loops can do

MuTAP adds surviving mutants to LLM prompts and repairs generated tests. In its
evaluation, it detected up to 28% more faulty human-written snippets than its
comparison methods. The authors also report that generated tests can have
syntax errors, functional errors, weak fault detection, and missing boundary
cases
([Dakhel et al., *Effective Test Generation Using Pre-trained Large Language Models and Mutation Testing*](https://arxiv.org/abs/2308.16557)).

An iterative scientific-debugging study asked an LLM to form a hypothesis for
each mutant and refine a test. Its iterative methods reached about an 80%
mutation score, compared with about 60% for one-pass generation. Most successful
tests appeared in the first three or four iterations. The study stopped each
case after ten iterations and found that LLM use cost more than its search-based
comparison
([Straubinger et al., *Mutation Testing via Iterative Large Language Model-Driven Scientific Debugging*](https://arxiv.org/abs/2503.08182)).

Meta's ACH system selected relatively few mutants for a specific risk instead
of processing every available mutant. Engineers accepted 73% of the generated
tests, but judged only 36% as relevant to the target privacy concern. Its LLM
equivalence detector was not perfect without preprocessing
([Foster et al., *Mutation-Guided LLM-based Test Generation at Meta*](https://arxiv.org/abs/2501.12862)).

These results support a selected, iterative workflow. They do not show that an
unattended agent should optimize the complete mutation score. That conclusion
is an inference.

### Equivalent and low-value mutants

`cargo-mutants` can produce a mutant that has the same effective behavior as
the original code. It recommends a visible `#[mutants::skip]` for a hard-to-test
function, a path filter for an untestable module, and a regular-expression
filter for a permanent class such as `Debug` implementations
([cargo-mutants: Using the results](https://mutants.rs/using-results.html),
[cargo-mutants: Skipping untestable code](https://mutants.rs/skip.html)).

Do not let the agent add an assertion only to make an equivalent mutant fail.
Do not test formatting, private call order, or an implementation detail unless
that behavior is a real contract. Record a skip only after a person confirms
that no important public observation can distinguish the mutant. These are
workflow rules inferred from the official guidance.

## Safe workflow for this repository

### 1. Keep the baseline reliable

Run `make check` first. `cargo-mutants` requires reliable, non-flaky baseline
tests. A failed baseline makes mutation results meaningless
([cargo-mutants: Getting started](https://mutants.rs/getting-started.html),
[cargo-mutants: Baseline tests](https://mutants.rs/baseline.html)).

Keep Nextest for the mutation run:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest
```

Nextest can stop soon after one test fails, which can reduce mutation-run time.
It does not run doctests, so `make check` must continue to run doctests
separately
([cargo-mutants: Using nextest](https://mutants.rs/nextest.html)).

### 2. Select one important area

Start with a crate or file that contains product logic. Do not start with all
1,885 candidates. For example:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest \
  --file crates/herdr-client/src/client.rs
```

`--file` limits the run to matching files. `--re` can select a function or a
mutation description, and `--exclude-re` can remove a known low-value class
([cargo-mutants: Filtering files](https://mutants.rs/skip_files.html),
[cargo-mutants: Filtering functions and mutants](https://mutants.rs/filter_mutants.html)).

Preview a selected set without running tests:

```sh
cargo mutants --workspace --list --diff \
  --file crates/herdr-client/src/client.rs
```

`--list` lists candidates, and `--diff` shows each source change
([cargo-mutants: Listing generated mutants](https://mutants.rs/list.html)).

### 3. Give the agent a small batch

Give the agent one related group of about five to ten missed mutants. Ask it to:

1. State the public behavior that each mutant violates.
2. Classify each mutant as useful, equivalent, or low value.
3. Add the smallest test for the public behavior.
4. Change production code only if it finds a separate real defect or a required
   testability boundary.
5. Run `make check` and the focused mutation command.
6. Stop for review.

The batch size is a local control, not a published optimum. It keeps each patch
small enough for a person to review and limits repeated errors from one weak
test oracle.

### 4. Review the oracle, not only the result

For each new test, confirm all of these points:

- The expected value comes from a requirement, public API rule, protocol rule,
  or clear invariant. It does not come only from the current implementation.
- The test fails for the mutant and passes for the original code.
- The assertion checks an observable result or side effect.
- The test is deterministic and does not depend on time, order, or external
  state unless that dependency is the behavior under test.
- One test can catch a related class of errors. It is not named or shaped only
  for one operator replacement.

This gate applies the cargo-mutants instruction to avoid tests that are too
tightly targeted to a mutant and to prefer tests through a public interface
([cargo-mutants: Using the results](https://mutants.rs/using-results.html)).

### 5. Use iteration as a cache, not as proof

For repeated focused runs, use:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest \
  --file crates/herdr-client/src/client.rs --iterate
```

`--iterate` skips mutants that were caught or unviable in an earlier run. It is
a heuristic: source movement can cause retests, and new changes can reduce test
coverage. The official documentation requires a final run without `--iterate`
([cargo-mutants: Iterating on missed mutants](https://mutants.rs/iterate.html)).

Limit the agent to three or four attempts for one mutant group. Then require a
person to decide whether the obstacle is a weak oracle, an equivalent mutant,
or an unsuitable test boundary. This limit is an inference from the study in
which most successful LLM-generated tests appeared in the first three or four
iterations
([Straubinger et al.](https://arxiv.org/abs/2503.08182)).

### 6. Control machine time

Start local parallel work conservatively with two jobs:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest -j2
```

The official guide recommends starting with `-j2` or `-j3`. More jobs can use
large amounts of CPU, memory, and disk, and can cause false timeouts or flakes
in non-hermetic tests
([cargo-mutants: Parallelism](https://mutants.rs/parallelism.html)).

The default test timeout is five times the baseline time, with a minimum of 20
seconds. Use `--timeout SECONDS` only after a real test needs more time; do not
increase it only to hide a hang
([cargo-mutants: Hangs and timeouts](https://mutants.rs/timeouts.html)).

For CI, divide a full run across machines with `--shard K/N`. The cargo-mutants
guide recommends enough work for at least ten mutants per worker and gives 8 to
32 shards as a starting range. Each shard repeats setup and baseline costs, so
more shards have diminishing returns
([cargo-mutants: Sharding](https://mutants.rs/shards.html)).

### 7. Use two run levels

Use a changed-code run for normal feature work:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest \
  --in-diff path/to/change.diff
```

`--in-diff` tests mutants that overlap changed Rust regions. The official guide
warns that this can miss a loss of coverage outside the diff and that test-only
changes select no mutants
([cargo-mutants: Testing code changed in a diff](https://mutants.rs/in-diff.html)).

Run the complete `make mutants` target on a schedule or before a high-risk
release. Keep `mutants.out` as the review artifact for that run. It contains
the mutation list, diffs, logs, outcome JSON, and outcome text files
([cargo-mutants: The `mutants.out` directory](https://mutants.rs/mutants-out.html)).

## Stopping rules

Stop work on a group when one of these conditions is true:

- The important mutants are caught by reviewed tests at the public boundary.
- A person records that a remaining mutant is equivalent or has no valuable,
  stable oracle.
- Three or four agent attempts do not produce a clear test.
- The next test would assert a private implementation detail.
- The patch becomes difficult to review as one logical change.
- The test becomes flaky, slow, or dependent on external state.

Do not use 100% mutation score as the only stopping rule. Run the focused set
without `--iterate`, run `make check`, review the patch, and then move to the
next important area. After several focused groups, use one full run to find the
next priority cluster.

## Short answer

An LLM is useful as a mutation-guided test author, but not as an autonomous
mutation-score optimizer. Let it run one small group, make it explain the
contract, inspect the test oracle, and stop after a few failed attempts. Use
`--iterate` for fast local feedback and a final non-iterative run for evidence.
