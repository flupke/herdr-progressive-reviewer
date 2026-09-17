use review_explore::*;
use review_guide::{GuideLineRange, ReviewCheckpoint};
use review_repository::repository::ChangedFile;
use std::sync::Arc;

fn exploration() -> Exploration {
    let path = ChangedFile::modified("policy.rs").review_path().clone();
    Exploration::new(Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        files: vec![ChangedFile::modified("policy.rs")],
        repository_root: std::env::temp_dir(),
        context: vec![],
        diffs: vec![],
        manifest: vec![ManifestEntry {
            file: 0,
            hunk: None,
        }],
        base: None,
        sources: vec![Source {
            display_path: path.display(),
            path,
            side: SourceSide::Old,
            content: Some(b"first\nsecond\nthird\n".to_vec()),
            limitation: None,
            base: None,
        }],
    }))
}

fn question(version: u32) -> Question {
    Question {
        id: "q".into(),
        version,
        topic: "policy".into(),
        text: "Keep resolved conversations resolved?".into(),
        rationale: None,
        visual: None,
        supporting: vec![],
        assessments: None,
        alternatives: vec![
            Alternative {
                id: "keep".into(),
                text: "Keep resolved".into(),
                outcome: TopicStatus::Accepted,
                recommendation: None,
            },
            Alternative {
                id: "change".into(),
                text: "Reopen".into(),
                outcome: TopicStatus::NeedsFollowUp,
                recommendation: None,
            },
        ],
        evidence: vec![EvidenceRef {
            location: review_explore::CodeLocation {
                path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
                side: review_explore::SourceSide::Old,
                lines: Some(GuideLineRange {
                    first_line: 1,
                    last_line: 2,
                }),
            },
            relationship: "Policy".into(),
            decision_relevance:
                "This policy determines whether the proposed recovery is sufficient.".into(),
        }],
    }
}

fn update(request: &TurnRequest, next: Option<Question>) -> InterviewUpdate {
    InterviewUpdate {
        reply: Some(review_explore::Reply {
            text: "The source supports this context.".into(),
            evidence: vec![],
        }),
        agenda: vec![],
        instance: request.instance.clone(),
        request: request.request.clone(),
        checkpoint: request.checkpoint.clone(),
        interpretation: None,
        topics: vec![],
        conclusion: next
            .is_none()
            .then(|| conclusion("Further human file inspection remains required")),
        next,
        limitations: vec![],
        findings: vec![],
    }
}

fn started() -> Exploration {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, Some(question(1)));
    response.topics.push(Topic {
        prompt: String::new(),
        prerequisites: vec![],
        rank: 0,
        id: "policy".into(),
        title: "Resolution".into(),
        entries: vec![review_explore::CodeLocation {
            path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
            side: review_explore::SourceSide::Old,
            lines: None,
        }],
        status: TopicStatus::Open,
    });
    assert!(exploration.apply(response).unwrap());
    exploration
}

