use super::*;

fn topic(id: &str, prerequisites: &[&str]) -> Topic {
    Topic {
        id: id.into(),
        title: id.into(),
        prompt: format!("Investigate {id}?"),
        prerequisites: prerequisites.iter().map(|id| (*id).into()).collect(),
        ..Topic::default()
    }
}
fn next(topic: &str, version: u32) -> Question {
    Question {
        id: topic.into(),
        topic: topic.into(),
        ..question(version)
    }
}
fn evidence() -> EvidenceRef {
    question(1).evidence.remove(0)
}
fn assessments(door: Door) -> Assessments {
    Assessments {
        door,
        reversibility: Consequence {
            summary: "Cache can be rebuilt".into(),
            details: "Only while authoritative reads and rebuild capacity remain available".into(),
            evidence: vec![evidence()],
            unknowns: vec![],
        },
        blast_radius: Consequence {
            summary: "Concurrent cache misses can overload the shared origin".into(),
            details:
                "Every instance uses the origin; local rollback cannot undo requests already sent"
                    .into(),
            evidence: vec![evidence()],
            unknowns: vec!["Deployment concurrency and origin capacity".into()],
        },
    }
}
fn contribute(exploration: &mut Exploration, text: &str) -> TurnRequest {
    let question = exploration.questions.last().unwrap().clone();
    exploration
        .request(
            Some(AnswerInput {
                text: text.into(),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap()
}
fn retirement(topic: &str, request: &TurnRequest) -> AgendaChange {
    AgendaChange {
        topic: topic.into(),
        action: AgendaAction::Retire,
        reason: "Rebuild path confirms disposable cache; durable-data migration does not apply"
            .into(),
        answer: request.answer.as_ref().map(|answer| answer.id.clone()),
        evidence: vec![evidence()],
        replacement: None,
        decision: None,
    }
}

#[test]
fn verified_domain_context_retires_only_the_dependent_inquiry_and_changes_investigation() {
    let mut exploration = started();
    let request = contribute(&mut exploration, "What depends on the stored format?");
    let mut response = update(&request, Some(question(2)));
    response.topics = vec![topic("migration", &["policy"]), topic("availability", &[])];
    response.next.as_mut().unwrap().assessments = Some(assessments(Door::OneWay));
    exploration.apply(response).unwrap();
    let original = exploration.questions.clone();
    let request = contribute(
        &mut exploration,
        "These files are disposable cache. The authoritative data is elsewhere.",
    );
    let mut response = update(&request, Some(next("availability", 1)));
    response.reply.as_mut().unwrap().text =
        "The rebuild path confirms that context. Rebuild capacity still needs investigation."
            .into();
    response.agenda = vec![retirement("migration", &request)];
    response.next.as_mut().unwrap().assessments = Some(assessments(Door::TwoWay));
    let duplicate = response.clone();
    assert!(exploration.apply(response).unwrap());
    assert!(!exploration.apply(duplicate).unwrap());
    assert_eq!(exploration.agenda_label("migration"), "Retired");
    assert_eq!(exploration.topics["migration"].status, TopicStatus::Open);
    assert_eq!(exploration.agenda_label("availability"), "Outstanding");
    assert_eq!(&exploration.questions[..2], original);
    assert!(exploration.interpretations.is_empty());
    let next_request = contribute(
        &mut exploration,
        "What happens if every instance rebuilds at once?",
    );
    assert_eq!(exploration.conversation.len(), 3);
    assert_eq!(
        next_request.answer.as_ref().unwrap().question,
        exploration.questions.last().cloned()
    );
    assert_eq!(
        exploration.answers[1].text,
        "These files are disposable cache. The authoritative data is elsewhere."
    );
    assert!(
        exploration.conversation[2].update.agenda[0]
            .reason
            .contains("Rebuild")
    );
}

#[test]
fn factual_question_gets_a_direct_reply_new_branch_and_new_source_without_agreement() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("origin.rs"), "shared_origin.fetch()\n").unwrap();
    let mut exploration = started();
    Arc::make_mut(&mut exploration.comparison).repository_root = root.path().into();
    let request = contribute(
        &mut exploration,
        "What happens if every instance rebuilds at once?",
    );
    let mut response = update(&request, Some(next("shared-origin", 1)));
    response.topics.push(topic("shared-origin", &[]));
    let reference = EvidenceRef {
        location: review_explore::CodeLocation {
            path: review_repository::repository::RepoPath::from_bytes(b"origin.rs"),
            side: review_explore::SourceSide::New,
            lines: Some(GuideLineRange {
                first_line: 1,
                last_line: 1,
            }),
        },
        relationship: "Every rebuild reaches the shared dependency".into(),
        decision_relevance: "This policy determines whether the proposed recovery is sufficient."
            .into(),
    };
    response.reply = Some(Reply {
        text: "Each instance fetches from the same origin. No fleet-wide limiter is shown.".into(),
        evidence: vec![reference.clone()],
    });
    response.next.as_mut().unwrap().evidence = vec![reference.clone()];
    exploration.apply(response).unwrap();
    assert!(exploration.interpretations.is_empty());
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Open);
    assert!(exploration.evidence(0).contains(&reference));
    assert_eq!(
        exploration
            .comparison
            .source(&reference.location)
            .unwrap()
            .read_text(root.path())
            .unwrap(),
        "shared_origin.fetch()\n"
    );
    let request = contribute(&mut exploration, "Show me that caller");
    assert_eq!(request.answer.as_ref().unwrap().text, "Show me that caller");
    assert!(
        exploration.conversation[1]
            .update
            .reply
            .as_ref()
            .unwrap()
            .text
            .contains("same origin")
    );
}

