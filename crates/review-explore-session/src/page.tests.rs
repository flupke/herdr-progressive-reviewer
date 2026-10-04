//! The stage of the round the session publishes for the Explore page.

use review_explore::Command;
use review_explore::{DispatchState, QuizStage, Step, StepState, TabTitle};
use review_explore_page::{
    CommandRefusal, CommandReply, ImplementationState, Interruption, PageAnswer, PageCommand,
    PageImplement, PageImplementation, ReviewName, RoundStage,
};
use review_repository::diff::DiffRow;

use super::*;

/// The number and text of the question the page shows, or `None` in another stage.
fn shown_question(stage: &RoundStage) -> Option<(usize, String)> {
    match stage {
        RoundStage::Question {
            number, question, ..
        } => Some((*number, question.id.clone())),
        _ => None,
    }
}

#[test]
fn the_page_follows_the_round_from_its_kickoff_to_its_conclusion_and_reset() {
    let mut harness = Harness::start();
    assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));

    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );

    let (answer, access) = harness.answer("Keep it.");
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((2, "q2".into()))
    );

    let (answer, access) = harness.answer("Keep it too.");
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    let RoundStage::Conclusion {
        request,
        conclusion: shown,
        implementation,
        quiz,
        ..
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(request, answer.request);
    assert_eq!(shown.summary, CONCLUSION);
    // Each answered question, with the answer the reviewer kept.
    let overview = harness.page.overview().expect("an overview");
    let kept: Vec<_> = overview
        .decisions
        .iter()
        .map(|decision| (decision.number, decision.answer.comment.as_str()))
        .collect();
    assert_eq!(kept, vec![(1, "Keep it."), (2, "Keep it too.")]);
    assert_eq!(implementation, None);
    let no_quiz = review_explore_page::PageQuiz {
        takes_answers: true,
        ..Default::default()
    };
    assert_eq!(quiz, no_quiz);

    harness.session.handle(Input::Command(Command::Reset));
    assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));
}

#[test]
fn the_page_shows_a_turn_the_agent_no_longer_works_on_as_interrupted() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);

    harness.session.handle(Input::Command(Command::Cancel));

    assert!(matches!(
        harness.page.stage(),
        RoundStage::Interrupted {
            request: Some(_),
            interruption: Interruption::Stopped,
            ..
        }
    ));
}

#[test]
fn a_reopened_reviewer_shows_its_restored_round_on_the_page() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));

    harness.reopen();
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );

    harness.answer("Keep it.");
    harness.reopen();
    assert!(
        matches!(
            harness.page.stage(),
            RoundStage::Interrupted {
                request: Some(_),
                interruption: Interruption::Stopped,
                ..
            }
        ),
        "the reopened reviewer no longer waits for the agent's turn"
    );
}

#[test]
fn a_cancelled_answer_brings_its_question_back_to_the_page() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answer, _) = harness.answer("Keep it.");

    let id = answer.answer.unwrap().id;
    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(id)));

    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );
}

/// The recaps and the reply of what the agent said back to the previous answer, above the
/// question or the conclusion the page shows.
fn shown_response(stage: &RoundStage) -> Option<(Vec<String>, Option<String>)> {
    stage.response().map(|response| {
        let recaps = response
            .interpretations
            .iter()
            .map(|interpretation| interpretation.recap.clone())
            .collect();
        (recaps, response.reply.clone())
    })
}

#[test]
fn the_page_shows_what_the_agent_said_back_to_the_previous_answer() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        shown_response(&harness.page.stage()),
        Some((Vec::new(), Some("I checked the policy.".into()))),
        "the first question follows no answer, only the kickoff's reply"
    );

    let (answer, access) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        shown_response(&harness.page.stage()),
        Some((
            vec!["Keep the policy.".into()],
            Some("I checked the policy.".into())
        ))
    );

    let (answer, access) = harness.answer("Keep it too.");
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    assert_eq!(
        shown_response(&harness.page.stage()),
        Some((vec!["Keep the policy.".into()], None))
    );
}

/// Whether the question the page shows waits again because its answer was cancelled.
fn answer_cancelled(stage: &RoundStage) -> bool {
    match stage {
        RoundStage::Question {
            answer_cancelled, ..
        } => *answer_cancelled,
        other => panic!("the page shows {other:?}"),
    }
}

