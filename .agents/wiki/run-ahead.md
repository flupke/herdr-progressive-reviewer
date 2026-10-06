# Run-ahead

How run-ahead prepares the next turn while a question waits: the fork processes, their records, the checks an answer must pass to continue as a fork, the switch, settling, failures, and the tests.

[Explore settings](explore-settings.md) says what run-ahead does for the reviewer and what it
costs; this page says how it works.

Run-ahead forks the pane agent's session while a question waits (setting `RunAhead` in
[`crates/review-explore-round-settings`](../../crates/review-explore-round-settings), pane key
`z`, saved under `explore_round` in `settings.json`, read after each input of the session:
turned off, it discards the forks that run). A bare answer continues as the fork of its
choice, when that fork submitted its turn (below); any other answer runs the plain chain. The
pieces:

- [`crates/agent-fork`](../../crates/agent-fork): a fork process that does not outlive the
  reviewer. `Launcher` starts every fork from one thread that lives as long as it, through
  `reviewer-control fork-exec <reviewer pid> <program> <arguments>`, which arms
  `PR_SET_PDEATHSIG(SIGTERM)` (the signal fires when that thread ends), checks that the reviewer
  still runs, then runs the program in its own place. A fork stays in the reviewer's process
  group, so the hang-up of the pane's terminal reaches it too: never give it a process group or
  a session of its own. `RunningFork::terminate` sends SIGTERM, then SIGKILL three seconds later.
  `ProcessStamp` (process ID and start time) names a process across a restart;
  `stop_recorded` stops a fork a stopped reviewer left, when its command line still holds its
  session ID. Only Linux has the parent-death signal: elsewhere, macOS included, `fork-exec`
  refuses and `ClaudeForks` takes no fork, so every answer runs the plain chain, with the
  reason.
- [`crates/review-run-ahead`](../../crates/review-run-ahead): the records saved beside each round
  (`RoundForks`, `ForkRecord`: question, choice, the answer ID the fork was told, session ID
  chosen before the start, the session it was taken from, transcript directory, the reviewer's
  and the fork's `ProcessStamp`, the kept turn until the fork is discarded, its end and tokens,
  why it was discarded, whether it is cleaned up; `RoundForks` also counts the forks that
  failed in a row), and `ForkHost`, the interface to the agent whose session is forked.
- [`crates/claude-fork`](../../crates/claude-fork): `ClaudeForks`, the `ForkHost` for Claude Code.
  It reads the pane agent's process from `/proc` (program, arguments without those that pick a
  session, an interactive mode or a prompt, working directory, environment without `HERDR_*`;
  `arguments.rs` lists every option of `claude` with its values, and an option it does not know,
  or a `--settings` of the agent's own, means no fork: a fork starts with the agent's exact flags
  or not at all),
  its transcript (`<config>/projects/*/<session>.jsonl`: the last `user` or `assistant` entry
  tells that the session moved, the last assistant entry gives the model), and starts
  `claude <pane arguments> -p --resume <session> --fork-session --session-id <new>
  --output-format stream-json --verbose --settings <hook> --allowedTools <the two submit tools>
  --model <model>`. The hook is `reviewer-control fork-guard`: no tool that writes, no MCP tool
  but the two submits, and only reading shell commands; a hook keeps the tool list, and so the
  prompt cache, as the agent's. A fork's tokens are the sum of its own assistant messages, by
  message ID. Herdr's `pane.agent_status_changed` subscription tells when the agent is idle
  (`HerdrClient::forward_agent_status_while` reports the current status once subscribed, then
  each change); nothing polls. Dropping `ClaudeForks` lets the discards under way finish.
