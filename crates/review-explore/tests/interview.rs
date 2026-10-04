use review_explore::*;
use review_repository::repository::ChangedFile;
use review_source::{ReviewCheckpoint, SourceLineRange};
use std::sync::Arc;

fn exploration() -> Exploration {
    exploration_at(std::env::temp_dir())
}

/// A round over a change to `policy.rs`, whose working copy is `root`.
fn exploration_at(repository_root: std::path::PathBuf) -> Exploration {
    let path = ChangedFile::modified("policy.rs").review_path().clone();
    Exploration::new(Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        files: vec![ChangedFile::modified("policy.rs")],
        repository_root,
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
                lines: Some(SourceLineRange {
                    first_line: 1,
                    last_line: 2,
                }),
            },
            notes: "This policy determines whether the proposed recovery is sufficient.".into(),
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
        reviewed: Vec::new(),
        reopened: Vec::new(),
        not_relevant: Vec::new(),
        challenger_proposals: Vec::new(),
        interpretation: None,
        topics: vec![],
        design: (request.is_kickoff() && next.is_some()).then(design),
        conclusion: next
            .is_none()
            .then(|| conclusion("Further human file inspection remains required")),
        next,
        limitations: vec![],
        findings: vec![],
    }
}

fn design() -> Design {
    Design::new(
        "A resolution policy decides whether a changed conversation reopens.",
        DesignPart::new(
            "The policy lives in policy.rs.",
            "The change adds a resolution policy to policy.rs.",
        ),
        DesignPart::new(
            "A conversation's state flows through the policy.",
            "A conversation's state flows into the policy, which returns its next state.",
        ),
        DesignPart::new(
            "One comparison per conversation.",
            "One comparison per conversation: constant time.",
        ),
        DesignPart::new(
            "Reopening everything was rejected as noisy.",
            "Reopening every conversation on each change, rejected as noisy.",
        ),
    )
}

/// The agent's design as JSON, to leave out what the tool requires.
fn design_input() -> serde_json::Value {
    serde_json::to_value(design()).unwrap()
}

fn started() -> Exploration {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, Some(question(1)));
    response.topics.push(policy_topic());
    assert!(exploration.apply(response).unwrap());
    exploration
}

fn policy_topic() -> Topic {
    Topic {
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
    }
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
    assert_eq!(exploration.answers.len(), 1);
}

#[test]
fn context_clarification_and_a_conditional_decision_append_exact_answers() {
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
    assert_eq!(retry.request, first.request);
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
        next.evidence[0].location.lines = Some(SourceLineRange {
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
#[path = "interview/cancel.rs"]
mod cancel;
#[path = "interview/challenger.rs"]
mod challenger;
#[path = "interview/not_relevant.rs"]
mod not_relevant;

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

/// A conclusion with `summary`, and an empty quiz.
fn conclusion(summary: &str) -> review_explore::Conclusion {
    review_explore::Conclusion {
        summary: summary.into(),
        quiz_empty_reason: Some("The change holds nothing at whiteboard level.".into()),
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
        "review": "runtime-access", "instance": request.instance, "request": request.request, "checkpoint": request.checkpoint,
        "interpretation": null,
        "summary": "Change the resolution policy; human Files inspection remains required.",
        "to_be_implemented": "Reopen resolved threads when new comments arrive.",
        "future_work": "Review notification volume later.",
        "quiz": [], "quiz_empty_reason": "The change only adjusts one policy rule."
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

fn policy_lines(first_line: u32, last_line: u32) -> CodeLocation {
    CodeLocation {
        path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
        side: SourceSide::Old,
        lines: Some(SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

#[test]
fn marks_follow_a_human_answer_and_name_changed_lines() {
    let mut exploration = exploration();
    let kickoff = exploration.request(None, None).unwrap();
    let mut response = update(&kickoff, Some(question(1)));
    response.reviewed.push(policy_lines(1, 1));
    let error = exploration.apply(response).unwrap_err().to_string();
    assert!(error.contains("follow a human answer"), "{error}");

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
    let mut response = update(&request, Some(self::question(2)));
    response.interpretation = Some(Interpretation {
        answer: request.answer.clone().unwrap().id,
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved".into(),
        follow_ups: vec![],
    });
    response.reviewed.push(policy_lines(1, 9));
    let error = exploration.apply(response.clone()).unwrap_err().to_string();
    assert!(error.contains("valid lines"), "{error}");

    response.reviewed = vec![policy_lines(1, 2)];
    response.reopened = vec![policy_lines(3, 3)];
    // The topic was just decided: conclude rather than ask on it again.
    response.next = None;
    response.conclusion = Some(conclusion("Resolved conversations stay resolved"));
    assert!(exploration.apply(response).unwrap());
}

#[test]
fn the_first_question_comes_after_the_design_of_the_change() {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, Some(question(1)));
    response.topics.push(policy_topic());

    response.design = None;
    assert!(exploration.apply(response.clone()).is_err());
    let mut refused = Vec::new();
    for (path, value) in [
        ("/algorithm/body", serde_json::json!(" \n")),
        ("/thesis", serde_json::json!(" ")),
        ("/data_flow/thesis", serde_json::json!("")),
        (
            "/overview/thesis",
            serde_json::json!("Two lines\nfor one thesis."),
        ),
    ] {
        let mut input = design_input();
        *input.pointer_mut(path).unwrap() = value;
        refused.push(input);
    }
    for field in ["thesis", "overview"] {
        let mut input = design_input();
        let object = if field == "thesis" {
            input.as_object_mut().unwrap()
        } else {
            input[field].as_object_mut().unwrap()
        };
        object.remove("thesis");
        refused.push(input);
    }
    for input in refused {
        response.design = Some(serde_json::from_value(input.clone()).unwrap());
        assert!(exploration.apply(response.clone()).is_err(), "{input}");
    }
    assert!(exploration.design().is_none());

    response.design = Some(design());
    assert!(exploration.apply(response).unwrap());
    assert_eq!(exploration.design(), Some(&design()));
}

#[test]
fn a_change_that_raises_no_question_concludes_without_a_design() {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();

    assert!(exploration.apply(update(&request, None)).unwrap());
    assert!(exploration.design().is_none());
}

#[test]
fn only_the_first_turn_explains_the_design() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "Why compare only once?".into(),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut response = update(&request, Some(self::question(2)));
    let mut revised = design_input();
    revised["overview"]["body"] = "A revised design.".into();
    response.design = Some(serde_json::from_value(revised).unwrap());

    assert!(exploration.apply(response.clone()).is_err());
    response.design = None;
    assert!(exploration.apply(response).unwrap());
    assert_eq!(exploration.design(), Some(&design()));
}
#[path = "interview/diagram.rs"]
mod diagram;
#[path = "interview/quiz.rs"]
mod quiz;
#[path = "interview/writing.rs"]
mod writing;

#[test]
fn a_first_pick_is_saved_with_the_answer_when_it_names_a_choice() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let refused = exploration.clone().request(
        Some(AnswerInput {
            option: Some("keep".into()),
            first_pick: Some("elsewhere".into()),
            ..AnswerInput::default()
        }),
        Some(&question),
    );
    assert!(refused.is_err());

    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                first_pick: Some("none-of-the-above".into()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();

    let answer = request.answer.unwrap();
    assert_eq!(answer.first_pick.as_deref(), Some("none-of-the-above"));
    assert_eq!(exploration.answers, vec![answer]);
}