#[test]
fn a_question_whose_answer_was_cancelled_shows_so_until_the_next_turn() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert!(!answer_cancelled(&harness.page.stage()));

    let (answer, _) = harness.answer("Keep it.");
    harness.session.handle(Input::Command(Command::CancelAnswer(
        answer.answer.unwrap().id,
    )));
    let cancelled = harness.next::<ui_events::ExploreAnswerCancelled>();
    harness.exploration = Some(cancelled.result.unwrap().exploration.clone());
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );
    assert!(answer_cancelled(&harness.page.stage()));

    let (answer, access) = harness.answer("Keep it, again.");
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert!(!answer_cancelled(&harness.page.stage()));
}

#[test]
fn the_page_names_the_review_it_belongs_to_as_the_panes_header_does() {
    let harness = Harness::start();
    assert_eq!(harness.page.review(), None);
    let snapshot = complete_repository_snapshot(&harness.repository);

    harness.session.name_review(&snapshot.identity);

    let repository = harness.repository.root().file_name().unwrap();
    assert_eq!(
        harness.page.review(),
        Some(ReviewName {
            repository: repository.to_string_lossy().into_owned(),
            revision: snapshot.identity.plain_display_id(),
            title: "Git working tree".into(),
        })
    );
}

#[test]
fn the_page_tells_each_round_from_the_next() {
    let mut harness = Harness::start();
    assert_eq!(harness.page.round(), None);

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let round = harness.page.round();
    assert!(round.is_some());
    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        harness.page.round(),
        round,
        "the same round asks its question"
    );

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.round(), None);

    harness.capture();
    let next = harness.request(None);
    harness.turn(&next);
    assert!(harness.page.round().is_some());
    assert_ne!(harness.page.round(), round);
}

#[test]
fn the_page_shows_the_lines_each_citation_of_the_question_names() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);

    assert!(applied(harness.submit(&access, question(&first, 1))));

    let RoundStage::Question { citations, .. } = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    let [citation] = &citations[..] else {
        panic!("the page shows {} citations", citations.len());
    };
    assert_eq!(citation.evidence.location.to_string(), "reviewed.rs new 1");
    assert_eq!(citation.evidence.notes, "Implements the policy");
    let rows = citation.lines.as_ref().unwrap();
    let shown: Vec<_> = rows
        .iter()
        .map(|row| {
            let text: String = row.tokens.iter().map(|token| token.text.as_str()).collect();
            (row.diff.clone(), text)
        })
        .collect();
    assert_eq!(
        shown,
        [(
            DiffRow::Add {
                new_line: 1,
                text: "+pub fn reviewed() {}".into()
            },
            "pub fn reviewed() {}".into()
        )]
    );
    assert!(
        matches!(
            harness.store.load(&harness.unit, b"reviewed.rs").unwrap(),
            review_store::LoadResult::Unreviewed
        ),
        "showing a citation marks none of its lines"
    );
}

#[test]
fn the_page_shows_each_earlier_question_with_the_lines_its_citations_name() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert!(harness.page.earlier_citations().is_empty());

    let (answer, access) = harness.answer("Keep it.");
    assert!(
        harness.page.earlier_citations().is_empty(),
        "the question the agent works on is not an earlier one yet"
    );
    assert!(applied(harness.submit(&access, question(&answer, 2))));

    let overview = harness.page.overview().expect("an overview");
    let numbers: Vec<_> = overview
        .earlier
        .iter()
        .map(|record| record.number)
        .collect();
    assert_eq!(numbers, [1]);
    let citations = harness.page.earlier_citations();
    let [list] = &citations[..] else {
        panic!(
            "the page has citations for {} earlier questions",
            citations.len()
        );
    };
    let [citation] = &list[..] else {
        panic!("the earlier question shows {} citations", list.len());
    };
    assert_eq!(citation.evidence.location.to_string(), "reviewed.rs new 1");
    let rows = citation.lines.as_ref().expect("the cited lines");
    assert_eq!(rows.len(), 1);
}

/// The question citing `evidence`, each `(path, notes)` at line 1 of the new side.
fn question_citing(request: &TurnRequest, evidence: &[(&str, &str)]) -> Operation {
    let Operation::SubmitQuestion(mut update) = question(request, 1) else {
        unreachable!("a question");
    };
    let next = update.next.as_mut().unwrap();
    next.evidence = evidence
        .iter()
        .map(|(path, notes)| {
            serde_json::from_value(serde_json::json!({
                "path": path, "side": "new", "lines": {"first_line": 1, "last_line": 1},
                "notes": notes
            }))
            .unwrap()
        })
        .collect();
    Operation::SubmitQuestion(update)
}

