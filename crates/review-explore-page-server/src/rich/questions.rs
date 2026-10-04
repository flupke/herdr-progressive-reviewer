//! The questions of the rich data set, as long as a real round's, in turn:
//!
//! 1. a two-way question with several paragraphs of Context, a table, a diagram, a sketch, four
//!    long choices and three citations;
//! 2. a one-way question, whose recommendation the page hides until the reviewer's first pick;
//! 3. a question with a diagram Mermaid cannot parse and a citation of a file outside the change.

use review_explore::{
    Alternative, Assessments, Consequence, Door, EvidenceRef, Interpretation, NotRelevantMark,
    NotRelevantReason, Question, SourceSide, TopicStatus,
};
use review_explore_page::{QuestionMarks, TurnResponse};

use super::change::{FLUSH, QUEUE, REPLY};
use crate::question_parts;

/// The fixed question `number`, from 1, in turn, and the lines an answer to it marks.
pub(super) fn question(number: usize) -> (Question, QuestionMarks) {
    match (number - 1) % 3 {
        0 => (flush_rule(), flush_rule_marks()),
        1 => (closing(), closing_marks()),
        _ => (size_setting(), QuestionMarks::default()),
    }
}

/// What the agent says back to the reviewer's answer to the previous question before its
/// question `number`, from 2: the rule while the pane is open before question 2, then what
/// happens on close.
pub(super) fn answer_response(number: usize) -> TurnResponse {
    if number.is_multiple_of(3) {
        return closing_response();
    }
    TurnResponse {
        interpretations: vec![Interpretation {
            answer: "previous-answer".into(),
            status: TopicStatus::NeedsFollowUp,
            recap: "**Send the queue after two seconds of quiet, or at once at twenty \
                    replies**, as the change does."
                .into(),
            follow_ups: vec![
                "Check that `ReplyQueue::tick` still runs while the pane is in the background."
                    .into(),
                "Say in the agent's notification how many replies it carries.".into(),
            ],
        }],
        reply: Some(
            "Agreed. The two numbers stay constants in `FlushPolicy::DEFAULT` for now. Whether \
             they should become settings depends on what happens when the pane closes, so I ask \
             about that first: a setting would have to cover both."
                .into(),
        ),
        path: None,
    }
}

/// What the agent says back to the reviewer's answer about the queue on close.
fn closing_response() -> TurnResponse {
    TurnResponse {
        interpretations: vec![Interpretation {
            answer: "previous-answer".into(),
            status: TopicStatus::Accepted,
            recap: "**Send the waiting replies in one notification when the pane \
                    closes**."
                .into(),
            follow_ups: Vec::new(),
        }],
        reply: Some(
            "Agreed: every reply is in its thread already, so the last notification only makes \
             sure the agent hears about them. One question is left, about the size cap."
                .into(),
        ),
        path: None,
    }
}

const FLUSH_RULE_CONTEXT: &str = "\
Before this change, every reply woke the agent at once. A reviewer who answers a dozen threads \
in a row started a dozen agent turns, each reading the whole conversation again, and the agent \
often answered the first thread while the reviewer was still writing the fifth.

The change puts the replies in a queue and lets a policy decide when the queue goes out. The \
policy sends the queue after two seconds without a new reply, or at once when twenty replies \
wait. Both numbers are constants in `FlushPolicy::DEFAULT`.

The question is whether that rule fits how reviewers write. Two seconds is shorter than the \
time most reviewers take to read the next thread, so a reviewer who reads before answering \
still sends one reply at a time; one who answers quickly in a row gets the batching.

| Rule | A burst of 12 replies in 10 s | One reply, then a minute of reading | Agent turns |
| --- | --- | --- | --- |
| Idle 2 s or 20 replies (the change) | [!good] One notification | [!good] Sent after 2 s | [!good] 1 |
| Merge within 100 ms | [!bad] Twelve notifications | [!good] Sent at once | [!bad] 12 |
| Only on Send all | [!good] One notification | [!warning] Waits for the button | [!warning] 1, if remembered |
| Settings, same defaults | [!good] One notification | [!good] Sent after 2 s | [!good] 1 |

The size cap matters only for a long burst: without it, a reviewer who never pauses for two \
seconds would hold every reply back until they stop.

```mermaid
flowchart LR
  empty[Empty queue] -->|push| waiting[Replies waiting]
  waiting -->|20th push| full{{Sent: Full}}
  waiting -->|2 s without a push| idle{{Sent: Idle}}
  waiting -->|the pane closes| closing{{Sent: Closing}}
  full --> empty
  idle --> empty
```

A push that leaves fewer than twenty replies waiting keeps the queue as it is.

What happens on close is a question of its own; this one is only about the rule while the pane \
is open.";