#[test]
fn correction_flags_reconsideration_without_erasing_the_original_decision() {
    let mut exploration = started();
    let request = contribute(&mut exploration, "Keep it resolved.");
    let original_answer = request.answer.as_ref().unwrap().id.clone();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: original_answer.clone(),
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved".into(),
        follow_ups: vec![],
    });
    exploration.apply(response).unwrap();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "Correction: these readers cannot rebuild locally.".into(),
                corrects: Some(original_answer.clone()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut response = update(&request, Some(self::question(2)));
    response.agenda.push(AgendaChange {
        action: AgendaAction::Reconsider,
        decision: Some(original_answer.clone()),
        ..retirement("policy", &request)
    });
    exploration.apply(response).unwrap();
    assert_eq!(exploration.agenda_label("policy"), "Reconsider");
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Accepted);
    assert_eq!(exploration.interpretations.len(), 1);
    assert_eq!(exploration.answers[0].text, "Keep it resolved.");
    assert_eq!(
        exploration.answers[1].corrects.as_ref(),
        Some(&original_answer)
    );
    let request = contribute(
        &mut exploration,
        "Keep it only if the remote rebuild is bounded.",
    );
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: request.answer.unwrap().id,
        status: TopicStatus::NeedsFollowUp,
        recap: "Recorded: conditional on bounded rebuild".into(),
        follow_ups: vec!["Bound remote rebuild requests".into()],
    });
    exploration.apply(response).unwrap();
    assert_eq!(exploration.agenda_label("policy"), "Follow-up required");
    assert_eq!(exploration.interpretations.len(), 2);
}

#[test]
fn invalid_agenda_update_is_atomic_and_cancelled_turn_cannot_resurrect_a_branch() {
    let mut exploration = started();
    let request = contribute(&mut exploration, "This is disposable cache");
    let mut response = update(&request, Some(next("availability", 1)));
    response.topics.push(topic("availability", &[]));
    response.agenda.push(retirement("unknown", &request));
    let before = exploration.topics.clone();
    assert!(exploration.apply(response.clone()).is_err());
    assert_eq!(exploration.topics, before);
    assert_eq!(exploration.conversation.len(), 1);
    response.agenda[0].topic = "policy".into();
    exploration.cancel();
    assert!(!exploration.apply(response.clone()).unwrap());
    let retry = exploration.retry().unwrap();
    response.request = retry.request;
    assert!(exploration.apply(response.clone()).unwrap());
    assert!(!exploration.apply(response).unwrap());
    let request = contribute(&mut exploration, "What is still open?");
    let response = update(&request, Some(question(2)));
    assert!(exploration.apply(response).is_err());
    assert_eq!(exploration.agenda_label("policy"), "Retired");
}

#[test]
fn unsupported_reversibility_stays_unknown_and_blast_radius_remains_independent() {
    let mut exploration = started();
    let request = contribute(&mut exploration, "Can we undo the effects?");
    let mut response = update(&request, Some(question(2)));
    let mut assessment = assessments(Door::TwoWay);
    assessment.reversibility.evidence.clear();
    assessment
        .reversibility
        .unknowns
        .push("Origin durability has not been established".into());
    response.next.as_mut().unwrap().assessments = Some(assessment.clone());
    assert!(exploration.apply(response.clone()).is_err());
    assessment.door = Door::Unknown;
    response.next.as_mut().unwrap().assessments = Some(assessment);
    exploration.apply(response).unwrap();
    let assessment = exploration.questions[1].assessments.as_ref().unwrap();
    assert_eq!(assessment.door, Door::Unknown);
    assert!(assessment.blast_radius.summary.contains("overload"));
}

#[test]
fn superseding_preserves_pending_wording_and_deferral_remains_outstanding() {
    let mut exploration = started();
    let request = contribute(
        &mut exploration,
        "Check availability instead of format migration.",
    );
    let mut response = update(&request, Some(next("availability", 1)));
    response.topics.push(topic("availability", &[]));
    response.agenda.push(AgendaChange {
        action: AgendaAction::Supersede,
        replacement: Some("availability".into()),
        ..retirement("policy", &request)
    });
    exploration.apply(response).unwrap();
    assert_eq!(exploration.agenda_label("policy"), "Superseded");
    assert_eq!(exploration.questions[0].text, question(1).text);
    let question = exploration.questions[1].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                deferred: true,
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: request.answer.unwrap().id,
        status: TopicStatus::Deferred,
        recap: "Deferred availability".into(),
        follow_ups: vec![],
    });
    exploration.apply(response).unwrap();
    assert_eq!(
        exploration.agenda_label("availability"),
        "Deferred · outstanding"
    );
}