#[test]
fn the_page_shows_no_lines_of_a_file_outside_the_change_and_the_tracked_files() {
    let mut harness = Harness::start();
    harness.files.write(".gitignore", b".env\n");
    harness.files.write("kept.rs", b"pub fn kept() {}\n");
    harness.files.new_change("Track the base files");
    harness
        .files
        .write("reviewed.rs", b"pub fn reviewed() -> bool { true }\n");
    harness.files.write(".env", b"TOKEN=synthetic\n");
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);

    let cited = [
        ("reviewed.rs", "changed"),
        ("kept.rs", "tracked"),
        (".env", "ignored"),
        (".git/config", "repository internals"),
    ];
    assert!(applied(
        harness.submit(&access, question_citing(&first, &cited))
    ));

    let RoundStage::Question { citations, .. } = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    let shown: Vec<_> = citations
        .iter()
        .map(|citation| {
            let rows = citation.lines.as_ref().map_or(0, Vec::len);
            let limitation = citation.lines.as_ref().err().cloned();
            (citation.evidence.notes.as_str(), rows > 0, limitation)
        })
        .collect();
    let untracked = Some(review_explore::Uncitable::Untracked);
    assert_eq!(
        shown,
        [
            ("changed", true, None),
            ("tracked", true, None),
            ("ignored", false, untracked.clone()),
            ("repository internals", false, untracked),
        ]
    );
}

impl Harness {
    /// Send `input` from the Explore page, as the answer to version `version` of question
    /// `question`, and return the session's reply.
    fn answer_on_page(
        &mut self,
        question: &str,
        version: u32,
        input: AnswerInput,
    ) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        let answer = PageAnswer {
            question: question.into(),
            version,
            input,
        };
        self.session.handle(Input::Page {
            command: PageCommand::Answer(answer),
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }

    /// Ask the first question, as the agent.
    pub(super) fn ask_first_question(&mut self) {
        self.capture();
        let first = self.request(None);
        let access = self.turn(&first);
        assert!(applied(self.submit(&access, question(&first, 1))));
    }
}

fn keep(text: &str) -> AnswerInput {
    AnswerInput {
        option: Some("keep".into()),
        text: text.into(),
        ..AnswerInput::default()
    }
}

#[test]
fn an_answer_from_the_page_is_saved_and_prompted_as_one_from_the_pane() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    // What the pane would post for the same pick and comment, from its copy of the round.
    let mut pane = harness.exploration().clone();
    let shown = pane.questions.last().cloned();
    let expected = pane
        .request(Some(keep("Keep it, with a test.")), shown.as_ref())
        .unwrap();

    let reply = harness.answer_on_page("q1", 1, keep("Keep it, with a test."));

    assert_eq!(reply, Ok(()));
    let posted = harness.next::<ui_events::ExplorePosted>();
    assert!(posted.result.is_ok(), "{:?}", posted.result);
    let saved = harness.saved();
    let [answer] = saved.exploration.answers.as_slice() else {
        panic!("saved answers: {:?}", saved.exploration.answers);
    };
    let expected = expected.answer.unwrap();
    assert_eq!(
        (
            &answer.question,
            &answer.in_reply_to,
            &answer.option,
            &answer.text,
            &answer.author
        ),
        (
            &expected.question,
            &expected.in_reply_to,
            &expected.option,
            &expected.text,
            &expected.author
        )
    );
    assert_eq!(posted.request.answer.as_ref(), Some(answer));
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", posted.request.request)));
    assert!(prompt.contains(&format!("Answer ID: {}\n", answer.id)));
    assert!(prompt.contains("Keep it, with a test."));
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));
}

#[test]
fn a_second_answer_from_another_page_or_after_the_pane_is_refused() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert_eq!(harness.answer_on_page("q1", 1, keep("Keep it.")), Ok(()));
    let first = harness.next::<ui_events::ExplorePosted>().request;

    let from_another_page = harness.answer_on_page("q1", 1, keep("Keep it too."));

    assert_eq!(from_another_page, Err(CommandRefusal::Stale));
    let saved = harness.saved();
    assert_eq!(
        saved.exploration.answers,
        vec![first.answer.clone().unwrap()]
    );
    assert_eq!(
        saved.exploration.pending_request().map(|r| &r.request),
        Some(&first.request)
    );

    let mut pane = Harness::start();
    pane.ask_first_question();
    pane.answer("Keep it.");
    let after_the_pane = pane.answer_on_page("q1", 1, keep("Keep it too."));
    assert_eq!(after_the_pane, Err(CommandRefusal::Stale));
    assert_eq!(pane.saved().exploration.answers.len(), 1);
}