- `review-explore-session/src/run_ahead/`: after every input, the session watches the question
  that waits (`run_ahead_reconcile`) and discards the forks of one that no longer does. Forks are
  taken once the agent is idle, again when it worked and its session moved; each gets the prompt
  `PreparedTurn` writes for its choice, with its own request, answer and access value, and the
  unreviewed lines as they will be after the answer (the question's marks applied to a copy of
  the review marks). A fork's submit is checked on a copy of the round and kept in
  `<round>.forks.json` beside the round; any call with a discarded fork's access is refused.
  Answer, Cancel answer, Reset, a new round, turning run-ahead off and closing the reviewer
  discard the forks; a reopened reviewer discards the forks a stopped reviewer left in the
  rounds of every review of the repository (`ReviewStore::rounds_with_forks`; forks of a
  reviewer that still runs stay its own). A question that forks of another running reviewer
  answer is not forked again: the record is checked under its lock before the forks start. The
  state watcher ignores `.forks.json`, as it ignores editor views.
- Fork failures. A fork that ends, or cannot start, without a kept turn before it is discarded
  failed (it crashed, was rate-limited, never submitted, or submitted only turns the tool
  refused): its choice is not prepared, and the log and its record say how it ended. After
  `FAILURES_TO_HALT` (3) failures in a row, `RoundForks::halted_at_ms` stops run-ahead for the
  round: its forks are discarded (`DiscardReason::TooManyFailures`), the pane shows a notice,
  and every later answer of the round runs the plain chain (`PlainReason::NoForks` with the
  reason). A kept turn starts the count again.
- `review-explore-session/src/run_ahead/answer.rs` and `switch.rs`: what an answer to a watched
  question does. `deliver_turn` saves the answer and applies its marks as always, then asks
  `run_ahead_answer`, which records the path and its reason (`TurnPath`, `PlainReason`) in
  `RoundForks::answers` and discards the forks the answer does not continue as.
  - The checks. The answer continues as the fork of its choice only when that turn is exactly
    the one the pane's agent would take for it: a choice (not None of the above) with no comment
    (white space is none, in the prompt too), a fork that submitted, no message of the reviewer
    in the round's conversation, under the question or since it was asked, that the forks do not
    hold (one posted once they were taken, or one the agent has not answered: a talk the agent
    answered moved its session, and the forks taken again after it hold it), the agent idle with
    an empty input box (`ForkHost::input_is_empty`), on the session the forks were taken from, whose
    last entry did not move, and a prompt and unreviewed diffs that are the fork's but for their
    identities (request, answer, access, diffs directory). Anything else runs the plain chain.
  - The switch. The turn's dispatch begins (the page shows the agent working) and
    `ForkHost::switch` runs on a thread of its own. `ClaudeForks` (`claude-fork/src/switch.rs`)
    waits until the fork's stream shows the answer to its submit (at most 10 s, or until the
    fork ends), stops the fork and keeps its transcript, checks again that the agent is idle
    with an empty box, submits `/resume <session>` through `agent.prompt` without a wait, then
    asks Herdr every 100 ms, for at most 20 s, until it reports the agent on that session, idle,
    with an empty box: Herdr sends no event when a session changes.
  - After the switch. The session pins the agent on that session (`PinnedAgent::new`) and
    records it as `last_agent_session`, commits the fork's turn under the answer's identities
    through the agent's own commit path, then records the dispatch as delivered. The agent's
    access and diffs are the fork's, as after a prompt. The question shows only once the switch
    is done; a turn saved meanwhile (after a Cancel answer, say) waits, and its prompt goes out
    once the switch ends, to the session the agent then runs.
  - A failure. A switch that fails, or a fork's turn that cannot be saved, marks the turn failed
    with the reason, and Retry runs the plain chain. A fork the agent was told to resume keeps its
    transcript (`Continuation::Failed { typed: true }`); a fork whose session may be the agent's
    is never discarded nor cleaned up.
  - Settling (`settle.rs`). The agent's session must match the round: a session that holds a
    turn the round does not have would take the answer a second time. When the agent may run a
    fork's session whose turn the round did not take (the switch failed once `/resume` was
    typed, which Claude Code may still carry out late; the reviewer stopped waiting, cancelled
    the answer or reset during the switch; the fork's turn was not saved; a reviewer stopped
    during the switch, which leaves the record `Switching`), `ForkHost::resume` has the agent
    resume the session the fork was taken from (`ForkRecord::from`), then the fork is discarded
    (`Continuation::Undone`). Claude Code takes what is typed in its pane in order, so once that
    `/resume` is typed and Herdr reports the agent there, an earlier late resume cannot move it.
    This rests on Claude Code taking what is typed while it loads a session after it, in order;
    it is not checked.
    When the round did take the fork's turn (a reviewer stopped between saving it and recording
    the switch), the agent resumes the fork's session instead. It settles at once after the
    switch, before a turn's prompt goes out (`run_ahead_hold`) and before forks are taken. A
    turn saved meanwhile waits; when the agent cannot be settled, the turn waits for Retry,
    which tries again, and no prompt reaches it. A resume Herdr did not confirm is not typed
    again without the reviewer asking.
  - No prompt reaches the agent while it switches or settles: the session holds
    `PromptSender::hold`, which keeps the thread service's courier, and so the round
    conversation's wakeups and the prompts to every other agent, from sending until the switch
    or the settling ends. The switch or the settling starts on a thread of its own once a prompt
    the courier was sending is sent (`PromptHold::drained`), so the session never waits.
  - Cancel answer after a prepared turn. The round forgets the answer and the fork's turn as
    for any turn. The agent knows that answer by the ID its fork was told, so the next prompts
    name it so in their `Cancelled answer:` lines (`RoundForks::answer_as_told`).
  - What the reviewer sees. The path of each answer's turn (`TurnPath`, `PlainReason`, in the
    tiny crate [`crates/review-turn-path`](../../crates/review-turn-path), which the records, the
    pane's events and the page share) reaches the page as `TurnResponse::path` and the pane as
    `ExploreTurnPath` (and `ExploreRestored::turn_paths`, from `RoundForks::turn_paths`): a
    prepared turn once the agent runs its fork's session, a plain chain as soon as the answer is
    recorded. Both show `TurnPath::line`, the one place of its wording, with the turn: the dim
    line under the question or conclusion in the pane, the previous turn's foot on the page.
    "Prepared while you were thinking", or "Not prepared: " and the reason in plain words. A
    check that could not be made says nothing there (`Unchecked` is in the log), nor does a
    question run-ahead did not watch (run-ahead off). A failed switch (`SwitchFailed`) and one
    the reviewer stopped waiting for (`Withdrawn`) leave the turn waiting for Retry, with the
    failure's reason; the turn Retry brings says why it was not prepared. The standalone server's `question-not-prepared`
    step and the gallery's `question-2-not-prepared` state show a plain chain's line.

