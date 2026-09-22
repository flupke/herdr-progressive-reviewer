use super::*;
use std::fmt::Write as _;

fn publish(fixture: &mut ExploreUi, response: InterviewUpdate) {
    fixture.app.publish(ExploreFinished {
        instance: response.instance.clone(),
        request: response.request.clone(),
        result: Ok(response),
    });
}

fn source_fixture() -> (ExploreUi, TurnRequest) {
    let mut base = String::new();
    for line in 1..=70 {
        writeln!(base, "// source line {line:02}").unwrap();
    }
    let policy = base.replace("line 65", "changed line 65");
    let (mut fixture, request) = ExploreUi::with_versions(base.as_bytes(), policy.as_bytes());
    fixture.app.update(UserInput::Resize {
        width: 120,
        height: 100,
    });
    (fixture, request)
}

fn primary_range(
    fixture: &ExploreUi,
    request: &TurnRequest,
    side: review_explore::SourceSide,
) -> InterviewUpdate {
    let mut response = fixture.response(request, 1);
    let question = response.next.as_mut().unwrap();
    question.evidence.truncate(1);
    question.evidence[0].location.side = side;
    question.evidence[0].location.lines = Some(GuideLineRange {
        first_line: 20,
        last_line: 35,
    });
    response
}

fn assert_closed_outline(buffer: &Buffer) {
    let rows: Vec<_> = buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .collect();
    let corners = |symbol: &str| {
        rows.iter()
            .enumerate()
            .flat_map(|(row, cells)| {
                cells.iter().enumerate().filter_map(move |(column, cell)| {
                    (cell.symbol() == symbol && cell.fg == ratatui::style::Color::Yellow)
                        .then_some((row, column))
                })
            })
            .collect::<Vec<_>>()
    };
    let top = corners("╭");
    let bottom = corners("╰");
    let right = corners("╮");
    assert_eq!(top.len(), 1, "one opening edge");
    assert_eq!(bottom.len(), 1, "one closing edge");
    assert_eq!(right.len(), 1, "one top-right corner");
    assert_eq!(corners("╯"), vec![(bottom[0].0, right[0].1)]);
    assert_eq!(top[0].1, bottom[0].1);
    for (row, cells) in rows.iter().enumerate().take(bottom[0].0).skip(top[0].0 + 1) {
        for column in [top[0].1, right[0].1] {
            assert_eq!(cells[column].symbol(), "│", "border gap at {row}:{column}");
            assert_eq!(cells[column].fg, ratatui::style::Color::Yellow);
        }
    }
}

#[test]
fn evidence_outlines_enclose_interleaved_diff_sides_and_wrapped_rows() {
    let base = format!(
        "// before\npub fn policy() {{\n    let old_first = \"{}\";\n    checkpoint_a();\n    removed_call();\n    checkpoint_b();\n}}\n// after\n",
        "old wide 字 text ".repeat(8)
    );
    let current = base
        .replace("old_first", "new_first")
        .replace("removed_call", "added_call");
    for side in [
        review_explore::SourceSide::New,
        review_explore::SourceSide::Old,
    ] {
        let (mut fixture, request) = ExploreUi::with_versions(base.as_bytes(), current.as_bytes());
        fixture.app.update(UserInput::Resize {
            width: 60,
            height: 120,
        });
        let mut response = primary_range(&fixture, &request, side);
        response.next.as_mut().unwrap().evidence[0].location.lines = Some(GuideLineRange {
            first_line: 2,
            last_line: 7,
        });
        publish(&mut fixture, response);
        let text = fixture.text();
        for line in ["old_first", "new_first", "removed_call", "added_call"] {
            assert!(text.contains(line), "missing {line}: {text}");
        }
        assert_closed_outline(&fixture.buffer());
    }
}

#[test]
fn overlapping_evidence_ranges_share_one_closed_outline() {
    for reverse in [false, true] {
        let (mut fixture, request) = source_fixture();
        let mut response = primary_range(&fixture, &request, review_explore::SourceSide::New);
        let question = response.next.as_mut().unwrap();
        question.evidence[0].location.lines = Some(GuideLineRange {
            first_line: 20,
            last_line: 30,
        });
        let mut overlapping = question.evidence[0].clone();
        overlapping.location.lines = Some(GuideLineRange {
            first_line: 25,
            last_line: 35,
        });
        overlapping.relationship = "Overlapping decision evidence".into();
        question.evidence.push(overlapping);
        if reverse {
            question.evidence.reverse();
        }
        publish(&mut fixture, response);
        for _ in 0..2 {
            assert_closed_outline(&fixture.buffer());
            let text = fixture.text();
            for line in [20, 35] {
                assert!(text.contains(&format!("source line {line}")), "{text}");
            }
            fixture.click("Fit evidence");
        }
    }
}