impl Harness {
    /// The reviewer's first pick on the page of version `version` of `question`, as the session
    /// replies to it.
    fn pick_on_page(&mut self, question: &str, version: u32) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        let command = PageCommand::Pick {
            question: question.into(),
            version,
        };
        self.session.handle(Input::Page { command, reply });
        replied.blocking_recv().expect("the session replies")
    }
}

#[test]
fn a_first_pick_on_the_page_is_accepted_while_its_question_waits_for_an_answer() {
    let mut harness = Harness::start();
    harness.ask_first_question();

    assert_eq!(harness.pick_on_page("q1", 1), Ok(()));
    assert_eq!(
        harness.pick_on_page("q1", 2),
        Err(CommandRefusal::Stale),
        "the agent posted version 1"
    );

    harness.answer("Keep it.");
    assert_eq!(harness.pick_on_page("q1", 1), Err(CommandRefusal::Stale));
    assert!(
        harness.saved().exploration.answers.len() == 1,
        "a pick saves nothing"
    );
}

#[test]
fn a_repeated_answer_from_the_page_is_applied_once() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert_eq!(harness.answer_on_page("q1", 1, keep("Keep it.")), Ok(()));

    let repeated = harness.answer_on_page("q1", 1, keep("Keep it."));

    assert_eq!(repeated, Err(CommandRefusal::AlreadyApplied));
    assert_eq!(harness.saved().exploration.answers.len(), 1);
}

#[test]
fn a_repeated_start_from_the_page_starts_one_round_and_a_late_one_none() {
    let mut harness = Harness::start();
    let start = harness.offered_start();
    let send = |harness: &mut Harness| {
        let (reply, replied) = CommandReply::channel();
        let kickoff = harness.session.start_from_page(false, &start, reply);
        (replied.blocking_recv().unwrap(), kickoff.is_some())
    };

    assert_eq!(send(&mut harness), (Ok(()), true));
    assert_eq!(
        send(&mut harness),
        (Err(CommandRefusal::AlreadyApplied), false)
    );
    harness.session.handle(Input::Command(Command::Cancel));
    assert_ne!(
        harness.offered_start(),
        start,
        "the next start is another one"
    );
    assert_eq!(send(&mut harness), (Err(CommandRefusal::Stale), false));
}

#[test]
fn a_page_answer_to_a_question_the_agent_moved_past_is_refused() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, access) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&access, question(&answer, 2))));

    assert_eq!(
        harness.answer_on_page("q1", 1, keep("Keep it, but log it.")),
        Err(CommandRefusal::Stale)
    );
    assert_eq!(
        harness.answer_on_page("q1", 1, keep("Keep it.")),
        Err(CommandRefusal::AlreadyApplied),
        "the answer the round has"
    );
    assert_eq!(
        harness.answer_on_page("q2", 2, keep("Keep it.")),
        Err(CommandRefusal::Stale),
        "the agent posted version 1"
    );
    let saved = harness.saved();
    assert_eq!(saved.exploration.answers.len(), 1);
    assert_eq!(saved.exploration.pending_request(), None);
}

#[test]
fn the_page_shows_why_the_prompt_of_its_answer_could_not_be_delivered() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    harness.agents.remove_agent(&PaneId(PANE.into()));

    let reply = harness.answer_on_page("q1", 1, keep("Keep it."));

    assert_eq!(reply, Ok(()), "the answer is saved");
    assert_eq!(harness.saved().exploration.answers.len(), 1);
    assert!(
        matches!(
            harness.page.stage(),
            RoundStage::Interrupted {
                interruption: Interruption::Failed(_),
                ..
            }
        ),
        "the page shows {:?}",
        harness.page.stage()
    );
}

#[test]
fn the_page_shows_the_lines_an_answer_to_its_question_marks() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    // Only a turn that follows an answer may mark lines.
    let (answer, access) = harness.answer("Keep it.");
    let Operation::SubmitQuestion(mut update) = question(&answer, 2) else {
        unreachable!("a question");
    };
    let lines: review_explore::CodeLocation = serde_json::from_value(serde_json::json!({
        "path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1}
    }))
    .unwrap();
    update.reviewed = vec![lines.clone()];
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));

    let RoundStage::Question { marks, .. } = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(marks.reviewed, vec![lines]);
    assert!(marks.not_relevant.is_empty() && marks.reopened.is_empty());
}