#[test]
fn settling_and_reopening_need_separate_attributed_human_answers() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap().id.clone();
    let mut response = update(&request, Some(self::question(2)));
    response.interpretation = Some(Interpretation {
        answer: answer.clone(),
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved".into(),
        follow_ups: vec![],
    });
    let error = exploration.apply(response.clone()).unwrap_err().to_string();
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Open);
    assert!(exploration.failed(&request.request, &error));
    let retry = exploration.retry().unwrap();
    assert_eq!(retry.response_error.as_deref(), Some(error.as_str()));
    assert_eq!(retry.answer, request.answer);
    response.request = retry.request;
    response.next = None;
    response.conclusion = Some(conclusion("Continue inspecting Files"));
    exploration.apply(response).unwrap();
    let correction = exploration
        .request(
            Some(AnswerInput {
                text: "I need to reconsider".into(),
                corrects: Some(answer),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut response = update(&correction, Some(self::question(2)));
    response.interpretation = Some(Interpretation {
        answer: correction.answer.unwrap().id,
        status: TopicStatus::Open,
        recap: "Recorded correction; clarify policy".into(),
        follow_ups: vec![],
    });
    exploration.apply(response).unwrap();
    assert_eq!(exploration.answers.len(), 2);
}

#[test]
fn context_clarification_conditional_decision_and_correction_append_exact_answers() {
    let mut exploration = started();
    let original = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "  Compliance archives these.\nKeep the audit trail.  ".into(),
                ..AnswerInput::default()
            }),
            Some(&original),
        )
        .unwrap();
    assert_eq!(
        exploration.answers[0].text,
        "  Compliance archives these.\nKeep the audit trail.  "
    );
    assert!(
        exploration
            .request(Some(AnswerInput::default()), Some(&original))
            .is_err()
    );
    let mut response = update(&request, Some(question(2)));
    response.interpretation = Some(Interpretation {
        answer: request.answer.unwrap().id,
        status: TopicStatus::Open,
        recap: "Recorded context: immutable archives".into(),
        follow_ups: vec![],
    });
    response.topics.push(Topic {
        prompt: String::new(),
        prerequisites: vec![],
        rank: 0,
        id: "retention".into(),
        title: "Audit retention".into(),
        entries: vec![],
        status: TopicStatus::Open,
    });
    assert!(exploration.apply(response).unwrap());
    assert_eq!(exploration.questions[0], original);
    assert!(exploration.topics.contains_key("retention"));

    let next = exploration.questions[1].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                text: "Only if we add a regression test.".into(),
                ..AnswerInput::default()
            }),
            Some(&next),
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap();
    assert_eq!(answer.option.as_ref().unwrap().id, "keep");
    assert_eq!(answer.option.as_ref().unwrap().text, "Keep resolved");
    let answer_id = answer.id.clone();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: answer_id.clone(),
        status: TopicStatus::NeedsFollowUp,
        recap: "Recorded: keep resolved; add a regression test — follow-up.".into(),
        follow_ups: vec!["Add regression test".into()],
    });
    exploration.apply(response).unwrap();
    assert_eq!(
        exploration.topics["policy"].status,
        TopicStatus::NeedsFollowUp
    );
    let correction = exploration
        .request(
            Some(AnswerInput {
                corrects: Some(answer_id),
                text: "The test must cover delayed replies too.".into(),
                ..AnswerInput::default()
            }),
            Some(&next),
        )
        .unwrap();
    assert_eq!(exploration.answers.len(), 3);
    assert!(correction.answer.unwrap().corrects.is_some());
    assert_eq!(
        exploration.answers[1].text,
        "Only if we add a regression test."
    );
}

#[test]
fn cancellation_retry_and_duplicate_results_cannot_rewrite_a_decision() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let first = exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    exploration.cancel();
    assert!(!exploration.apply(update(&first, None)).unwrap());
    let retry = exploration.retry().unwrap();
    assert_ne!(retry.request, first.request);
    assert_eq!(retry.answer, first.answer);
    assert_eq!(exploration.answers.len(), 1);
    let mut response = update(&retry, None);
    response.interpretation = Some(Interpretation {
        answer: retry.answer.unwrap().id,
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved.".into(),
        follow_ups: vec![],
    });
    assert!(exploration.apply(response.clone()).unwrap());
    assert!(!exploration.apply(response).unwrap());
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Accepted);
}

#[test]
fn unsafe_evidence_and_wrong_comparisons_preserve_last_question() {
    let mut exploration = started();
    let request = exploration.request(None, None).unwrap();
    let original = exploration.questions.clone();
    for source in ["../../etc/passwd", "/etc/passwd", "missing"] {
        let mut next = question(2);
        next.evidence[0].location.path =
            review_repository::repository::RepoPath::from_bytes(source.as_bytes());
        assert!(exploration.apply(update(&request, Some(next))).is_err());
    }
    for (first_line, last_line) in [(0, 1), (2, 1), (1, 500)] {
        let mut next = question(2);
        next.evidence[0].location.lines = Some(GuideLineRange {
            first_line,
            last_line,
        });
        assert!(exploration.apply(update(&request, Some(next))).is_err());
    }
    let mut response = update(&request, Some(question(2)));
    response.checkpoint.checkpoint = "other".into();
    assert!(exploration.apply(response).is_err());
    assert_eq!(exploration.questions, original);
    exploration.cancel();
    assert!(exploration.request(None, None).is_ok());
}

