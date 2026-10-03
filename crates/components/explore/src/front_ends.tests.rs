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
    ExploreCommitted, ExploreFinished, ExplorePosted, ExploreProgress, ExploreRestored,
};

use super::rows;
use crate::flow::ConversationLayout;
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
                in_reply_to: None,
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