#[test]
fn the_page_keeps_the_design_the_first_turn_explained_for_the_rest_of_the_round() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert_eq!(harness.page.design(), None);

    assert!(applied(harness.submit(&access, question(&first, 1))));
    let explained: review_explore::Design = serde_json::from_value(design()).unwrap();
    assert_eq!(harness.page.design().as_deref(), Some(&explained));

    let (answer, access) = harness.answer("Keep it.");
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));
    assert_eq!(harness.page.design().as_deref(), Some(&explained));
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    assert_eq!(harness.page.design().as_deref(), Some(&explained));

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.design(), None);
}

impl Harness {
    /// The start the page offers, or a start no stage offers.
    pub(super) fn offered_start(&self) -> String {
        match self.page.stage() {
            RoundStage::NoRound { start } | RoundStage::StartFailed { start, .. } => start,
            _ => "no-start".into(),
        }
    }

    /// Start a round from the Explore page, and return the session's reply.
    pub(super) fn start_on_page(&mut self, challenger: bool) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        self.session.handle(Input::Page {
            command: PageCommand::Start {
                challenger,
                start: self.offered_start(),
            },
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }

    /// Start a round from the Explore page as the reviewer's worker does, which sends the
    /// kickoff itself; return the reply and the kickoff.
    pub(super) fn start_as_worker(&mut self) -> (Result<(), CommandRefusal>, Option<TurnRequest>) {
        let (reply, replied) = CommandReply::channel();
        let start = self.offered_start();
        let kickoff = self.session.start_from_page(false, &start, reply);
        let reply = replied.blocking_recv().expect("the session replies");
        (reply, kickoff)
    }
}

#[test]
fn a_round_started_from_the_page_is_captured_saved_and_prompted_as_one_from_the_pane() {
    let mut harness = Harness::start();

    assert_eq!(harness.start_on_page(true), Ok(()));

    assert!(harness.next::<ui_events::ExplorePageStart>().0.is_ok());
    let posted = harness.next::<ui_events::ExplorePosted>();
    assert!(posted.request.is_kickoff());
    assert!(posted.request.challenger);
    let round = posted.result.expect("the kickoff is saved");
    assert_eq!(round.exploration.instance, posted.request.instance);
    assert!(round.exploration.challenger);
    assert_eq!(
        round.exploration.comparison.checkpoint.review_unit,
        harness.unit
    );
    harness.exploration = Some(round.exploration.clone());
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", posted.request.request)));
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));
    assert_eq!(
        harness.page.round().as_ref(),
        Some(&posted.request.instance)
    );

    let access = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Explore review access: "))
        .unwrap()
        .to_owned();
    assert!(applied(
        harness.submit(&access, question(&posted.request, 1))
    ));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );
}

#[test]
fn the_page_shows_a_starting_round_until_its_kickoff_is_saved() {
    let mut harness = Harness::start();

    let (reply, kickoff) = harness.start_as_worker();

    assert_eq!(reply, Ok(()));
    assert!(matches!(
        harness.page.stage(),
        RoundStage::Starting {
            started_at_ms: Some(_),
            ..
        }
    ));
    assert_eq!(harness.page.round(), None);
    let kickoff = kickoff.expect("the kickoff, for the worker to send");
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(kickoff))));
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));

    let mut pane = Harness::start();
    pane.capture();
    assert!(
        matches!(pane.page.stage(), RoundStage::Starting { .. }),
        "a round started in the pane"
    );
}

#[test]
fn a_start_from_the_page_is_refused_while_a_round_runs_or_starts() {
    let mut harness = Harness::start();
    let (_, kickoff) = harness.start_as_worker();
    assert!(kickoff.is_some());

    let (second, none) = harness.start_as_worker();
    assert_eq!(second, Err(CommandRefusal::Stale));
    assert!(none.is_none());

    let mut running = Harness::start();
    running.ask_first_question();
    assert_eq!(running.start_on_page(false), Err(CommandRefusal::Stale));
    assert_eq!(running.agents.prompts().len(), 1, "no second kickoff");
}

#[test]
fn stopping_or_resetting_a_starting_round_shows_that_no_round_runs() {
    for command in [Command::Cancel, Command::Reset] {
        let mut harness = Harness::start();
        let (reply, _) = harness.start_as_worker();
        assert_eq!(reply, Ok(()));

        harness.session.handle(Input::Command(command));

        assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));
    }
}