#[test]
fn an_agent_cannot_invent_acceptance_rewrite_questions_or_bind_an_old_answer() {
    let mut exploration = started();
    let request = exploration.request(None, None).unwrap();
    assert!(
        exploration
            .apply(update(&request, Some(question(1))))
            .is_err()
    );
    let mut response = update(&request, Some(question(2)));
    response.topics.push(Topic {
        prompt: String::new(),
        prerequisites: vec![],
        rank: 0,
        id: "invented".into(),
        title: "Invented agreement".into(),
        entries: vec![],
        status: TopicStatus::Accepted,
    });
    assert!(exploration.apply(response).is_err());
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: "invented".into(),
        status: TopicStatus::Accepted,
        recap: "Accepted".into(),
        follow_ups: vec![],
    });
    assert!(exploration.apply(response).is_err());
    assert!(exploration.answers.is_empty());
}

#[path = "interview/adaptive.rs"]
mod adaptive;

#[test]
fn none_of_the_above_keeps_the_question_open_and_the_original_wording_intact() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let none = question.choices().last().unwrap().clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some(none.id.clone()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap();
    assert_eq!(answer.option, Some(none));
    assert_eq!(answer.option.as_ref().unwrap().outcome, TopicStatus::Open);
    assert_eq!(answer.question.as_ref(), Some(&question));
    assert!(answer.text.is_empty());
    let mut response = update(&request, Some(self::question(2)));
    response.interpretation = Some(Interpretation {
        answer: answer.id.clone(),
        status: TopicStatus::Accepted,
        recap: "Invented acceptance".into(),
        follow_ups: vec![],
    });
    assert!(exploration.apply(response.clone()).is_err());
    response.interpretation = None;
    assert!(exploration.apply(response).unwrap());
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Open);
    assert_eq!(exploration.questions[0], question);
}

#[test]
fn agent_choices_cannot_duplicate_the_builtin_none_choice() {
    let mut exploration = started();
    let request = exploration.request(None, None).unwrap();
    for duplicate_id in [false, true] {
        let mut question = question(2);
        if duplicate_id {
            question.alternatives[0].id = "none-of-the-above".into();
        } else {
            question.alternatives[0].text = " none of the above ".into();
        }
        assert!(exploration.apply(update(&request, Some(question))).is_err());
        assert_eq!(exploration.questions.len(), 1);
    }
}

fn conclusion(summary: &str) -> review_explore::Conclusion {
    review_explore::Conclusion {
        summary: summary.into(),
        ..Default::default()
    }
}

#[test]
fn dedicated_conclusion_preserves_the_final_choice_and_rejects_a_changed_retry() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some("change".into()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    let payload = serde_json::json!({
        "review": request.instance, "request": request.request, "checkpoint": request.checkpoint,
        "interpretation": null,
        "summary": "Change the resolution policy; human Files inspection remains required.",
        "to_be_implemented": "Reopen resolved threads when new comments arrive.",
        "future_work": "Review notification volume later."
    });
    let mut submitted: ConclusionSubmission = serde_json::from_value(payload.clone()).unwrap();
    assert!(exploration.submit(submitted.clone().into_update()).is_err());
    submitted.interpretation = Some(Interpretation {
        answer: request.answer.as_ref().unwrap().id.clone(),
        status: TopicStatus::NeedsFollowUp,
        recap: "Recorded: reopen on new comments — follow-up.".into(),
        follow_ups: vec!["Change resolution policy".into()],
    });
    let update = submitted.into_update();
    assert!(exploration.submit(update.clone()).unwrap());
    assert!(!exploration.submit(update.clone()).unwrap());
    assert_eq!(
        exploration.topics["policy"].status,
        TopicStatus::NeedsFollowUp
    );
    assert_eq!(exploration.answers, vec![request.answer.unwrap()]);
    assert_eq!(exploration.questions, vec![question]);
    let mut changed = update;
    changed
        .conclusion
        .as_mut()
        .unwrap()
        .to_be_implemented
        .push_str(" changed");
    assert!(exploration.submit(changed).is_err());
    for (field, value) in [
        ("next", serde_json::json!({})),
        ("future_work", serde_json::json!([])),
    ] {
        let mut invalid = payload.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<ConclusionSubmission>(invalid).is_err());
    }
}
