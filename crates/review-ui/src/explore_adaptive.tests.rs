use super::*;
use review_explore::{AgendaAction, AgendaChange, Assessments, Consequence, Door, Reply};

fn publish(fixture: &mut ExploreUi, response: InterviewUpdate) {
    let actions = fixture.app.publish(ExploreFinished {
        instance: response.instance.clone(),
        request: response.request.clone(),
        result: Ok(response),
    });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::SetReviewed { .. } | Action::Thread(_)))
    );
}

#[test]
fn conversation_redirects_agenda_and_opens_new_evidence_in_the_native_viewer() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 100,
    });
    let mut response = fixture.response(&request, 1);
    response.topics.push(Topic {
        id: "migration".into(),
        title: "Durable migration".into(),
        prompt: "How will durable rows migrate?".into(),
        prerequisites: vec!["policy".into()],
        ..Topic::default()
    });
    publish(&mut fixture, response);
    fixture.app.update(UserInput::Paste(
        "These files are disposable cache. The authoritative data is elsewhere.".into(),
    ));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.files.write(
        "target/origin.rs",
        b"pub fn rebuild() { shared_origin(); }\n",
    );
    let mut response = fixture.response(&request, 2);
    response.interpretation = None;
    let evidence = EvidenceRef {
        location: review_explore::CodeLocation {
            path: review_repository::repository::RepoPath::from_bytes(b"target/origin.rs"),
            side: review_explore::SourceSide::New,
            lines: Some(GuideLineRange {
                first_line: 1,
                last_line: 1,
            }),
        },
        relationship: "Rebuild uses a shared origin".into(),
        decision_relevance: "This policy determines whether the proposed recovery is sufficient."
            .into(),
    };
    response.reply = Some(Reply {text:"Rebuild reads authoritative rows. Migration is unnecessary; simultaneous rebuilds still matter.".into(),evidence:vec![evidence.clone()]});
    response.agenda.push(AgendaChange {
        topic: "migration".into(),
        action: AgendaAction::Retire,
        reason: "Confirmed disposable cache from rebuild path".into(),
        answer: request.answer.as_ref().map(|answer| answer.id.clone()),
        evidence: vec![evidence.clone()],
        replacement: None,
        decision: None,
    });
    response.topics.push(Topic {
        id: "availability".into(),
        title: "Rebuild availability".into(),
        prompt: "Can the origin absorb all rebuilds?".into(),
        ..Topic::default()
    });
    let next = response.next.as_mut().unwrap();
    next.id = "rebuild".into();
    next.topic = "availability".into();
    next.evidence = vec![evidence.clone()];
    next.text = "How should concurrent rebuilds be bounded?".into();
    next.assessments = Some(cache_assessment(evidence));
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(text.contains("Door: Two-way"), "{text}");
    assert!(text.contains("Blast radius: All instances share origin load"));
    assert!(text.contains("pub fn rebuild()"));
    assert!(!text.contains("Question 1:"));
    assert!(text.contains("Agent: Rebuild reads authoritative rows"));
    assert!(!text.contains("Recorded domain context"));
    fixture.app.update(UserInput::Key(Key::Char('m')));
    let text = fixture.text();
    assert!(text.contains("Retired: Durable migration"));
    assert!(text.contains("Outstanding: Rebuild availability"));
    assert!(text.contains("How will durable rows migrate?"));
    assert!(text.contains("Depends on: Resolution"));
    assert!(text.contains("Files still requires human inspection"));
    let buffer = fixture.buffer();
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.fg == ratatui::style::Color::Yellow && cell.symbol() == "╭")
    );
    fixture.app.update(UserInput::Paste(
        "What happens if every instance rebuilds at once?".into(),
    ));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request.answer.unwrap().question.as_ref().unwrap().topic,
        "availability"
    );
}