#[test]
fn a_start_that_cannot_capture_the_change_shows_why_on_the_page() {
    let mut harness = Harness::start();
    std::fs::remove_dir_all(harness.files.root().join(".git")).unwrap();

    assert_eq!(
        harness.start_on_page(false),
        Ok(()),
        "the page loads the starting round at once"
    );

    assert!(harness.next::<ui_events::ExplorePageStart>().0.is_ok());
    assert!(
        harness.next::<ui_events::ExplorePageStart>().0.is_err(),
        "the pane hears that the start failed"
    );
    assert!(
        matches!(harness.page.stage(), RoundStage::StartFailed { .. }),
        "the page shows {:?}",
        harness.page.stage()
    );
    assert!(harness.agents.prompts().is_empty());
}

#[test]
fn the_first_pick_of_a_page_answer_is_saved_with_the_answer() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let input = AnswerInput {
        first_pick: Some("change".into()),
        ..keep("Keep it after all.")
    };

    assert_eq!(harness.answer_on_page("q1", 1, input), Ok(()));

    let posted = harness.next::<ui_events::ExplorePosted>();
    assert!(posted.result.is_ok(), "{:?}", posted.result);
    let saved = harness.saved();
    let [answer] = saved.exploration.answers.as_slice() else {
        panic!("saved answers: {:?}", saved.exploration.answers);
    };
    assert_eq!(
        (
            answer.option.as_ref().map(|o| o.id.as_str()),
            answer.first_pick.as_deref()
        ),
        (Some("keep"), Some("change"))
    );
    assert_eq!(posted.request.answer.as_ref(), Some(answer));
}

#[test]
fn a_page_answer_whose_first_pick_is_not_a_choice_is_refused() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let input = AnswerInput {
        first_pick: Some("elsewhere".into()),
        ..keep("Keep it.")
    };

    let reply = harness.answer_on_page("q1", 1, input);

    assert!(matches!(reply, Err(CommandRefusal::Failed(_))), "{reply:?}");
    assert!(harness.saved().exploration.answers.is_empty());
}

impl Harness {
    /// Send `text` from the Explore page as the list to implement for the conclusion of turn
    /// `conclusion`, in place of the request `replaces` the page showed, and return the
    /// session's reply.
    pub(super) fn implement_on_page(
        &mut self,
        conclusion: &str,
        replaces: Option<&str>,
        text: &str,
    ) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        let implement = PageImplement {
            conclusion: conclusion.into(),
            replaces: replaces.map(str::to_owned),
            text: text.into(),
        };
        self.session.handle(Input::Page {
            command: PageCommand::Implement(implement),
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }

    /// The request of the turn that posted the round's conclusion.
    pub(super) fn conclusion_request(&self) -> String {
        self.saved()
            .exploration
            .conclusion_request()
            .expect("a conclusion")
            .to_owned()
    }

    /// The implementation request the page shows for the conclusion.
    pub(super) fn shown_implementation(&self) -> Option<PageImplementation> {
        match self.page.stage() {
            RoundStage::Conclusion { implementation, .. } => implementation,
            stage => panic!("the page shows {stage:?}"),
        }
    }

    /// Wait until the implementation request is sent or fails, then pick up the outcome the
    /// prompt sender saved, as the storage watcher hands it to the session.
    pub(super) fn implementation_finished(&mut self) -> ui_events::ExploreImplementationFinished {
        let finished = self.next::<ui_events::ExploreImplementationFinished>();
        self.session.handle(Input::StorageChanged);
        finished
    }
}

const EDITED: &str = "Add a regression test.\nAnd document the policy.";

#[test]
fn an_implement_from_the_page_saves_and_prompts_the_request_the_pane_would() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    // What the pane would send for the same edited list, from its copy of the round.
    let pane = harness
        .saved()
        .exploration
        .implementation(EDITED.into())
        .unwrap();

    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));

    let finished = harness.implementation_finished();
    assert_eq!(finished.state, DispatchState::Delivered);
    let saved = harness.saved();
    let [delivery] = saved.implementations.values().collect::<Vec<_>>()[..] else {
        panic!("saved requests: {:?}", saved.implementations);
    };
    let expected = review_explore::ImplementationRequest {
        delivery: delivery.request.delivery.clone(),
        ..pane
    };
    assert_eq!(delivery.request, expected);
    assert_eq!(finished.request, expected);
    let prompts = harness.agents.prompts();
    assert_eq!(
        prompts.last().unwrap().text,
        review_explore_runner::implementation_prompt(&expected)
    );
}