Tests: `review-explore-session` checks the session with a fake `ForkHost` (prompts, access,
discards, records, each reason of the plain chain and the path the pane and the page are given,
forks taken again after a talk in the chat or in the pane and used by the next bare answer, a
switch, a failed one, a turn held during a switch, each failure of a fork and the stop after too
many, Stop waiting, Retry, Cancel answer and Reset during a switch, from the pane and the page,
settling after a reopen, another reviewer's forks); `reviewer` checks real forks on an isolated
Herdr with a forkable Claude Code stand-in (`runtime/run_ahead.tests.rs`; while
`prompt.unreported-resumes` exists in the server's directory, the stand-in does not report its
resumes to Herdr): the stand-in in the pane takes `/resume <session>` as Claude Code does, reporting the session to
Herdr as Claude Code's session hook does (with a newer `--seq` and `--session-start-source
resume`; without them Herdr kept reporting the first session), and a fork stand-in prints its submit's answer
once the test submitted for it; and the parent-death signal (`tests/fork_lifetime.rs`). Two tests
run real Claude Code turns on your subscription, at `claude-sonnet-5-5`, and are ignored by
default:

```sh
cargo build -p reviewer --bin reviewer-control -p review-mcp-config --bin reviewer-mcp
cargo nextest run -p reviewer a_real_claude_code --run-ignored only
```

The second of them, `a_real_claude_code_agent_continues_as_the_fork_of_a_bare_answer`, answers
the first question bare with a choice whose fork submitted, checks that the pane resumed the
fork's session and that the fork's turn is the round's, then sends the next answer and has the
real agent take it in the fork's session (`claude -p --resume <fork>`).

They write the agent's and the forks' transcripts in your Claude Code configuration and delete
them at the end.