#[test]
fn base_evidence_outside_hunks_uses_full_historical_text_and_old_coordinates() {
    let (mut fixture, request) = source_fixture();
    let response = primary_range(&fixture, &request, review_explore::SourceSide::Old);
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(text.contains("policy.rs · Base"), "{text}");
    assert!(
        text.contains("source line 20") && text.contains("source line 35"),
        "{text}"
    );
    assert!(!text.contains("outside the displayed diff"));
    fixture.click("Fit evidence");
    let actions = fixture.app.update(UserInput::Key(Key::Char('K')));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Lsp { .. }))
    );
    assert!(fixture.text().contains("source line 35"));
    fixture.app.publish(ui_events::SourceLocationAccepted {
        location: review_lsp::SourceLocation {
            path: fixture.files.root().join("caller.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
    });
    assert!(fixture.text().contains("pub fn caller()"));
    fixture.click("Primary");
    assert!(fixture.text().contains("source line 35"));
}

#[test]
fn uncataloged_base_citation_displays_history_instead_of_live_source() {
    let (mut fixture, request) = ExploreUi::new();
    fixture
        .files
        .write("caller.rs", b"pub fn current_caller() {}\n");
    let mut response = fixture.response(&request, 1);
    let question = response.next.as_mut().unwrap();
    question.evidence.truncate(1);
    question.evidence[0].location.path =
        review_repository::repository::RepoPath::from_bytes(b"caller.rs");
    question.evidence[0].location.side = review_explore::SourceSide::Old;
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(text.contains("caller.rs · Base"), "{text}");
    assert!(text.contains("pub fn caller()"), "{text}");
    assert!(!text.contains("current_caller"), "{text}");
}

#[test]
fn fit_reveals_both_ends_and_outlines_instead_of_centering_the_first_line() {
    let (mut fixture, request) = source_fixture();
    let response = primary_range(&fixture, &request, review_explore::SourceSide::New);
    publish(&mut fixture, response);
    for _ in 0..2 {
        let buffer = fixture.buffer();
        let text = fixture.text();
        for line in [17, 20, 35, 38] {
            assert!(text.contains(&format!("source line {line}")), "{text}");
        }
        for corner in ["╭", "╰"] {
            assert!(buffer.content.iter().any(|cell| cell.symbol() == corner && cell.fg == ratatui::style::Color::Yellow));
        }
        fixture.click("Fit evidence");
        fixture.app.update(UserInput::Key(Key::Down));
        fixture.app.update(UserInput::Key(Key::Alt('0')));
    }
}

#[test]
fn only_decision_evidence_is_in_the_primary_cycle_and_supporting_sources_stay_available() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 90,
    });
    let mut response = fixture.response(&request, 1);
    let question = response.next.as_mut().unwrap();
    question.evidence.truncate(1);
    let mut supporting = question.evidence[0].clone();
    supporting.location.path = review_repository::repository::RepoPath::from_bytes(b"caller.rs");
    supporting.relationship = "Supporting caller context".into();
    supporting.decision_relevance.clear();
    question.supporting.push(supporting.clone());
    response.reply.as_mut().unwrap().evidence.push(supporting);
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(text.contains("Establishes") && text.contains("Policy behavior"));
    assert!(text.contains("For your answer"));
    assert!(text.contains("Evidence 1/1"));
    assert!(text.contains("Supporting sources 1"));
    assert!(!text.contains("Supporting caller context"));
    assert!(text.contains("1. Keep resolved") && text.contains("2. Inspect the caller"));
    fixture.app.update(UserInput::Key(Key::Char('e')));
    assert!(fixture.text().contains("Evidence 1/1"));
    fixture.click("Supporting sources 1");
    fixture.click("Supporting caller context");
    assert!(fixture.text().contains("pub fn caller()"));
}

#[test]
fn fit_accounts_for_every_wrapped_line_of_the_relevant_range() {
    let base: String = (1..=45)
        .map(|line| {
            if (20..=22).contains(&line) {
                format!(
                    "// LINE_{line} {} END_{line}\n",
                    "wrapped context ".repeat(8)
                )
            } else {
                format!("// line {line}\n")
            }
        })
        .collect();
    let current = base.replace("line 40", "changed line 40");
    let (mut fixture, request) = ExploreUi::with_versions(base.as_bytes(), current.as_bytes());
    fixture.app.update(UserInput::Resize {
        width: 60,
        height: 100,
    });
    let mut response = primary_range(&fixture, &request, review_explore::SourceSide::New);
    response.next.as_mut().unwrap().evidence[0]
        .location
        .lines
        .as_mut()
        .unwrap()
        .last_line = 22;
    publish(&mut fixture, response);
    let text = fixture.text();
    assert!(
        text.contains("LINE_20") && text.contains("END_22"),
        "{text}"
    );
    assert!(
        fixture
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol() == "╰" && cell.fg == ratatui::style::Color::Yellow)
    );
}

#[test]
fn mcp_submission_is_acknowledged_only_after_validation_and_retries_are_idempotent() {
    let (mut fixture, request) = ExploreUi::new();
    let valid = fixture.response(&request, 1);
    let (response, result) = std::sync::mpsc::channel();
    for count in [0, 1] {
        let mut invalid = valid.clone();
        invalid.next.as_mut().unwrap().alternatives.truncate(count);
        fixture.app.publish(ui_events::ExploreSubmission {
            update: invalid,
            response: response.clone(),
        });
        assert!(result.recv().unwrap().unwrap_err().contains("two to five"));
        assert!(!fixture.text().contains("Question 1:"));
    }
    let mut invalid = valid.clone();
    invalid.next.as_mut().unwrap().evidence[0]
        .decision_relevance
        .clear();
    fixture.app.publish(ui_events::ExploreSubmission {
        update: invalid,
        response: response.clone(),
    });
    assert!(
        result
            .recv()
            .unwrap()
            .unwrap_err()
            .contains("how it could change the answer")
    );
    assert!(!fixture.text().contains("Question 1:"));
    for applied in [true, false] {
        let actions = fixture.app.publish(ui_events::ExploreSubmission {
            update: valid.clone(),
            response: response.clone(),
        });
        assert_eq!(result.recv().unwrap().unwrap(), applied);
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::SetReviewed { .. } | Action::Thread(_)))
        );
        assert_eq!(fixture.text().matches("Question 1:").count(), 1);
    }
    let mut changed = valid;
    changed.next.as_mut().unwrap().text = "Rewritten question".into();
    fixture.app.publish(ui_events::ExploreSubmission {
        update: changed,
        response,
    });
    assert!(result.recv().unwrap().is_err());
    assert!(!fixture.text().contains("Rewritten question"));
}
