use super::*;
use review_explore::{Exploration, ReviewerAnswer};
use std::sync::Arc;

fn comparison() -> Comparison {
    Comparison {
        checkpoint: serde_json::from_value(serde_json::json!({"review_unit":"r","checkpoint":"c"}))
            .unwrap(),
        files: vec![],
        repository_root: std::env::temp_dir(),
        context: vec![],
        diffs: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    }
}

fn answer(request: &TurnRequest) -> ReviewerAnswer {
    let question: review_explore::Question = serde_json::from_value(serde_json::json!({
        "id":"earlier-question", "version":7, "topic":"policy", "text":"Keep the policy?",
        "alternatives":[{"id":"keep","text":"Keep the full policy — including legacy callers\nWith \"bounded\" recovery","outcome":"accepted","recommendation":"This is recommended because of existing compatibility"},
            {"id":"change","text":"Change it","outcome":"needs_follow_up"}],
        "evidence":[]
    }))
    .unwrap();
    ReviewerAnswer {
        id: "answer-id".into(),
        checkpoint: request.checkpoint.clone(),
        option: Some(question.alternatives[0].clone()),
        question: Some(question),
        in_reply_to: "first-turn".into(),
        text: "Keep it only after checking the unchanged caller.\nLiteral \"{{ROOT}}\" — {{TURN}}"
            .into(),
        deferred: false,
        corrects: None,
        author: "reviewer".into(),
    }
}

fn turn_input(prompt: &str) -> serde_json::Value {
    serde_json::from_str(
        prompt
            .lines()
            .find_map(|line| line.strip_prefix("Turn input (JSON): "))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn kickoff_supplies_scope_and_identity_for_a_direct_first_submission() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let request = exploration.request(None, None).unwrap();
    let prepared = PreparedTurn::prepare(&request, &comparison).unwrap();
    let prompt = prepared.prompt();
    assert!(prompt.contains(&format!("Explore review: {}", request.instance)));
    assert!(prompt.contains(&format!("Explore request: {}", request.request)));
    assert!(prompt.contains(&serde_json::to_string(&comparison.repository_root).unwrap()));
    assert!(prompt.contains("submit the first question directly"));
    assert_eq!(
        turn_input(&prompt),
        serde_json::json!({
            "instance": request.instance, "request": request.request,
            "checkpoint": request.checkpoint, "answer": null
        })
    );
    for obsolete in [
        "Mailbox:",
        "request.json",
        "manifest.json",
        "response.json",
        "read_explore",
        "get_explore",
    ] {
        assert!(!prompt.contains(obsolete), "{obsolete}");
    }
}

#[test]
fn wakeup_delivers_full_selected_text_and_comment_with_only_the_required_identity() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let original = request.clone();
    let prompt = PreparedTurn::prepare(&request, &comparison)
        .unwrap()
        .prompt();
    let result = turn_input(&prompt);
    assert_eq!(
        result,
        serde_json::json!({
            "instance": request.instance, "request": request.request, "checkpoint": request.checkpoint,
            "answer": {
                "id": "answer-id", "question": {"id":"earlier-question", "version":7},
                "option": {"id":"keep", "text":"Keep the full policy — including legacy callers\nWith \"bounded\" recovery", "outcome":"accepted"},
                "text": "Keep it only after checking the unchanged caller.\nLiteral \"{{ROOT}}\" — {{TURN}}"
            }
        })
    );
    assert_eq!(
        request, original,
        "Preparing a prompt must not shrink reviewer history"
    );
    assert!(prompt.len() - result.to_string().len() < 350);
    assert!(prompt.contains("submit_question"));
    assert!(!prompt.contains("get_explore"));
    assert_eq!(prompt.matches(&request.instance).count(), 1);
    assert_eq!(prompt.matches(&request.request).count(), 1);
}

