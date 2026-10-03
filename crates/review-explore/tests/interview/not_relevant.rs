use super::*;
use review_repository::repository::RepoPath;

/// A not-relevant mark on lines of the changed file.
fn mark(reason: Option<NotRelevantReason>, test: Option<TestLocation>) -> NotRelevantMark {
    NotRelevantMark {
        location: policy_lines(1, 2),
        reason,
        test,
    }
}

fn test_lines(path: &str, first_line: u32, last_line: u32) -> TestLocation {
    TestLocation {
        path: RepoPath::from_bytes(path.as_bytes()),
        lines: SourceLineRange {
            first_line,
            last_line,
        },
    }
}

/// The error the kickoff turn gets for `marks`, or `None` when it is
/// accepted. The working copy holds a three-line `tests/policy.rs`.
fn kickoff_error(marks: Vec<NotRelevantMark>) -> Option<String> {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("tests")).unwrap();
    std::fs::write(root.path().join("tests/policy.rs"), "a\nb\nc\n").unwrap();
    let mut exploration = exploration_at(root.path().to_owned());
    let kickoff = exploration.request(None, None).unwrap();
    let mut response = update(&kickoff, Some(question(1)));
    response.topics.push(Topic {
        id: "policy".into(),
        title: "Resolution".into(),
        ..Topic::default()
    });
    response.not_relevant = marks;
    exploration
        .apply(response)
        .err()
        .map(|error| error.to_string())
}

#[test]
fn a_not_relevant_mark_needs_a_reason() {
    let error = kickoff_error(vec![mark(None, None)]).unwrap();

    assert!(error.contains("policy.rs old 1-2"), "{error}");
    for reason in [
        NotRelevantReason::RemovedCode,
        NotRelevantReason::FollowsCode,
    ] {
        assert_eq!(kickoff_error(vec![mark(Some(reason), None)]), None);
        // Any reason may name a test, which must exist.
        let test = |path| Some(test_lines(path, 1, 1));
        assert_eq!(
            kickoff_error(vec![mark(Some(reason), test("tests/policy.rs"))]),
            None
        );
        let error = kickoff_error(vec![mark(Some(reason), test("tests/missing.rs"))]).unwrap();
        assert!(error.contains("policy.rs old 1-2"), "{error}");
    }
}

#[test]
fn tested_mechanics_name_a_test_that_exists_at_the_checkpoint() {
    let tested = |test| mark(Some(NotRelevantReason::TestedMechanics), test);

    assert_eq!(
        kickoff_error(vec![tested(Some(test_lines("tests/policy.rs", 2, 3)))]),
        None
    );
    for test in [
        None,
        Some(test_lines("tests/missing.rs", 1, 1)),
        Some(test_lines("tests/policy.rs", 3, 4)),
    ] {
        let error = kickoff_error(vec![tested(test.clone())]).unwrap();
        assert!(error.contains("policy.rs old 1-2"), "{test:?}: {error}");
    }
}

#[test]
fn a_round_saved_before_marks_had_reasons_loads_as_before() {
    let mut saved = serde_json::to_value(ExploreRound::new(started())).unwrap();
    let request = saved["exploration"]["conversation"][0]["update"]["request"].clone();
    let legacy = serde_json::json!([{
        "path": "policy.rs", "side": "old", "lines": {"first_line": 1, "last_line": 2}
    }]);
    saved["exploration"]["conversation"][0]["update"]["not_relevant"] = legacy.clone();
    saved["marks"] = serde_json::json!({request.as_str().unwrap(): {
        "answer": null, "reviewed": [], "not_relevant": legacy, "reopened": [], "problem": null
    }});

    let round: ExploreRound = serde_json::from_value(saved.clone()).unwrap();

    let unexplained = vec![mark(None, None)];
    assert_eq!(
        round.exploration.conversation[0].update.not_relevant,
        unexplained
    );
    assert_eq!(
        round.marks.values().next().unwrap().not_relevant,
        unexplained
    );
    assert_eq!(serde_json::to_value(&round).unwrap(), saved);
}

#[test]
fn a_summary_names_only_what_changed() {
    use MarkTense::Applied;
    let counts = |reviewed_lines, reviewed_files, reopened_lines| MarkCounts {
        reviewed_lines,
        reviewed_files,
        reopened_lines,
        ..MarkCounts::default()
    };

    assert_eq!(counts(0, 0, 0).summary(Applied), "");
    assert_eq!(counts(0, 0, 2).summary(Applied), "Reopened 2 lines");
    assert_eq!(
        counts(3, 1, 0).summary(Applied),
        "Marked 3 lines and 1 whole file reviewed"
    );
    let not_relevant = MarkCounts {
        not_relevant_lines: 5,
        not_relevant_files: 1,
        ..counts(0, 0, 2)
    };
    assert_eq!(
        not_relevant.summary(Applied),
        "Marked 5 lines and 1 whole file not relevant · reopened 2 lines"
    );
    assert_eq!(
        not_relevant.summary(MarkTense::Pending),
        "Will mark 5 lines and 1 whole file not relevant · reopen 2 lines"
    );
}
