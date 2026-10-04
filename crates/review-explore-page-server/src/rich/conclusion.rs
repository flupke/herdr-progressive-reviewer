//! The conclusion of the rich data set: a long summary, a list of ten tasks to implement, future
//! work, and a quiz of three items with their proofs.

use review_explore::{Conclusion, Interpretation, QuizItem, SourceSide, TopicStatus};
use review_explore_page::TurnResponse;

use super::change::{QUEUE, REPLY};

const SUMMARY: &str = "\
The change batches the reviewer's replies: the agent hears about them in one notification \
after two seconds without a new reply, at once when twenty replies wait, and when the pane \
closes. Each reply is still saved to its thread before it is queued, so batching delays only \
the notification, never the reply itself.

**Limitations.** The idle delay is measured on the pane's tick, every 250 ms, so a lone reply \
reaches the agent between two and two and a quarter seconds after it was written. While the \
pane is in the background, the tick may slow down, and with it the flush; the round did not \
check how much.

**Remaining uncertainty.** Nobody measured how often reviewers pause for less than two seconds \
between replies, which decides how much the batching saves. The agent's notification does not \
yet say how many replies it carries or why it was sent, so the agent cannot tell a batch cut \
short by the cap from a complete one.";

const TO_BE_IMPLEMENTED: &str = "\
1. Name the flush reason in the agent's notification, so the agent can tell a pause from a full queue and from a closing pane.
2. Say in the notification how many replies it carries, and from how many threads.
3. Keep the pane's tick running while the pane is in the background, or flush the queue when the pane loses focus.
4. Flush the queue before the pane reloads its threads after a revision change, so that no reply waits across revisions.
5. Add a test that twenty replies in a row send one notification, at the twentieth push.
6. Add a test that a lone reply goes out on the first tick after two seconds, with a fake clock rather than a sleep.
7. Add a test that closing the pane sends the waiting replies once, and that an empty queue sends nothing.
8. Log each flush with its reason and its size, at debug level.
9. Document the two seconds and the twenty replies in the usage guide, under Comment threads.
10. Remove `FlushPolicy::immediate`, which nothing calls any more.";

const FUTURE_WORK: &str = "\
Make the idle delay and the size cap settings once a reviewer asks for other numbers. Show the \
number of waiting replies in the pane's footer, which would also tell the reviewer that the \
agent has not heard about them yet.";

/// The conclusion: with its quiz when `quiz`, or else with the reason it has none.
pub(super) fn conclusion(quiz: bool) -> Conclusion {
    Conclusion {
        summary: SUMMARY.into(),
        to_be_implemented: TO_BE_IMPLEMENTED.into(),
        future_work: FUTURE_WORK.into(),
        quiz: if quiz { quiz_items() } else { Vec::new() },
        quiz_empty_reason: (!quiz).then(|| {
            "The round settled the timing questions already; a quiz would repeat them.".into()
        }),
    }
}

/// What the agent says back to the reviewer's last answer above its conclusion.
pub(super) fn conclusion_response() -> TurnResponse {
    TurnResponse {
        interpretations: vec![Interpretation {
            answer: "previous-answer".into(),
            status: TopicStatus::Accepted,
            recap: "**Keep the cap of twenty replies a constant** until a reviewer asks \
                    for another number."
                .into(),
            follow_ups: Vec::new(),
        }],
        reply: Some(
            "That settles the last open topic. The conclusion below lists what the change still \
             needs before it can merge."
                .into(),
        ),
    }
}

fn quiz_items() -> Vec<QuizItem> {
    vec![
        QuizItem {
            question: "A reviewer answers five threads quickly, less than a second between \
                       replies, then stops to read the next file. How many times is the agent \
                       notified?"
                .into(),
            answers: vec![
                "Once, about two seconds after the fifth reply".into(),
                "Five times, once for each reply".into(),
                "Not until the reviewer closes the pane".into(),
            ],
            correct: 0,
            why: "The queue goes out after two seconds without a new reply, so a quick run of \
                  answers travels together."
                .into(),
            proof: vec![QUEUE.evidence(
                SourceSide::New,
                Some((31, 38)),
                "The tick sends the queue once the reviewer has been quiet long enough.",
            )],
            level: "Timing: when the agent hears about replies.".into(),
        },
        QuizItem {
            question: "The machine crashes one second after the reviewer wrote a reply. What is \
                       lost?"
                .into(),
            answers: vec![
                "The reply itself, which was only in the queue".into(),
                "Only the agent's notification; the reply is in its thread".into(),
                "Nothing, since the notification went out at once".into(),
            ],
            correct: 1,
            why: "A reply is saved to its thread before it is queued; only the queue lives in \
                  memory."
                .into(),
            proof: vec![REPLY.evidence(
                SourceSide::New,
                Some((14, 17)),
                "The thread saves the reply, then the queue takes it.",
            )],
            level: "Durability: what is saved before the queue.".into(),
        },
        QuizItem {
            question: "A reviewer writes twenty-five replies without ever pausing for two \
                       seconds. When does the agent first hear about them?"
                .into(),
            answers: vec![
                "At the twentieth reply".into(),
                "At the twenty-fifth reply".into(),
                "Two seconds after the last reply".into(),
            ],
            correct: 0,
            why: "The size cap sends the queue at once when twenty replies wait, pause or not."
                .into(),
            proof: vec![QUEUE.evidence(
                SourceSide::New,
                Some((22, 29)),
                "A full queue goes out on the push that fills it.",
            )],
            level: "Limits: the size cap.".into(),
        },
    ]
}