const FLUSH_RULE_SKETCH: &str = "\
```text
replies   ▲  ▲ ▲   ▲                    ▲
time      0s 0.6s  1.4s      3.4s       9s      11s
queue     [1][2][3][4] ───── sent (Idle) [5] ── sent (Idle)
```";

/// When the queue goes out while the pane is open: the rule of the change, recommended.
fn flush_rule() -> Question {
    Question {
        rationale: Some(FLUSH_RULE_CONTEXT.into()),
        visual: Some(FLUSH_RULE_SKETCH.into()),
        assessments: Some(Assessments {
            door: Door::TwoWay,
            reversibility: Consequence {
                summary: "Both numbers are constants in one place; changing them later touches \
                          one line and no saved data."
                    .into(),
                details: "The queue lives in memory only, so a later rule needs no migration: \
                          a pane started after the change simply follows the new rule."
                    .into(),
                evidence: vec![FLUSH.evidence(
                    SourceSide::New,
                    Some((20, 21)),
                    "The whole rule is this constant.",
                )],
                unknowns: Vec::new(),
            },
            blast_radius: Consequence {
                summary: "Every reviewer who writes replies while the agent works: the agent \
                          hears about their replies up to two seconds later."
                    .into(),
                details: String::new(),
                evidence: Vec::new(),
                unknowns: vec![
                    "How often reviewers pause for less than two seconds between replies.".into(),
                    "Whether an agent's prompt cache survives a batch as well as single replies."
                        .into(),
                ],
            },
        }),
        ..question_parts::question(
            "flush-rule",
            "batching",
            "When should the replies waiting in the queue reach the agent, now that a reply no \
             longer goes out the moment the reviewer writes it?",
            vec![
                Alternative {
                    recommendation: Some(
                        "It batches a quick run of answers into one agent turn, and \
                         `ReplyQueue::tick` still sends a lone reply within two seconds, so a \
                         reviewer who reads between answers sees no delay."
                            .into(),
                    ),
                    ..question_parts::alternative(
                        "idle-or-full",
                        "Send the queue after two seconds without a new reply, or at once when \
                         twenty replies wait, as `FlushPolicy::DEFAULT` does",
                        TopicStatus::Accepted,
                    )
                },
                question_parts::alternative(
                    "merge",
                    "Keep sending each reply at once, but merge the replies that arrive within \
                     the same 100 milliseconds into one notification",
                    TopicStatus::NeedsFollowUp,
                ),
                question_parts::alternative(
                    "send-all",
                    "Hold every reply until the reviewer presses Send all, and show the number of \
                     waiting replies in the pane's footer",
                    TopicStatus::NeedsFollowUp,
                ),
                question_parts::alternative(
                    "settings",
                    "Make the `idle` delay and the `cap` of `FlushPolicy` settings of the \
                     reviewer, with two seconds and twenty replies as their defaults",
                    TopicStatus::NeedsFollowUp,
                ),
            ],
            vec![
                QUEUE.evidence(
                    SourceSide::New,
                    Some((22, 38)),
                    "A push only queues the reply, unless the queue is full; the pane's tick \
                     sends it once the reviewer has been quiet for the policy's delay. This is \
                     the behaviour the question is about.",
                ),
                FLUSH.evidence(
                    SourceSide::New,
                    Some((19, 30)),
                    "The two numbers, two seconds and twenty replies, and the two checks the \
                     queue asks. Nothing else reads them.",
                ),
                QUEUE.evidence(
                    SourceSide::Old,
                    Some((17, 21)),
                    "Before the change, a push sent the reply to the agent at once: one \
                     notification, and one agent turn, per reply.",
                ),
            ],
        )
    }
}

/// What an answer to the question about the queue on close marks: the closing code of the
/// threads reviewed, the queue's opening lines not relevant.
fn closing_marks() -> QuestionMarks {
    QuestionMarks {
        reviewed: vec![question_parts::lines(REPLY.path, 16, 27)],
        not_relevant: vec![NotRelevantMark {
            location: question_parts::lines(QUEUE.path, 1, 3),
            reason: Some(NotRelevantReason::FollowsCode),
            test: None,
        }],
        reopened: Vec::new(),
    }
}

/// The queue's file reviewed, and the reply's call site not relevant to the decision.
fn flush_rule_marks() -> QuestionMarks {
    QuestionMarks {
        reviewed: vec![
            question_parts::lines(QUEUE.path, 22, 38),
            question_parts::lines(FLUSH.path, 19, 30),
        ],
        not_relevant: vec![NotRelevantMark {
            location: question_parts::lines(REPLY.path, 1, 5),
            reason: Some(NotRelevantReason::FollowsCode),
            test: None,
        }],
        reopened: Vec::new(),
    }
}

const CLOSING_CONTEXT: &str = "\
When the reviewer closes the pane, some replies may still wait in the queue: the reviewer \
wrote them less than two seconds ago. Each one is already saved in its thread; what is at stake \
is whether the agent hears about them.

The change sends them in one last notification, with the reason `Closing`, before the pane \
saves its threads. The pane then closes at once: it does not wait for the agent to answer.

| Choice | The agent hears about the last replies | The pane closes |
| --- | --- | --- |
| Send them on close (the change) | [!good] At once | [!good] At once |
| Send them when the pane opens again | [!warning] Hours later, or never | [!good] At once |
| Drop them, and say how many | [!bad] Never | [!good] At once |";

/// What happens to the queue on close: hard to reverse, so the recommendation waits for the
/// reviewer's first pick.
fn closing() -> Question {
    Question {
        rationale: Some(CLOSING_CONTEXT.into()),
        assessments: Some(Assessments {
            door: Door::OneWay,
            reversibility: Consequence {
                summary: "A notification that was dropped, or that waits for a pane that never \
                          opens again, cannot be sent later: the queue that held it is gone."
                    .into(),
                details: String::new(),
                evidence: vec![REPLY.evidence(
                    SourceSide::New,
                    Some((20, 27)),
                    "The last moment the queue exists.",
                )],
                unknowns: Vec::new(),
            },
            blast_radius: Consequence {
                summary: "A reviewer who closes the pane expecting the agent to work on their \
                          last replies overnight."
                    .into(),
                details: String::new(),
                evidence: Vec::new(),
                unknowns: vec!["How many reviewers close the pane right after replying.".into()],
            },
        }),
        ..question_parts::question(
            "closing",
            "batching",
            "What should happen to the replies still waiting in the queue when the reviewer \
             closes the pane?",
            vec![
                Alternative {
                    recommendation: Some(
                        "Every reply is already saved in its thread; sending the queue on close \
                         only makes sure the agent hears about the last ones before the reviewer \
                         leaves."
                            .into(),
                    ),
                    ..question_parts::alternative(
                        "send-on-close",
                        "Send them in one last notification before the pane closes, as the \
                         change does",
                        TopicStatus::Accepted,
                    )
                },
                question_parts::alternative(
                    "send-next-open",
                    "Keep them in the thread files and tell the agent about them the next time \
                     the pane opens",
                    TopicStatus::NeedsFollowUp,
                ),
                question_parts::alternative(
                    "drop",
                    "Drop the waiting notifications, and show how many were dropped as the pane \
                     closes",
                    TopicStatus::NeedsFollowUp,
                ),
            ],
            vec![
                REPLY.evidence(
                    SourceSide::New,
                    Some((20, 27)),
                    "Closing the threads sends the queue first, with the reason Closing, then \
                     saves each thread.",
                ),
                QUEUE.evidence(
                    SourceSide::New,
                    Some((40, 50)),
                    "A flush of an empty queue sends nothing, so a pane closed with no reply \
                     waiting makes no call to the agent.",
                ),
            ],
        )
    }
}

const SIZE_SETTING_CONTEXT: &str = "\
The cap of twenty replies is a constant. A reviewer who wants the agent to hear about every \
reply at once cannot ask for a cap of one.

The settings file is read once when the pane starts, so a setting would apply to the next pane, \
not to the one open:

```mermaid
flowchart LR
  settings[settings.toml] --> pane[pane start (reads it once)]
  pane --> policy[FlushPolicy]