#[test]
fn a_reply_to_an_earlier_question_keeps_its_identity_and_the_other_draft() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("First context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture
        .app
        .update(UserInput::Paste("Unposted current draft".into()));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    fixture
        .app
        .update(UserInput::Paste("Why did you ask that?".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request
            .answer
            .as_ref()
            .unwrap()
            .question
            .as_ref()
            .unwrap()
            .version,
        1
    );
    assert_eq!(
        request.answer.as_ref().unwrap().text,
        "Why did you ask that?"
    );
    let mut response = fixture.response(&request, 3);
    response.next = None;
    response.conclusion = Some(conclusion(
        "Current exploration paused; human Files inspection remains required.",
    ));
    response.interpretation = None;
    response.reply = Some(Reply {
        text: "The first question concerns compatibility.".into(),
        evidence: vec![],
    });
    publish(&mut fixture, response);
    fixture.click("[Previous]");
    fixture.app.update(UserInput::Key(Key::Enter));
    assert!(fixture.text().contains("Unposted current draft"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request.answer.unwrap().question.as_ref().unwrap().version,
        2
    );
}

#[test]
fn conclusion_opens_its_page_and_preserves_the_unposted_question_draft() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture
        .app
        .update(UserInput::Paste("Investigate consumers".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.app.update(UserInput::Paste(
        "Still typing this independent thought".into(),
    ));
    let mut response = fixture.response(&request, 2);
    response.interpretation = None;
    response.next = None;
    response.conclusion = Some(conclusion(
        "Consumer deployment is unknown. Human Files inspection remains required.",
    ));
    response.topics[0].prompt = "Check external readers".into();
    publish(&mut fixture, response);
    assert!(fixture.text().contains("Conclusion"));
    fixture.click("[Previous]");
    assert!(
        fixture
            .text()
            .contains("Still typing this independent thought")
    );
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request.answer.unwrap().text,
        "Still typing this independent thought"
    );
}

fn cache_assessment(evidence: EvidenceRef) -> Assessments {
    Assessments {
        door: Door::TwoWay,
        reversibility: Consequence {
            summary: "Rebuild disposable files".into(),
            details: "Only while the authoritative origin stays available".into(),
            evidence: vec![evidence.clone()],
            unknowns: vec![],
        },
        blast_radius: Consequence {
            summary: "All instances share origin load".into(),
            details: "No global limiter is shown".into(),
            evidence: vec![evidence],
            unknowns: vec!["Deployment size and origin capacity".into()],
        },
    }
}

#[test]
fn an_initial_stopping_point_exposes_reply_uncertainty_and_remaining_inspection() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    response.next = None;
    response.reply = Some(Reply {
        text: "No source-verifiable policy choice is ready.".into(),
        evidence: vec![],
    });
    response.conclusion = Some(conclusion(
        "Deployment context is missing; further human file inspection is required.",
    ));
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(!text.contains("No source-verifiable policy choice is ready."));
    assert!(text.contains("Deployment context is missing"));
    assert!(!text.contains("Question 1:"));
    fixture.click("[Opening]");
    assert!(
        fixture
            .text()
            .contains("No source-verifiable policy choice is ready.")
    );
}

#[test]
fn initial_conclusion_accepts_context_and_keeps_a_new_unscoped_draft_when_a_question_arrives() {
    let (mut fixture, request) = ExploreUi::new();
    let initial_request = request.request.clone();
    let mut response = fixture.response(&request, 1);
    response.next = None;
    response.conclusion = Some(conclusion(
        "Deployment context missing; human Files inspection remains required",
    ));
    publish(&mut fixture, response);
    fixture.click("[Reply]");
    fixture
        .app
        .update(UserInput::Paste("These files are disposable cache".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert!(request.answer.as_ref().unwrap().question.is_none());
    assert_eq!(
        request.answer.as_ref().unwrap().in_reply_to,
        initial_request
    );
    fixture
        .app
        .update(UserInput::Paste("Unposted context for the opening".into()));
    let mut response = fixture.response(&request, 1);
    response.interpretation = None;
    publish(&mut fixture, response);
    assert!(fixture.text().contains("Question 1/1"));
    assert!(!fixture.text().contains("Unposted context for the opening"));
    fixture.click("[Conclusion]");
    assert!(fixture.text().contains("Unposted context for the opening"));
    assert_eq!(fixture.text().matches("Your answer").count(), 1);
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert!(request.answer.as_ref().unwrap().question.is_none());
    assert_eq!(
        request.answer.unwrap().text,
        "Unposted context for the opening"
    );
}

#[test]
fn resuming_or_revisiting_a_correction_preserves_its_original_answer_link() {
    for revisit in [false, true] {
        let (mut fixture, request) = ExploreUi::new();
        fixture.respond(&request, 1);
        fixture.app.update(UserInput::Paste("First context".into()));
        let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        let original = request.answer.as_ref().unwrap().id.clone();
        fixture.respond(&request, 2);
        fixture.app.update(UserInput::Key(Key::Char('[')));
        fixture.app.update(UserInput::Key(Key::Char('x')));
        fixture.app.update(UserInput::Paste(
            "Correction: only with bounded rebuilds".into(),
        ));
        fixture.app.update(UserInput::Key(Key::Tab));
        if revisit {
            fixture.app.update(UserInput::Key(Key::Char(']')));
            fixture.click("[Reply]");
            fixture.app.update(UserInput::Paste("Other draft".into()));
            fixture.app.update(UserInput::Key(Key::Tab));
            fixture.app.update(UserInput::Key(Key::Char('[')));
        }
        fixture.app.update(UserInput::Key(Key::Enter));
        let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        assert_eq!(request.answer.as_ref().unwrap().corrects, Some(original));
        assert_eq!(
            request.answer.unwrap().text,
            "Correction: only with bounded rebuilds"
        );
    }
}

#[test]
fn automatic_advancement_keeps_the_direct_answer_visible_before_the_next_question() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 30,
    });
    fixture.respond(&request, 1);
    fixture
        .app
        .update(UserInput::Paste("Why does this caller rebuild?".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let mut response = fixture.response(&request, 2);
    response.interpretation = None;
    response.reply = Some(Reply {
        text: "The decoder rejects the previous format and the caller then rebuilds.".into(),
        evidence: vec![],
    });
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(text.contains("Agent: The decoder rejects"), "{text}");
    assert!(text.contains("Question 2: keep resolved?"));
    assert!(
        text.find("Agent: The decoder rejects").unwrap()
            < text.find("Question 2: keep resolved?").unwrap(),
        "{text}"
    );
}

fn question_after_conclusion_context() -> ExploreUi {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 120,
    });
    let mut response = fixture.response(&request, 1);
    response.next = None;
    response.conclusion = Some(conclusion(
        "Deployment context missing; human Files inspection remains required",
    ));
    publish(&mut fixture, response);
    fixture.click("[Reply]");
    fixture
        .app
        .update(UserInput::Paste("Opening context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let mut response = fixture.response(&request, 1);
    response.interpretation = None;
    publish(&mut fixture, response);
    fixture
}

#[test]
fn correcting_a_question_from_the_conclusion_composer_keeps_both_contexts_separate() {
    let mut fixture = question_after_conclusion_context();
    fixture
        .app
        .update(UserInput::Paste("Question context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let original = request.answer.as_ref().unwrap().clone();
    let mut response = fixture.response(&request, 2);
    response.next = None;
    response.conclusion = Some(conclusion("Further human file inspection remains required"));
    response.interpretation = None;
    publish(&mut fixture, response);
    fixture.click("[Reply]");
    fixture
        .app
        .update(UserInput::Paste("Unposted opening context".into()));
    fixture.click("[Previous]");
    fixture.click("[More]");
    fixture.click("[Correct]");
    fixture
        .app
        .update(UserInput::Paste("Question correction".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let correction = request.answer.unwrap();
    assert_eq!(correction.question, original.question);
    assert_eq!(correction.corrects, Some(original.id));
    assert_eq!(correction.text, "Question correction");
    fixture.click("[Conclusion]");
    fixture.click("[Reply]");
    assert!(fixture.text().contains("Unposted opening context"));
}

#[test]
fn conclusion_history_restores_its_draft_after_visiting_question_evidence() {
    let mut fixture = question_after_conclusion_context();
    fixture.click("[Conclusion]");
    fixture.click("[Reply]");
    fixture.app.update(UserInput::Paste("Opening draft".into()));
    fixture.click("[Next]");
    fixture.click("pub fn policy()");
    assert_eq!(fixture.app.focus, ui_events::ReviewPane::Detail);
    fixture.click("[Conclusion]");
    fixture.app.update(UserInput::Key(Key::Tab));
    assert_eq!(fixture.app.focus, ui_events::ReviewPane::Navigation);
    fixture.app.update(UserInput::Key(Key::Char('!')));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let answer = request.answer.unwrap();
    assert!(answer.question.is_none());
    assert_eq!(answer.text, "Opening draft!");
}
