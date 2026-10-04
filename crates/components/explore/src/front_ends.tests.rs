//! The pane while another front end, such as the Explore page, answers the
//! round it shows. The session sends the pane the same events as for its own
//! answers; these tests replay them.

use std::sync::Arc;

use component_core::ComponentEventBus;
use ratatui::layout::Rect;
use review_explore::{
    AnswerInput, Comparison, ConversationTurn, Exploration, ExploreRound, Question, TurnRequest,
};
use review_source::ReviewCheckpoint;
use ui_actions::Action;
use ui_events::{
    ExploreAnswerCancelled, ExploreCommitted, ExploreFinished, ExplorePageReset, ExplorePageStart,
    ExplorePageStopped, ExplorePosted, ExploreProgress, ExploreRestored,
};

use super::rows;
use crate::flow::ConversationLayout;
use crate::start::StartButton;
use crate::{Control, ExploreComponent, Progress};

fn question(id: &str) -> Question {
    serde_json::from_value(serde_json::json!({
        "id": id, "version": 1, "topic": "policy", "text": "Keep the policy?",
        "rationale": null, "visual": null,
        "alternatives": [
            {"id": "keep", "text": "Keep it", "outcome": "accepted"},
            {"id": "change", "text": "Change it", "outcome": "needs_follow_up"}
        ],
        "evidence": []
    }))
    .unwrap()
}

/// The agent's turn for `request`, after `answer`, asking `next`.
fn agent_turn(
    exploration: &Exploration,
    request: &str,
    answer: Option<&str>,
    next: &Question,
) -> ConversationTurn {
    ConversationTurn {
        answer: answer.map(str::to_owned),
        update: serde_json::from_value(serde_json::json!({
            "instance": exploration.instance, "request": request,
            "checkpoint": exploration.comparison.checkpoint,
            "interpretation": null,
            "topics": [{"id": "policy", "title": "Policy", "entries": [], "status": "open"}],
            "next": next, "conclusion": null, "limitations": [], "findings": []
        }))
        .unwrap(),
    }
}

/// A saved round whose first question waits for the reviewer.
fn asked_round() -> ExploreRound {
    let mut exploration = Exploration::new(Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: "/tmp".into(),
        files: vec![],
        diffs: vec![],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    }));
    let first = question("q1");
    let kickoff = agent_turn(&exploration, "kickoff", None, &first);
    exploration.topics = kickoff
        .update
        .topics
        .iter()
        .map(|topic| (topic.id.clone(), topic.clone()))
        .collect();
    exploration.conversation.push(kickoff);
    exploration.questions.push(first);
    let mut round = ExploreRound::new(exploration);
    round.revision = 1;
    round
}

/// What the session saves and announces when another front end answers the
/// first question with its first choice.
fn answered_elsewhere(round: &ExploreRound) -> (TurnRequest, ExploreRound) {
    let mut copy = round.exploration.clone();
    let request = copy
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                text: "Keep it, answered on the page.".into(),
                ..AnswerInput::default()
            }),
            round.exploration.questions.first(),
        )
        .unwrap();
    let mut saved = round.clone();
    assert!(saved.post(&request).unwrap());
    saved.revision += 1;
    (request, saved)
}

struct Pane {
    bus: ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
}

impl Pane {
    /// The pane, reopened on `round`.
    fn showing(round: &ExploreRound) -> Self {
        let mut bus = ComponentEventBus::<Action>::new();
        let target = bus.mount(|events| {
            ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default())
        });
        bus.publish(ExploreRestored {
            result: Ok(Some(Arc::new(round.clone()))),
            view: None,
            historical: false,
            storage_error: None,
            progress: ExploreProgress::Ready,
        })
        .unwrap();
        Self { bus, target }
    }

    fn component(&mut self) -> &mut ExploreComponent {
        self.bus.get_mut::<ExploreComponent>(self.target).unwrap()
    }

    /// The rows of the selected question's answers and status.
    fn answers(&mut self) -> Vec<String> {
        let component = self.component();
        let question = component.question().unwrap().clone();
        let mut layout = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        let palette = ui_theme::Theme::default().palette;
        component.answers(component.selected, &question, &mut layout, palette);
        rows(&layout)
    }
}