#[test]
fn unknown_reason_decision_and_cyclic_prerequisites_are_rejected() {
    let mut exploration = started();
    let request = contribute(&mut exploration, "Investigate recovery");
    let mut response = update(&request, Some(question(2)));
    response.topics = vec![topic("a", &["b"]), topic("b", &["a"])];
    assert!(exploration.apply(response.clone()).is_err());
    response.topics.clear();
    let mut change = retirement("policy", &request);
    change.answer = Some("wrong-input".into());
    response.agenda.push(change);
    assert!(exploration.apply(response.clone()).is_err());
    response.agenda[0] = AgendaChange {
        action: AgendaAction::Reconsider,
        decision: Some("invented-agreement".into()),
        ..retirement("policy", &request)
    };
    assert!(exploration.apply(response).is_err());
    assert_eq!(exploration.topics.len(), 1);
}

#[test]
fn direct_citations_reject_unsafe_paths_and_invalid_lines_without_changing_history() {
    use review_repository::repository::RepoPath;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("context.rs"), "context\n").unwrap();
    std::fs::write(outside.path().join("external.rs"), "external\n").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("external.rs"),
        root.path().join("link.rs"),
    )
    .unwrap();
    let mut exploration = started();
    Arc::make_mut(&mut exploration.comparison).repository_root = root.path().into();
    let request = contribute(&mut exploration, "Inspect more context");
    let mut response = update(&request, Some(question(2)));
    for path in [
        b"../external.rs".as_slice(),
        b"/etc/passwd",
        b"link.rs",
        b"missing.rs",
        b".",
    ] {
        let mut invalid = response.clone();
        let mut evidence = question(1).evidence.remove(0);
        evidence.location.path = RepoPath::from_bytes(path);
        evidence.location.side = SourceSide::New;
        evidence.location.lines = None;
        invalid.reply.as_mut().unwrap().evidence.push(evidence);
        assert!(exploration.apply(invalid).is_err(), "{path:?}");
        assert_eq!(exploration.conversation.len(), 1);
    }
    std::fs::write(root.path().join("image.bin"), b"\0binary").unwrap();
    let mut evidence = question(1).evidence.remove(0);
    evidence.location.path = RepoPath::from_bytes(b"image.bin");
    evidence.location.side = SourceSide::New;
    evidence.location.lines = None;
    evidence.relationship = "Non-text source; inspection unavailable".into();
    response.reply.as_mut().unwrap().evidence.push(evidence);
    exploration.apply(response).unwrap();
    assert_eq!(exploration.conversation.len(), 2);
}

#[test]
fn deferred_pending_metadata_can_evolve_without_changing_its_decision_or_posted_text() {
    let mut exploration = started();
    let original = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                deferred: true,
                ..AnswerInput::default()
            }),
            Some(&original),
        )
        .unwrap();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: request.answer.unwrap().id,
        status: TopicStatus::Deferred,
        recap: "Deferred; deployment context missing".into(),
        follow_ups: vec![],
    });
    exploration.apply(response).unwrap();
    let request = contribute(
        &mut exploration,
        "The deployment changed; prioritize compatibility",
    );
    let mut response = update(&request, Some(question(2)));
    let mut pending = exploration.topics["policy"].clone();
    pending.prompt = "Can mixed versions share cache files?".into();
    pending.rank = 4;
    response.topics.push(pending.clone());
    exploration.apply(response).unwrap();
    assert_eq!(exploration.topics["policy"], pending);
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Deferred);
    assert_eq!(exploration.questions[0], original);
    assert_eq!(exploration.interpretations.len(), 1);
}

#[test]
fn a_questionless_contribution_is_bound_to_its_closing_turn_and_cannot_invent_a_decision() {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    exploration.apply(update(&request, None)).unwrap();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "These files are disposable cache".into(),
                ..AnswerInput::default()
            }),
            None,
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap();
    assert!(answer.question.is_none());
    assert_eq!(
        answer.in_reply_to,
        exploration.conversation[0].update.request
    );
    let mut response = update(&request, Some(question(1)));
    response.topics.push(topic("policy", &[]));
    response.interpretation = Some(Interpretation {
        answer: answer.id.clone(),
        status: TopicStatus::Accepted,
        recap: "Accepted".into(),
        follow_ups: vec![],
    });
    assert!(exploration.apply(response.clone()).is_err());
    response.interpretation = None;
    exploration.apply(response).unwrap();
    assert_eq!(exploration.questions.len(), 1);
}

#[test]
fn repeating_a_recorded_finding_on_a_new_turn_does_not_duplicate_required_work() {
    let mut exploration = started();
    for version in [2, 3] {
        let request = contribute(&mut exploration, "What remains to inspect?");
        let mut response = update(&request, Some(question(version)));
        response.findings = vec!["Require a list-valued records field".into()];
        exploration.apply(response).unwrap();
    }
    assert_eq!(
        exploration.findings,
        vec!["Require a list-valued records field"]
    );
    assert_eq!(exploration.conversation.len(), 3);
}