#[test]
fn question_size_does_not_expand_the_wakeup() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let before = PreparedTurn::prepare(&request, &comparison)
        .unwrap()
        .prompt();
    let answer = request.answer.as_mut().unwrap();
    let mut question = serde_json::to_value(answer.question.as_ref().unwrap()).unwrap();
    let detailed = "Supporting investigation, unrelated to transmitting the answer. ".repeat(2_000);
    question["text"] = detailed.clone().into();
    question["rationale"] = detailed.clone().into();
    question["visual"] = detailed.clone().into();
    question["alternatives"][1]["text"] = detailed.clone().into();
    let evidence = serde_json::json!([{
        "path":"src/policy.rs", "side":"new", "lines":{"first_line":1,"last_line":2},
        "relationship":detailed, "decision_relevance":detailed
    }]);
    question["evidence"] = evidence.clone();
    question["supporting"] = evidence.clone();
    let consequence = serde_json::json!({"summary":detailed,"details":detailed,"evidence":evidence,"unknowns":[detailed]});
    question["assessments"] = serde_json::json!({"door":"unknown","reversibility":consequence,"blast_radius":consequence});
    answer.question = Some(serde_json::from_value(question).unwrap());
    answer.option.as_mut().unwrap().recommendation = Some(detailed);
    assert_eq!(
        PreparedTurn::prepare(&request, &comparison)
            .unwrap()
            .prompt(),
        before
    );
    assert!(serde_json::to_string(&request).unwrap().len() > 1_000_000);
}

#[test]
fn questionless_replies_keep_the_closing_turn_identity() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    let mut answer = answer(&request);
    answer.question = None;
    answer.option = None;
    let text = answer.text.clone();
    request.answer = Some(answer);
    let prompt = PreparedTurn::prepare(&request, &comparison)
        .unwrap()
        .prompt();
    assert_eq!(
        turn_input(&prompt)["answer"],
        serde_json::json!({
            "id":"answer-id", "question":null, "in_reply_to":"first-turn", "text":text
        })
    );
}

#[test]
fn absent_comments_and_choices_do_not_imply_deferral_and_none_keeps_its_full_label() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    let mut answer = answer(&request);
    answer.option = answer.question.as_ref().unwrap().choices().last().cloned();
    answer.text.clear();
    request.answer = Some(answer);
    let input = turn_input(
        &PreparedTurn::prepare(&request, &comparison)
            .unwrap()
            .prompt(),
    );
    assert_eq!(
        input["answer"]["option"],
        serde_json::json!({
            "id":"none-of-the-above", "text":"None of the above", "outcome":"open"
        })
    );
    for absent in ["text", "deferred", "corrects"] {
        assert!(input["answer"].get(absent).is_none());
    }
    let answer = request.answer.as_mut().unwrap();
    answer.option = None;
    answer.deferred = true;
    let input = turn_input(
        &PreparedTurn::prepare(&request, &comparison)
            .unwrap()
            .prompt(),
    );
    assert_eq!(input["answer"]["deferred"], true);
    assert!(input["answer"].get("option").is_none());
}

#[test]
fn retries_and_corrections_only_add_the_relevant_details() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let ordinary = turn_input(
        &PreparedTurn::prepare(&request, &comparison)
            .unwrap()
            .prompt(),
    );
    request.answer.as_mut().unwrap().corrects = Some("earlier-answer".into());
    let corrected = turn_input(
        &PreparedTurn::prepare(&request, &comparison)
            .unwrap()
            .prompt(),
    );
    let mut expected = ordinary;
    expected["answer"]["corrects"] = "earlier-answer".into();
    assert_eq!(corrected, expected);
    request.response_error = Some("Fix the invalid line range".into());
    let prompt = PreparedTurn::prepare(&request, &comparison)
        .unwrap()
        .prompt();
    expected["response_error"] = "Fix the invalid line range".into();
    assert_eq!(turn_input(&prompt), expected);
    request.answer = None;
    let kickoff = PreparedTurn::prepare(&request, &comparison)
        .unwrap()
        .prompt();
    expected["answer"] = serde_json::Value::Null;
    assert_eq!(turn_input(&kickoff), expected);
}
