use super::*;

fn proposal(title: &str, result: ProposalResult, reason: Option<&str>) -> ChallengerProposal {
    ChallengerProposal {
        title: title.into(),
        result,
        reason: reason.map(Into::into),
    }
}

/// A round with or without a Challenger, and its kickoff turn reporting
/// `proposals`.
fn kickoff(challenger: bool, proposals: Vec<ChallengerProposal>) -> (Exploration, InterviewUpdate) {
    let mut exploration = exploration();
    exploration.challenger = challenger;
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, Some(question(1)));
    response.topics.push(Topic {
        id: "policy".into(),
        title: "Resolution".into(),
        ..Topic::default()
    });
    response.challenger_proposals = proposals;
    (exploration, response)
}

#[test]
fn a_turn_saves_what_became_of_the_challengers_proposals() {
    let proposals = vec![
        proposal("Lock choice", ProposalResult::Asked, None),
        proposal("Retry policy", ProposalResult::Merged, None),
        proposal(
            "Log format",
            ProposalResult::Retired,
            Some("policy.rs new 2 already logs it"),
        ),
        proposal("Timeouts", ProposalResult::Kept, None),
    ];
    let (mut exploration, response) = kickoff(true, proposals.clone());

    assert!(exploration.apply(response).unwrap());
    let round: ExploreRound =
        serde_json::from_value(serde_json::to_value(ExploreRound::new(exploration)).unwrap())
            .unwrap();

    assert_eq!(
        round.exploration.conversation[0]
            .update
            .challenger_proposals,
        proposals
    );
}

#[test]
fn a_retired_proposal_needs_its_reason_and_every_proposal_a_title() {
    for (proposals, expected) in [
        (
            vec![proposal("Log format", ProposalResult::Retired, None)],
            "Log format",
        ),
        (
            vec![proposal("Log format", ProposalResult::Retired, Some("  "))],
            "Log format",
        ),
        (vec![proposal(" ", ProposalResult::Asked, None)], "title"),
    ] {
        let (mut exploration, response) = kickoff(true, proposals);

        let error = exploration.apply(response).unwrap_err().to_string();

        assert!(error.contains(expected), "{error}");
        assert!(exploration.conversation.is_empty());
    }
}

#[test]
fn only_a_round_with_a_challenger_takes_proposals() {
    let (mut exploration, response) =
        kickoff(false, vec![proposal("Lock", ProposalResult::Asked, None)]);

    assert!(exploration.apply(response).is_err());
    assert!(exploration.conversation.is_empty());
}

#[test]
fn a_conclusion_reports_proposals_too() {
    let mut exploration = exploration();
    exploration.challenger = true;
    let request = exploration.request(None, None).unwrap();
    let proposals = vec![proposal(
        "Log format",
        ProposalResult::Retired,
        Some("Settled by the logging test"),
    )];
    let submission: ConclusionSubmission = serde_json::from_value(serde_json::json!({
        "review": "access", "instance": request.instance, "request": request.request,
        "checkpoint": request.checkpoint, "interpretation": null,
        "summary": "Nothing to decide.", "to_be_implemented": "", "future_work": "",
        "challenger_proposals": proposals,
    }))
    .unwrap();

    assert!(exploration.submit(submission.into_update()).unwrap());
    assert_eq!(
        exploration.conversation[0].update.challenger_proposals,
        proposals
    );
}

#[test]
fn a_turn_without_proposals_saves_as_before() {
    let (mut exploration, response) = kickoff(false, vec![]);
    exploration.apply(response).unwrap();

    let saved = serde_json::to_value(&exploration.conversation[0].update).unwrap();

    assert!(saved.get("challenger_proposals").is_none(), "{saved}");
}