```

No reviewer has asked for another number yet.";

/// Whether the cap becomes a setting: its sketch does not parse, and it cites a file outside
/// the change.
fn size_setting() -> Question {
    Question {
        rationale: Some(SIZE_SETTING_CONTEXT.into()),
        assessments: Some(Assessments {
            door: Door::TwoWay,
            reversibility: Consequence {
                summary: "A setting can be added later without breaking anything; removing \
                          one later would ignore what reviewers wrote."
                    .into(),
                details: String::new(),
                evidence: Vec::new(),
                unknowns: vec!["Whether any reviewer wants another number.".into()],
            },
            blast_radius: Consequence {
                summary: "Only reviewers who change the setting.".into(),
                details: String::new(),
                evidence: Vec::new(),
                unknowns: vec!["How a cap of one would interact with the idle delay.".into()],
            },
        }),
        ..question_parts::question(
            "size-setting",
            "batching",
            "Should the size cap of twenty replies be a setting of the reviewer?",
            vec![
                question_parts::alternative(
                    "constant",
                    "Keep twenty as a constant until a reviewer asks for another number",
                    TopicStatus::Accepted,
                ),
                question_parts::alternative(
                    "setting",
                    "Read it from the reviewer's settings file, with twenty as the default",
                    TopicStatus::NeedsFollowUp,
                ),
            ],
            vec![
                FLUSH.evidence(
                    SourceSide::New,
                    Some((27, 29)),
                    "The only place the cap is read.",
                ),
                EvidenceRef {
                    location: question_parts::lines("config/settings.toml", 1, 12),
                    notes: "The reviewer's settings, which have no notification section yet."
                        .into(),
                },
            ],
        )
    }
}
