//! The stage of the round the session publishes for the Explore page.

use review_explore::Command;
use review_explore_page::{CommandRefusal, CommandReply, PageAnswer, PageCommand, RoundStage};
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
    assert_eq!(harness.page.stage(), RoundStage::NoRound);

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);

    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );

    let (answer, access) = harness.answer("Keep it.");
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((2, "q2".into()))
    );

    let (answer, access) = harness.answer("Keep it too.");
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    let RoundStage::Conclusion(shown) = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(shown.summary, CONCLUSION);

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.stage(), RoundStage::NoRound);
}

#[test]
fn the_page_shows_a_turn_the_agent_no_longer_works_on_as_interrupted() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);

    harness.session.handle(Input::Command(Command::Cancel));

    assert_eq!(
        harness.page.stage(),
        RoundStage::Interrupted { failure: None }
    );
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
    assert_eq!(
        harness.page.stage(),
        RoundStage::Interrupted { failure: None },
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
    fn ask_first_question(&mut self) {
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
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
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

#[test]
fn a_page_answer_to_a_question_the_agent_moved_past_is_refused() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, access) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&access, question(&answer, 2))));

    assert_eq!(
        harness.answer_on_page("q1", 1, keep("Keep it.")),
        Err(CommandRefusal::Stale)
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
            RoundStage::Interrupted { failure: Some(_) }
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
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
    assert_eq!(harness.page.design().as_deref(), Some(&explained));
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    assert_eq!(harness.page.design().as_deref(), Some(&explained));

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.design(), None);
}

impl Harness {
    /// Start a round from the Explore page, and return the session's reply.
    fn start_on_page(&mut self, challenger: bool) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        self.session.handle(Input::Page {
            command: PageCommand::Start { challenger },
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }

    /// Start a round from the Explore page as the reviewer's worker does, which sends the
    /// kickoff itself; return the reply and the kickoff.
    fn start_as_worker(&mut self) -> (Result<(), CommandRefusal>, Option<TurnRequest>) {
        let (reply, replied) = CommandReply::channel();
        let kickoff = self.session.start_from_page(false, reply);
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
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
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
    assert_eq!(harness.page.stage(), RoundStage::Starting);
    assert_eq!(harness.page.round(), None);
    let kickoff = kickoff.expect("the kickoff, for the worker to send");
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(kickoff))));
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);

    let mut pane = Harness::start();
    pane.capture();
    assert_eq!(
        pane.page.stage(),
        RoundStage::Starting,
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

        assert_eq!(harness.page.stage(), RoundStage::NoRound);
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