#[test]
fn an_answer_from_elsewhere_shows_in_the_pane_without_a_waiting_state() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = answered_elsewhere(&round);

    pane.bus
        .publish(ExplorePosted {
            request,
            result: Ok(Arc::new(saved)),
        })
        .unwrap();

    let rows = pane.answers();
    assert!(
        rows.iter()
            .any(|row| row.contains("Keep it, answered on the page.")),
        "{rows:?}"
    );
    let component = pane.component();
    assert!(component.progress == Progress::Ready);
    assert!(component.status.is_empty(), "{}", component.status);
}

#[test]
fn the_pane_can_still_send_an_answer_that_the_saved_round_refuses() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = answered_elsewhere(&round);
    pane.bus
        .publish(ExplorePosted {
            request,
            result: Ok(Arc::new(saved.clone())),
        })
        .unwrap();
    let component = pane.component();
    component.editor = comment_editor::CommentEditor::new("And add a test.", &component.keymap);

    // The question is answered, so no choice is selected: the pane sends free text.
    let actions = component.answer(Control::Send);

    let [Action::Explore(review_explore::Command::Turn(second))] = actions.as_slice() else {
        panic!("the pane sends a turn: {actions:?}");
    };
    assert!(second.answer.as_ref().unwrap().option.is_none());
    let refusal = saved.clone().post(second).unwrap_err().to_string();
    pane.bus
        .publish(ExplorePosted {
            request: (**second).clone(),
            result: Err(refusal),
        })
        .unwrap();
    let component = pane.component();
    assert!(component.progress == Progress::Ready);
    assert!(
        !component.status.is_empty(),
        "the pane says it was not posted"
    );
    assert_eq!(component.exploration.as_ref().unwrap().answers.len(), 1);
}

#[test]
fn the_pane_moves_to_the_agents_next_question() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = answered_elsewhere(&round);
    pane.bus
        .publish(ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(saved.clone())),
        })
        .unwrap();
    let mut next = saved;
    let answer = request.answer.as_ref().map(|answer| answer.id.as_str());
    let second = question("q2");
    let turn = agent_turn(&next.exploration, &request.request, answer, &second);
    next.exploration.conversation.push(turn);
    next.exploration.questions.push(second);
    next.exploration.pause_delivery();
    next.revision += 1;
    let (response, acknowledged) = std::sync::mpsc::channel();

    pane.bus
        .publish(ExploreCommitted {
            round: Arc::new(next),
            applied: true,
            response,
        })
        .unwrap();

    assert_eq!(acknowledged.recv().unwrap(), Ok(true));
    let component = pane.component();
    assert_eq!(component.selected, 1);
    assert!(component.progress == Progress::Ready);
}

#[test]
fn a_failed_delivery_of_an_answer_from_elsewhere_leaves_the_pane_unchanged() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = answered_elsewhere(&round);
    pane.bus
        .publish(ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(saved)),
        })
        .unwrap();

    pane.bus
        .publish(ExploreFinished {
            instance: request.instance,
            request: request.request,
            result: Err("The selected agent is no longer available".into()),
        })
        .unwrap();

    let component = pane.component();
    assert!(
        component.progress == Progress::Ready,
        "no Retry in the pane"
    );
    assert!(component.status.is_empty(), "{}", component.status);
}

/// What the session saves and announces when the Explore page starts a round: the kickoff,
/// and the round it opens.
fn started_elsewhere(challenger: bool) -> (TurnRequest, ExploreRound) {
    let mut exploration = Exploration::new(asked_round().exploration.comparison);
    exploration.challenger = challenger;
    let mut copy = exploration.clone();
    let kickoff = copy.request(None, None).unwrap();
    let mut round = ExploreRound::new(exploration);
    assert!(round.post(&kickoff).unwrap());
    round.revision = 1;
    (kickoff, round)
}

impl Pane {
    /// The pane on its start screen.
    fn starting() -> Self {
        let mut bus = ComponentEventBus::<Action>::new();
        let target = bus.mount(|events| {
            ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default())
        });
        bus.publish(ExploreRestored {
            result: Ok(None),
            view: None,
            historical: false,
            storage_error: None,
            progress: ExploreProgress::Ready,
        })
        .unwrap();
        Self { bus, target }
    }
}