#[test]
fn the_page_shows_its_implementation_request_as_sending_then_sent() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.delivery.close();

    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));

    let shown = harness.shown_implementation().expect("a request");
    assert_eq!(
        (shown.text.as_str(), &shown.state, shown.sent_at_ms),
        (EDITED, &ImplementationState::Sending, None)
    );
    harness.delivery.open();
    harness.implementation_finished();
    let shown = harness.shown_implementation().expect("a request");
    assert_eq!(shown.state, ImplementationState::Sent);
    // The page says when the agent received it.
    assert!(shown.sent_at_ms.is_some());
    harness.reopen();
    assert_eq!(
        harness.shown_implementation().map(|shown| shown.state),
        Some(ImplementationState::Sent)
    );
}

#[test]
fn a_request_saved_before_a_reopening_shows_as_paused_and_may_be_replaced_on_the_page() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.delivery.close();
    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));

    harness.reopen();

    harness.delivery.open();
    let shown = harness.shown_implementation().expect("a request");
    assert_eq!(shown.state, ImplementationState::Paused);
    assert_eq!(
        harness.implement_on_page(&conclusion, None, EDITED),
        Err(CommandRefusal::Stale),
        "the page did not show the paused request"
    );
    assert_eq!(
        harness.implement_on_page(&conclusion, Some(&shown.delivery), "Only this."),
        Ok(()),
        "a new request in place of the paused one, as the pane offers it"
    );
    let finished = (0..2)
        .map(|_| harness.implementation_finished())
        .find(|finished| finished.state == DispatchState::Delivered)
        .expect("the new request reaches the agent");
    assert_eq!(finished.request.text, "Only this.");
}

#[test]
fn a_repeated_implement_is_applied_once_and_a_stale_one_starts_no_second_implementation() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));
    harness.implementation_finished();
    let prompts = harness.agents.prompts().len();

    let repeated = harness.implement_on_page(&conclusion, None, EDITED);
    let other_conclusion = harness.implement_on_page("another-turn", None, EDITED);

    assert_eq!(repeated, Err(CommandRefusal::AlreadyApplied));
    assert_eq!(other_conclusion, Err(CommandRefusal::Stale));
    assert_eq!(harness.saved().implementations.len(), 1);
    assert_eq!(harness.agents.prompts().len(), prompts);

    let mut pane = Harness::start();
    pane.conclude();
    let conclusion = pane.conclusion_request();
    let request = pane
        .saved()
        .exploration
        .implementation("From the pane.".into())
        .unwrap();
    pane.session
        .handle(Input::Command(Command::Implement(request)));
    let after_the_pane = pane.implement_on_page(&conclusion, None, EDITED);
    assert_eq!(after_the_pane, Err(CommandRefusal::Stale));
    assert_eq!(pane.saved().implementations.len(), 1);
}

#[test]
fn an_empty_list_from_the_page_is_refused_as_in_the_pane() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    let pane = harness
        .saved()
        .exploration
        .implementation(" \n".into())
        .unwrap_err();

    let reply = harness.implement_on_page(&conclusion, None, " \n");

    assert_eq!(reply, Err(CommandRefusal::Failed(pane.to_string())));
    assert!(harness.saved().implementations.is_empty());
    assert_eq!(harness.shown_implementation(), None);
}

#[test]
fn after_a_request_that_was_not_sent_the_page_may_send_another() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    // The agent leaves after the request is saved, before it is sent.
    harness.delivery.close();
    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));
    harness.agents.remove_agent(&PaneId(PANE.into()));
    harness.delivery.open();
    let finished = harness.implementation_finished();
    assert!(matches!(finished.state, DispatchState::NotSent(_)));
    let shown = harness.shown_implementation().expect("a request");
    assert!(
        matches!(shown.state, ImplementationState::NotSent(_)),
        "{shown:?}"
    );
    harness.agents.upsert_agent(agent());

    assert_eq!(
        harness.implement_on_page(&conclusion, None, EDITED),
        Err(CommandRefusal::Stale),
        "the page did not show the request that was not sent"
    );
    assert_eq!(
        harness.implement_on_page(&conclusion, Some(&shown.delivery), "Only this."),
        Ok(())
    );
    let finished = harness.implementation_finished();
    assert_eq!(finished.state, DispatchState::Delivered);
    assert_eq!(finished.request.text, "Only this.");
}

