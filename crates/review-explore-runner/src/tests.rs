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

#[test]
fn kickoff_supplies_scope_and_identity_and_uses_the_mcp_schema() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let request = exploration.request(None, None).unwrap();
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(prompt.contains(&format!("Explore pass: {}", request.instance)));
    assert!(prompt.contains(&format!("Explore request: {}", request.request)));
    assert!(prompt.contains("Explore review access: fresh-access\n"));
    assert!(prompt.contains("Review unit: r\nCheckpoint: c\n"));
    assert!(prompt.contains(&format!(
        "Repository root: {}",
        comparison.repository_root.display()
    )));
    assert!(prompt.contains("Submit the first question directly"));
    assert!(prompt.contains("MCP tools' advertised input schemas"));
    for obsolete in [
        "Mailbox:",
        "request.json",
        "manifest.json",
        "response.json",
        "read_explore",
        "get_explore",
        "Turn input (JSON)",
        "{{",
        "\"instance\":",
        "schema (",
    ] {
        assert!(!prompt.contains(obsolete), "{obsolete}");
    }
}

#[test]
fn wakeup_omits_coverage_feedback() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    for label in [
        "Coverage inspection reminder",
        "Uncovered directory groups",
        "Uncovered files",
        "Unassigned:",
        "Awaiting answer:",
        "covered_percent_tenths",
        "coverage_after_answer",
    ] {
        assert!(!prompt.contains(label), "{label}");
    }
    assert!(prompt.contains("Answer ID: answer-id"));
}

#[test]
fn wakeup_delivers_full_selected_text_and_comment_with_plain_identity() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let original = request.clone();
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(prompt.contains("Answer ID: answer-id\nQuestion: earlier-question (version 7)\n"));
    assert!(prompt.contains("Selected option ID: keep\nSelected outcome: accepted\n"));
    assert!(prompt.contains("Selected option:\nKeep the full policy — including legacy callers\nWith \"bounded\" recovery\n"));
    assert!(prompt.ends_with("Comment:\nKeep it only after checking the unchanged caller.\nLiteral \"{{ROOT}}\" — {{TURN}}\n"));
    assert_eq!(
        request, original,
        "Preparing a prompt must not shrink reviewer history"
    );
    assert!(prompt.contains("submit_question"));
    assert!(!prompt.contains("get_explore"));
    assert!(!prompt.contains("Turn input (JSON)"));
    assert!(!prompt.contains("Repository root:"));
    assert!(!prompt.contains("This is recommended"));
    assert_eq!(prompt.matches(&request.instance).count(), 1);
    assert_eq!(prompt.matches(&request.request).count(), 1);
    assert!(prompt.contains("Review unit: r\nCheckpoint: c\n"));
    assert!(prompt.len() < 1_000);
}

#[test]
fn question_size_does_not_expand_the_wakeup() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let before = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
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
        PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt(),
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
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(prompt.contains("Answer ID: answer-id\nReply to conclusion: first-turn\n"));
    assert!(prompt.ends_with(&format!("Comment:\n{text}\n")));
    assert!(!prompt.contains("Question:"));
    assert!(!prompt.contains("Selected option"));
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
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(prompt.contains("Selected option ID: none-of-the-above\nSelected outcome: open\n"));
    assert!(prompt.ends_with("Selected option:\nNone of the above\n"));
    for absent in [
        "Comment:",
        "Explicitly deferred",
        "Corrects answer:",
        "Previous response error:",
    ] {
        assert!(!prompt.contains(absent));
    }
    let answer = request.answer.as_mut().unwrap();
    answer.option = None;
    answer.deferred = true;
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(prompt.contains("Explicitly deferred\n"));
    assert!(!prompt.contains("Selected option"));
    assert!(!prompt.contains("Comment:"));
}

#[test]
fn retries_and_corrections_only_add_the_relevant_details() {
    let comparison = comparison();
    let mut exploration = Exploration::new(Arc::new(comparison.clone()));
    let mut request = exploration.request(None, None).unwrap();
    request.answer = Some(answer(&request));
    let ordinary = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    request.answer.as_mut().unwrap().corrects = Some("earlier-answer".into());
    let corrected = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert_eq!(
        corrected.replace("Corrects answer: earlier-answer\n", ""),
        ordinary
    );
    request.response_error = Some("Fix the invalid line range\nKeep the literal {{ROOT}}".into());
    let prompt = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert_eq!(
        prompt.replace(
            "\nPrevious response error:\nFix the invalid line range\nKeep the literal {{ROOT}}\n",
            ""
        ),
        corrected
    );
    request.answer = None;
    let kickoff = PreparedTurn::prepare(&request, &comparison, "fresh-access").prompt();
    assert!(kickoff.contains(
        "Previous response error:\nFix the invalid line range\nKeep the literal {{ROOT}}\n"
    ));
    assert!(!kickoff.contains("Answer ID: answer-id"));
    assert!(!kickoff.contains("Corrects answer:"));
}