#[test]
fn a_round_started_elsewhere_shows_in_the_pane_as_one_started_there() {
    let mut pane = Pane::starting();
    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();
    let (kickoff, round) = started_elsewhere(true);

    let actions = pane
        .bus
        .publish(ExplorePosted {
            request: kickoff.clone(),
            result: Ok(Arc::new(round.clone())),
        })
        .unwrap()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert!(
        !actions.contains(&Action::ExplorePage(ui_actions::ExplorePageAction::Open)),
        "the page that started the round is open already"
    );
    let component = pane.component();
    let shown = component
        .exploration
        .as_ref()
        .expect("the pane shows the round");
    assert_eq!(shown.instance, round.exploration.instance);
    assert!(shown.challenger);
    assert!(component.progress == Progress::Waiting);
    assert!(
        component.shows_page_round(),
        "a round started on the page is followed there"
    );

    let mut asked = round;
    let first = question("q1");
    let turn = agent_turn(&asked.exploration, &kickoff.request, None, &first);
    asked.exploration.conversation.push(turn);
    asked.exploration.questions.push(first);
    asked.exploration.pause_delivery();
    asked.revision += 1;
    let (response, acknowledged) = std::sync::mpsc::channel();
    pane.bus
        .publish(ExploreCommitted {
            round: Arc::new(asked),
            applied: true,
            response,
        })
        .unwrap();

    assert_eq!(acknowledged.recv().unwrap(), Ok(true));
    let component = pane.component();
    assert_eq!(component.question().map(|q| q.id.as_str()), Some("q1"));
    assert!(component.progress == Progress::Ready);
}

#[test]
fn a_failed_kickoff_of_a_round_started_elsewhere_offers_retry_in_the_pane() {
    let mut pane = Pane::starting();
    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();
    let (kickoff, round) = started_elsewhere(false);
    pane.bus
        .publish(ExplorePosted {
            request: kickoff.clone(),
            result: Ok(Arc::new(round)),
        })
        .unwrap();

    pane.bus
        .publish(ExploreFinished {
            instance: kickoff.instance,
            request: kickoff.request,
            result: Err("The selected agent is no longer available".into()),
        })
        .unwrap();

    assert!(pane.component().progress == Progress::Retryable);
}

#[test]
fn a_round_started_elsewhere_leaves_a_pane_that_is_starting_its_own() {
    let mut pane = Pane::starting();
    let actions = pane.component().start(StartButton::START_IN_PANE.start);
    assert!(!actions.is_empty(), "the pane asks the session to capture");
    let (kickoff, round) = started_elsewhere(false);

    pane.bus
        .publish(ExplorePosted {
            request: kickoff,
            result: Ok(Arc::new(round)),
        })
        .unwrap();

    let component = pane.component();
    assert!(component.exploration.is_none());
    assert!(component.progress == Progress::Capturing);
}

#[test]
fn a_round_starting_on_the_page_shows_in_the_pane_until_its_kickoff_is_saved() {
    let mut pane = Pane::starting();

    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();

    let component = pane.component();
    assert!(component.progress == Progress::Waiting);
    assert!(
        component.start(StartButton::START_IN_PANE.start).is_empty(),
        "no second start from the pane"
    );
    let (kickoff, round) = started_elsewhere(false);
    pane.bus
        .publish(ExplorePosted {
            request: kickoff,
            result: Ok(Arc::new(round)),
        })
        .unwrap();
    assert!(pane.component().exploration.is_some());
}

#[test]
fn the_pane_stops_a_round_starting_on_the_page() {
    let mut pane = Pane::starting();
    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();

    let actions = pane.component().cancel();

    assert!(
        matches!(
            actions.as_slice(),
            [Action::Explore(review_explore::Command::Cancel)]
        ),
        "{actions:?}"
    );
    let component = pane.component();
    assert!(component.progress == Progress::Ready);
    assert!(component.exploration.is_none());
}

#[test]
fn a_kickoff_saved_after_the_pane_stopped_the_page_start_leaves_the_start_screen() {
    let mut pane = Pane::starting();
    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();
    pane.component().cancel();
    let (kickoff, round) = started_elsewhere(false);

    pane.bus
        .publish(ExplorePosted {
            request: kickoff,
            result: Ok(Arc::new(round)),
        })
        .unwrap();

    let component = pane.component();
    assert!(component.exploration.is_none(), "the session cancels it");
    assert!(component.progress == Progress::Ready);
}