#[test]
fn a_pane_implement_that_missed_the_pages_request_starts_no_second_implementation() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.delivery.close();
    assert_eq!(harness.implement_on_page(&conclusion, None, EDITED), Ok(()));
    // The pane's copy of the round, taken before the page's request.
    let pane = harness
        .saved()
        .exploration
        .implementation("From the pane.".into())
        .unwrap();

    harness
        .session
        .handle(Input::Command(Command::Implement(pane.clone())));

    harness.delivery.open();
    let finished = [
        harness.next::<ui_events::ExploreImplementationFinished>(),
        harness.implementation_finished(),
    ];
    let outcomes: Vec<_> = finished
        .iter()
        .map(|finished| {
            let sent = finished.state == DispatchState::Delivered;
            (finished.request.text.as_str(), sent)
        })
        .collect();
    assert_eq!(outcomes, [("From the pane.", false), (EDITED, true)]);
    let late = harness
        .saved()
        .exploration
        .implementation("From the pane, later.".into())
        .unwrap();
    harness
        .session
        .handle(Input::Command(Command::Implement(late)));
    assert!(matches!(
        harness
            .next::<ui_events::ExploreImplementationFinished>()
            .state,
        DispatchState::NotSent(_)
    ));
    assert_eq!(harness.saved().implementations.len(), 1);
    let implement_prompts = harness
        .agents
        .prompts()
        .iter()
        .filter(|prompt| prompt.text.contains("From the pane") || prompt.text.contains(EDITED))
        .count();
    assert_eq!(implement_prompts, 1);
}

/// The steps of the round rail the page shows, with their states, and the tab title.
fn shown_rail(feed: &review_explore_page::RoundFeed) -> Option<(Vec<(Step, StepState)>, TabTitle)> {
    let overview = feed.overview()?;
    let rail = overview
        .rail
        .iter()
        .map(|step| (step.step.clone(), step.state))
        .collect();
    Some((rail, overview.title))
}

#[test]
fn the_page_shows_where_the_round_stands_on_its_rail_and_in_the_tab_title() {
    use StepState::{Current, Done, Later};
    let quiz = || Step::Quiz {
        stage: QuizStage::Later,
    };
    let mut harness = Harness::start();
    assert_eq!(shown_rail(&harness.page), None);

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert_eq!(
        shown_rail(&harness.page),
        Some((
            vec![
                (Step::Design, Current { working: true }),
                (quiz(), Later),
                (Step::Conclusion, Later),
            ],
            TabTitle::AgentWorking
        ))
    );

    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        shown_rail(&harness.page),
        Some((
            vec![
                (Step::Design, Done),
                (Step::Question { number: 1 }, Current { working: false }),
                (quiz(), Later),
                (Step::Conclusion, Later),
            ],
            TabTitle::YourTurn { question: 1 }
        ))
    );

    let (answer, access) = harness.answer("Keep it.");
    assert_eq!(
        shown_rail(&harness.page).map(|(rail, title)| (rail[1].clone(), title)),
        Some((
            (Step::Question { number: 1 }, Current { working: true }),
            TabTitle::AgentWorking
        ))
    );

    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        shown_rail(&harness.page),
        Some((
            vec![
                (Step::Design, Done),
                (Step::Question { number: 1 }, Done),
                (Step::Question { number: 2 }, Current { working: false }),
                (quiz(), Later),
                (Step::Conclusion, Later),
            ],
            TabTitle::YourTurn { question: 2 }
        ))
    );

    harness.answer("Keep it too.");
    harness.session.handle(Input::Command(Command::Cancel));
    assert_eq!(
        shown_rail(&harness.page).map(|(rail, title)| (rail[2].clone(), title)),
        Some((
            (Step::Question { number: 2 }, Current { working: false }),
            TabTitle::RetryNeeded
        ))
    );

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(shown_rail(&harness.page), None);
}

#[test]
fn the_page_shows_the_answer_the_agents_turn_carries_and_when_the_turn_went_out() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    // The kickoff carries no answer.
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { answer: None, .. }
    ));
    assert!(applied(harness.submit(&access, question(&first, 1))));

    let (answer, _) = harness.answer("Keep it.");
    // The prompt went out on another thread, which saved when: the storage watcher follows.
    harness.session.handle(Input::StorageChanged);

    let RoundStage::AgentWorking {
        request,
        sent_at_ms,
        answer: Some(sent),
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(request, answer.request);
    assert!(sent_at_ms.is_some());
    assert_eq!(
        sent_at_ms,
        harness.saved().turns[&answer.request].started_at_ms
    );
    let answered = sent.question.as_ref().expect("the question it answers");
    assert_eq!(answered.question.id, "q1");
    assert!(!answered.picked_blind);
    assert_eq!(sent.kept.choice.as_deref(), Some("Keep it"));
    assert_eq!(sent.kept.comment, "Keep it.");

    // Stopped, the turn waits for Retry with the same answer.
    harness.session.handle(Input::Command(Command::Cancel));
    let RoundStage::Interrupted {
        answer: Some(stopped),
        ..
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(stopped, sent);
}