#[test]
fn a_round_that_could_not_start_on_the_page_leaves_the_pane_on_its_start_screen() {
    for failure in ["capture", "kickoff"] {
        let mut pane = Pane::starting();
        pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();

        if failure == "capture" {
            pane.bus
                .publish(ExplorePageStart(Err(
                    "Repository comparison is not ready".into()
                )))
                .unwrap();
        } else {
            let (kickoff, _) = started_elsewhere(false);
            pane.bus
                .publish(ExplorePosted {
                    request: kickoff,
                    result: Err("Explore storage is unavailable".into()),
                })
                .unwrap();
        }

        let component = pane.component();
        assert!(component.progress == Progress::Ready, "{failure}");
        assert!(component.exploration.is_none(), "{failure}");
        assert!(!component.status.is_empty(), "{failure}: the pane says why");
    }
}

impl Pane {
    /// The pane answers the first question itself, and the session saves the answer: the agent
    /// works on its turn. Returns the turn and the saved round.
    fn answer_first_question(&mut self, round: &ExploreRound) -> (TurnRequest, ExploreRound) {
        let component = self.component();
        component.turns[0].choice = 0;
        let actions = component.answer(Control::Send);
        let [Action::Explore(review_explore::Command::Turn(request))] = actions.as_slice() else {
            panic!("the pane sends a turn: {actions:?}");
        };
        let request = (**request).clone();
        let mut saved = round.clone();
        assert!(saved.post(&request).unwrap());
        saved.revision += 1;
        self.bus
            .publish(ExplorePosted {
                request: request.clone(),
                result: Ok(Arc::new(saved.clone())),
            })
            .unwrap();
        assert!(self.component().progress == Progress::Waiting);
        (request, saved)
    }
}

#[test]
fn the_pane_follows_stop_waiting_on_the_page() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    pane.answer_first_question(&round);

    pane.bus
        .publish(ExplorePageStopped {
            round: Some(round.exploration.instance.clone()),
        })
        .unwrap();

    let component = pane.component();
    assert!(
        component.progress == Progress::Retryable,
        "the pane offers Retry"
    );
    assert!(!component.status.is_empty(), "the pane says why");
    assert!(
        component
            .exploration
            .as_ref()
            .unwrap()
            .pending_request()
            .is_none()
    );
}

#[test]
fn the_pane_follows_a_stop_on_the_page_of_a_round_starting_there() {
    let mut pane = Pane::starting();
    pane.bus.publish(ExplorePageStart(Ok(()))).unwrap();

    pane.bus
        .publish(ExplorePageStopped { round: None })
        .unwrap();

    let component = pane.component();
    assert!(component.exploration.is_none());
    assert!(
        component.progress == Progress::Ready,
        "the pane offers Start"
    );
}

#[test]
fn the_pane_follows_retry_on_the_page_of_its_own_turn() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = pane.answer_first_question(&round);
    pane.bus
        .publish(ExploreFinished {
            instance: request.instance.clone(),
            request: request.request.clone(),
            result: Err("The selected agent is no longer available".into()),
        })
        .unwrap();
    assert!(pane.component().progress == Progress::Retryable);
    let mut retried = saved;
    retried.revision += 1;

    pane.bus
        .publish(ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(retried)),
        })
        .unwrap();

    let component = pane.component();
    assert!(
        component.progress == Progress::Waiting,
        "the pane waits for the agent again"
    );
    assert_eq!(
        component
            .exploration
            .as_ref()
            .unwrap()
            .pending_request()
            .map(|pending| &pending.request),
        Some(&request.request)
    );
}

#[test]
fn the_pane_follows_cancel_answer_on_the_page() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);
    let (request, saved) = answered_elsewhere(&round);
    pane.bus
        .publish(ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(saved.clone())),
        })
        .unwrap();
    let answer = request.answer.unwrap().id;
    let mut cancelled = saved;
    cancelled.cancel_answer(&answer).unwrap();
    cancelled.revision += 1;

    pane.bus
        .publish(ExploreAnswerCancelled {
            answer,
            result: Ok(Arc::new(cancelled)),
        })
        .unwrap();

    let component = pane.component();
    assert!(component.exploration.as_ref().unwrap().answers.is_empty());
    assert!(component.progress == Progress::Ready);
    assert!(!component.status.is_empty(), "the pane says what happened");
}

#[test]
fn the_pane_follows_reset_on_the_page() {
    let round = asked_round();
    let mut pane = Pane::showing(&round);

    pane.bus
        .publish(ExplorePageReset {
            round: round.exploration.instance.clone(),
        })
        .unwrap();

    let component = pane.component();
    assert!(
        component.exploration.is_none(),
        "the pane shows its start screen"
    );
    assert!(component.progress == Progress::Ready);
}
