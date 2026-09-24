use super::*;

#[test]
fn coverage_overview_opens_a_gap_without_losing_the_question_draft() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    assert!(fixture.text().contains("Coverage 0% of required lines"));
    fixture.app.update(UserInput::Key(Key::Char('1')));
    fixture
        .app
        .update(UserInput::Paste("Keep this draft".into()));
    fixture.click("Coverage 0% of required lines");
    assert!(fixture.text().contains("Needs answers"));
    fixture.click("Next unexplored region");
    assert_eq!(fixture.app.navigation, ReviewNavigation::Explore);
    let coverage = fixture
        .app
        .event_bus
        .get::<DiffComponent>(fixture.app.diff_component)
        .unwrap();
    assert_eq!(
        coverage
            .evidence_view(EvidenceView::Coverage)
            .and_then(DiffComponent::evidence_path),
        Some("policy.rs")
    );
    assert!(fixture.text().contains("File diff · policy.rs"));
    assert!(fixture.text().contains("Coverage 0% of required lines"));
    fixture.click("Close diff");
    fixture.click("Coverage 0% of required lines");
    assert!(fixture.text().contains("Keep this draft"));
}

#[test]
fn coverage_keyboard_opens_the_pass_diff_without_editing_the_answer() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Key(Key::Char('g')));
    fixture.app.update(UserInput::Key(Key::Alt('n')));
    assert!(fixture.text().contains("File diff · policy.rs"));
    assert_eq!(fixture.app.navigation, ReviewNavigation::Explore);
}

#[test]
fn coverage_control_reveals_overview_from_a_scrolled_question_and_restores_scroll() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 16,
    });
    for _ in 0..8 {
        fixture.app.update(UserInput::Key(Key::PageDown));
    }
    let before = fixture.text();
    assert!(!before.contains("of required changed lines answered"));
    fixture.click("Coverage 0% of required lines");
    let overview = fixture.text();
    assert!(
        overview.contains("0% of required changed lines answered"),
        "{overview}"
    );
    assert!(overview.contains("Needs answers"), "{overview}");
    assert!(!overview.contains("Question 1 ·"));
    assert!(
        fixture
            .buffer()
            .content
            .chunks(usize::from(fixture.app.width))
            .any(|row| row
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .contains("┌ Coverage "))
    );
    fixture.click("Coverage 0% of required lines");
    assert_eq!(fixture.text(), before);
}

#[test]
fn show_file_diff_reveals_the_selected_diff_inside_coverage() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.click("Coverage 0% of required lines");
    fixture.click("[Show file diff]");
    let visible = fixture.text();
    assert!(visible.contains("File diff ·"), "{visible}");
    assert!(visible.contains("[Close diff]"), "{visible}");
}

#[test]
fn coverage_header_reports_credited_units_even_below_one_percent() {
    let mut policy = b"pub fn policy() -> bool { true }\n".to_vec();
    for _ in 0..400 {
        policy.extend_from_slice(b"// changed line\n");
    }
    let (mut fixture, kickoff) = ExploreUi::with_policy(&policy);
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut pass = review_explore::ExplorePass::new(exploration);
    pass.post(&kickoff).unwrap();
    pass.submit(&fixture.response(&kickoff, 1), false).unwrap();
    let question = pass.exploration.questions[0].clone();
    let answer = pass
        .exploration
        .clone()
        .request(
            Some(review_explore::AnswerInput {
                option: Some("inspect".into()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    pass.post(&answer).unwrap();
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(pass))),
        view: None,
        passes: vec![],
        historical: false,
        storage_error: None,
    });
    fixture.app.publish(ui_events::ExploreCoverageRefresh);
    let coverage = fixture.text();
    assert!(
        coverage.contains("Coverage 0.4% of required lines"),
        "{coverage}"
    );
}
use std::fmt::Write as _;
use ui_events::EvidenceView;

impl ExploreUi {
    fn saved_evidence_draft(&self, path: &str) -> review_threads::Draft {
        let source = self
            .comparison
            .working_source_at(std::path::Path::new(path))
            .unwrap();
        let mut draft = review_threads::Draft::start(
            path.into(),
            Arc::new(review_threads::ThreadSource {
                anchor: review_guide::DiffRangeAnchor {
                    source_checkpoint: self.comparison.checkpoint.checkpoint.clone(),
                    old_path: None,
                    new_path: Some(path.into()),
                    old_lines: None,
                    new_lines: Some(0..1),
                    target_kind: review_guide::GuideAnchorKind::Lines,
                    source_hunk_count: 1,
                    old_content: None,
                    new_content: Some(source.read(self.files.root()).unwrap()),
                    diff_hash: String::new(),
                },
                excerpt: source.read_text(self.files.root()).unwrap(),
            }),
        );
        draft.text = format!("Saved comment for {path}");
        draft
    }

    fn assert_no_comment_writes(actions: &[Action]) {
        assert!(!actions.iter().any(|action| matches!(
            action,
            Action::Thread(
                review_threads::ThreadCommand::SaveDraft { .. }
                    | review_threads::ThreadCommand::DiscardDraft { .. }
                    | review_threads::ThreadCommand::Post { .. }
            )
        )));
    }

    fn posted_comment(actions: Vec<Action>) -> review_threads::Post {
        actions
            .into_iter()
            .find_map(|action| match action {
                Action::Thread(review_threads::ThreadCommand::Post { post, .. }) => Some(post),
                _ => None,
            })
            .expect("ordinary comment post")
    }

    fn switch_comment_reference(&mut self, command: char) {
        for key in [Key::Tab, Key::Tab, Key::Char(command), Key::Tab] {
            self.app.update(UserInput::Key(key));
        }
    }

    fn shared_comment_draft(&mut self, cached_draft: bool) -> review_threads::ReviewThreads {
        let mut book =
            review_threads::ReviewThreads::new(self.comparison.checkpoint.review_unit.clone());
        if cached_draft {
            book.save_draft(self.saved_evidence_draft("caller.rs"))
                .unwrap();
        }
        self.app.publish(ui_events::ReviewThreadsLoaded {
            review_unit: book.review_unit.clone(),
            result: Ok(book.clone()),
        });
        self.app.update(UserInput::Key(Key::Tab));
        self.app.update(UserInput::Key(Key::Char('a')));
        for action in self
            .app
            .update(UserInput::Paste("Shared source draft".into()))
        {
            if let Action::Thread(review_threads::ThreadCommand::SaveDraft { draft, .. }) = action {
                book.save_draft(draft).unwrap();
            }
        }
        self.switch_comment_reference('e');
        self.app.publish(ui_events::ReviewThreadsLoaded {
            review_unit: book.review_unit.clone(),
            result: Ok(book.clone()),
        });
        book
    }

    pub(super) fn inline_sizes(&self) -> Vec<(EvidenceView, ui_events::DiffViewportChanged)> {
        let explore = self
            .app
            .event_bus
            .get::<ExploreComponent>(self.app.explore_component)
            .unwrap();
        let diff = self
            .app
            .event_bus
            .get::<DiffComponent>(self.app.diff_component)
            .unwrap();
        explore
            .conversation_layout(
                Rect::new(0, 2, self.app.width, self.app.height.saturating_sub(3)),
                diff,
                self.app.palette,
            )
            .viewports()
            .0
    }

    fn inline_height(&self, turn: usize) -> u16 {
        self.inline_sizes()
            .iter()
            .find(|(id, _)| matches!(id, EvidenceView::Question { turn: index, .. } if *index == turn))
            .unwrap()
            .1
            .height
            + 2
    }

    fn point(&self, needle: &str) -> (u16, u16) {
        let buffer = self.buffer();
        buffer
            .content
            .chunks(usize::from(self.app.width))
            .enumerate()
            .find_map(|(row, cells)| {
                let line: String = cells.iter().map(ratatui::buffer::Cell::symbol).collect();
                line.find(needle).map(|byte| {
                    (
                        u16::try_from(line[..byte].chars().count()).unwrap(),
                        u16::try_from(row).unwrap(),
                    )
                })
            })
            .unwrap_or_else(|| panic!("missing {needle}: {}", self.text()))
    }

    pub(super) fn click(&mut self, needle: &str) {
        self.click_actions(needle);
    }

    pub(super) fn click_actions(&mut self, needle: &str) -> Vec<Action> {
        let (column, row) = self.point(needle);
        self.app.update(UserInput::MouseClick { column, row })
    }

    fn viewer_path(&self, turn: usize, reference: usize) -> &str {
        self.app
            .event_bus
            .get::<DiffComponent>(self.app.diff_component)
            .unwrap()
            .evidence_view(EvidenceView::Question { turn, reference })
            .unwrap()
            .evidence_path()
            .unwrap()
    }
}

#[test]
fn accepted_mcp_questions_advance_after_input_and_preserve_history_drafts() {
    let (mut fixture, mut request) = ExploreUi::new();
    let (response, received) = std::sync::mpsc::channel();
    for version in 1..=3 {
        let update = fixture.response(&request, version);
        fixture.app.publish(ui_events::ExploreSubmission {
            update,
            response: response.clone(),
        });
        assert!(received.recv().unwrap().unwrap());
        let text = fixture.text();
        assert!(text.contains(&format!("Question {version}:")), "{text}");
        assert!(!text.contains("Next question ready"), "{text}");

        fixture.click("2. Inspect the caller");
        let (column, row) = fixture.point("[Send]");
        request = ExploreUi::request(fixture.app.update(UserInput::MouseClick { column, row }));
        fixture.app.update(UserInput::MouseRelease);
        // Incidental activity used to suppress the next accepted question.
        fixture.app.update(UserInput::Key(Key::Escape));
        fixture.app.update(UserInput::Key(Key::PageUp));
    }

    // A fresh draft stays attached to its question when the next turn arrives.
    fixture
        .app
        .update(UserInput::Paste("Keep my new thought".into()));
    let update = fixture.response(&request, 4);
    fixture
        .app
        .publish(ui_events::ExploreSubmission { update, response });
    assert!(received.recv().unwrap().unwrap());
    let text = fixture.text();
    assert!(text.contains("Question 4:"), "{text}");
    assert!(!text.contains("Question 3:"), "{text}");
    assert!(!text.contains("Keep my new thought"), "{text}");
    assert!(!text.contains("Next question ready"), "{text}");
    fixture.click("[Previous]");
    let text = fixture.text();
    assert!(text.contains("Question 3:"), "{text}");
    assert!(text.contains("Keep my new thought"), "{text}");
    assert!(!text.contains("Question 4:"), "{text}");
    fixture.click("[Latest]");
    assert!(fixture.text().contains("Question 4:"));
}

#[test]
fn mcp_questions_received_in_other_panes_are_ready_on_return_and_retries_do_not_navigate() {
    for mode in [ReviewNavigation::Files, ReviewNavigation::Threads] {
        let (mut fixture, request) = ExploreUi::new();
        fixture.respond(&request, 1);
        fixture.app.update(UserInput::Key(Key::Char('2')));
        let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
        fixture.app.publish(ReviewNavigationChanged(mode));
        let update = fixture.response(&request, 2);
        let (response, received) = std::sync::mpsc::channel();
        fixture.app.publish(ui_events::ExploreSubmission {
            update: update.clone(),
            response: response.clone(),
        });
        assert!(received.recv().unwrap().unwrap());
        assert_eq!(fixture.app.navigation, mode);
        fixture
            .app
            .publish(ReviewNavigationChanged(ReviewNavigation::Explore));
        assert!(fixture.text().contains("Question 2:"));
        assert!(!fixture.text().contains("Question 1:"));
        fixture.click("[Previous]");
        fixture
            .app
            .publish(ui_events::ExploreSubmission { update, response });
        assert!(!received.recv().unwrap().unwrap());
        assert!(fixture.text().contains("Question 1:"));
        assert!(!fixture.text().contains("Question 2:"));
    }
}

#[test]
fn evidence_opened_after_threads_load_restores_saved_drafts_without_reloading() {
    for side in [
        review_explore::SourceSide::New,
        review_explore::SourceSide::Old,
    ] {
        assert_evidence_restores_saved_drafts(side);
    }
}

fn assert_evidence_restores_saved_drafts(side: review_explore::SourceSide) {
    let (mut fixture, request) = ExploreUi::new();
    let drafts = ["policy.rs", "caller.rs"].map(|path| fixture.saved_evidence_draft(path));
    let mut book =
        review_threads::ReviewThreads::new(fixture.comparison.checkpoint.review_unit.clone());
    for draft in &drafts {
        book.save_draft(draft.clone()).unwrap();
    }
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book),
    });
    let mut response = fixture.response(&request, 1);
    let caller = fixture
        .comparison
        .working_source_at(std::path::Path::new("caller.rs"))
        .unwrap();
    response.next.as_mut().unwrap().evidence[1] = EvidenceRef {
        location: review_explore::CodeLocation {
            path: caller.path.clone(),
            side,
            lines: Some(GuideLineRange {
                first_line: 1,
                last_line: 1,
            }),
        },
        relationship: "Unchanged caller".into(),
        decision_relevance: "This policy determines whether the proposed recovery is sufficient."
            .into(),
    };
    let actions = fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    ExploreUi::assert_no_comment_writes(&actions);
    for (reference, draft) in drafts.iter().enumerate() {
        assert_eq!(fixture.viewer_path(0, reference), draft.path());
        assert!(fixture.text().contains(&draft.text), "{}", fixture.text());
        if reference == 1 {
            assert!(fixture.text().contains("pub fn caller()"));
        }
        fixture.app.update(UserInput::Key(Key::Tab));
        let post = ExploreUi::posted_comment(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        assert_eq!(
            post,
            draft.post(),
            "restoration preserves publication and source identities"
        );
        for key in [Key::Tab, Key::Tab, Key::Char('e')] {
            ExploreUi::assert_no_comment_writes(&fixture.app.update(UserInput::Key(key)));
        }
    }
}

#[test]
fn opening_historical_caller_maps_inherited_comments_without_reloading() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 90,
    });
    let mut draft = fixture.saved_evidence_draft("caller.rs");
    let anchor = &mut Arc::make_mut(&mut draft.source).anchor;
    anchor.old_path = anchor.new_path.take();
    anchor.old_lines = anchor.new_lines.take();
    anchor.old_content = anchor.new_content.take();
    let mut book =
        review_threads::ReviewThreads::new(fixture.comparison.checkpoint.review_unit.clone());
    book.post(draft.post()).unwrap();
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book),
    });
    let mut response = fixture.response(&request, 1);
    let question = response.next.as_mut().unwrap();
    question.evidence.truncate(1);
    question.evidence[0].location.path =
        review_repository::repository::RepoPath::from_bytes(b"caller.rs");
    question.evidence[0].location.side = review_explore::SourceSide::Old;
    let actions = fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    ExploreUi::assert_no_comment_writes(&actions);
    let text = fixture.text();
    assert!(text.contains("caller.rs · Base"), "{text}");
    assert!(text.contains(&draft.text), "{text}");
    assert!(!text.contains("original context"), "{text}");
}

#[test]
fn cancelled_drafts_do_not_reappear_in_fresh_evidence_views_from_stale_books() {
    let (mut fixture, request) = ExploreUi::new();
    let draft = fixture.saved_evidence_draft("policy.rs");
    let mut book =
        review_threads::ReviewThreads::new(fixture.comparison.checkpoint.review_unit.clone());
    book.save_draft(draft.clone()).unwrap();
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book.clone()),
    });
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Key(Key::Tab));
    assert!(fixture.text().contains(&draft.text));
    fixture.click("Cancel");
    assert!(!fixture.text().contains(&draft.text));
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book),
    });
    assert!(!fixture.text().contains(&draft.text));
    fixture.switch_comment_reference('e');
    assert!(!fixture.text().contains(&draft.text));
    fixture.app.update(UserInput::Key(Key::Char('a')));
    fixture
        .app
        .update(UserInput::Paste("Fresh source comment".into()));
    let post = ExploreUi::posted_comment(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_ne!(&post.message().id, draft.message_id());
    assert_eq!(post.message().text, "Fresh source comment");
}

#[test]
fn preparation_and_delivery_failure_stay_in_the_conversation() {
    let (mut fixture, request) = ExploreUi::new();
    let text = fixture.text();
    assert!(text.contains("Waiting for the implementation agent"));
    for absent in ["Diff ·", "Your answer", "[More]", "[Send]"] {
        assert!(!text.contains(absent));
    }
    fixture.respond(&request, 1);
    fixture
        .app
        .update(UserInput::Paste("Keep this exact answer.".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let text = fixture.text();
    assert!(text.contains("You: Keep resolved"));
    assert!(text.contains("Keep this exact answer."));
    assert!(text.find("You:") < text.find("Waiting for the implementation agent"));
    assert!(!text.contains("Your answer"));
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Err("Delivery unavailable".into()),
    });
    assert!(fixture.text().contains("Delivery unavailable"));
    assert!(fixture.text().contains("[Retry]"));
    fixture.app.update(UserInput::Key(Key::Enter));
    assert!(fixture.text().contains("Keep this exact answer."));
}

#[test]
fn evidence_fits_wrapping_and_resizes_without_using_files_sidebar_width() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.file_width = Some(39);
    fixture.respond(&request, 1);
    let fitted = fixture.inline_height(0);
    assert!(fitted <= (fixture.app.height - 5) / 2);
    assert_eq!(fixture.inline_sizes()[0].1.width, 136);
    fixture.app.update(UserInput::Key(Key::Alt('j')));
    assert_eq!(fixture.inline_height(0), fitted + 2);
    let (column, row) = fixture.point("drag to resize");
    fixture.app.update(UserInput::MouseClick { column, row });
    fixture.app.update(UserInput::MouseDrag {
        column,
        row: row + 1,
    });
    fixture.app.update(UserInput::MouseRelease);
    assert_eq!(fixture.inline_height(0), fitted + 3);
    fixture.app.update(UserInput::Resize {
        width: 55,
        height: 45,
    });
    assert_eq!(
        fixture.inline_height(0),
        fitted + 3,
        "manual height survives reflow"
    );
    fixture.app.update(UserInput::Key(Key::Alt('0')));
    assert!(
        fixture.inline_height(0) > fitted,
        "fit measures wrapped visual rows"
    );
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 45,
    });
    assert_eq!(fixture.inline_height(0), fitted);
    assert_eq!(fixture.app.file_width, Some(39));
}

#[test]
fn history_pages_retain_independent_evidence_and_drafts() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 80,
    });
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste(
        "Human context for the first question.".into(),
    ));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    let mut response = fixture.response(&request, 2);
    let source = fixture
        .comparison
        .sources
        .iter()
        .find(|source| source.display_path == "tests.rs")
        .unwrap();
    response.next.as_mut().unwrap().evidence = vec![EvidenceRef {
        location: review_explore::CodeLocation {
            path: source.path.clone(),
            side: source.side,
            lines: Some(GuideLineRange {
                first_line: 1,
                last_line: 1,
            }),
        },
        relationship: "Test behavior".into(),
        decision_relevance: "This policy determines whether the proposed recovery is sufficient."
            .into(),
    }];
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    let text = fixture.text();
    assert!(!text.contains("Question 1: keep resolved?"));
    assert!(text.contains("Question 2: keep resolved?"));
    assert!(!text.contains("Human context for the first question."));
    assert_eq!(fixture.inline_sizes().len(), 1);
    assert_eq!(text.matches("Your answer").count(), 1);
    fixture
        .app
        .update(UserInput::Paste("Current answer draft.".into()));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    fixture.app.publish(ui_events::SourceLocationAccepted {
        location: SourceLocation {
            path: fixture.files.root().join("caller.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
    });
    assert_eq!(fixture.viewer_path(0, 0), "caller.rs");
    assert_eq!(fixture.viewer_path(1, 0), "tests.rs");
    assert!(!fixture.text().contains("Current answer draft."));
    assert!(fixture.text().contains("You: Keep resolved"));
    assert!(
        fixture
            .text()
            .contains("Human context for the first question.")
    );
    fixture.click("[Next]");
    assert!(!fixture.text().contains("fn caller()"));
    assert_eq!(fixture.viewer_path(1, 0), "tests.rs");
    assert!(fixture.text().contains("Current answer draft."));
    fixture.click("[Previous]");
    assert!(fixture.text().contains("fn caller()"));
    assert!(!fixture.text().contains("Current answer draft."));
}

#[test]
fn response_while_reading_history_opens_the_new_question() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("First context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture
        .app
        .update(UserInput::Paste("Second context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(fixture.text().contains("Question 1: keep resolved?"));
    fixture.respond(&request, 3);
    assert!(!fixture.text().contains("Question 1: keep resolved?"));
    assert!(fixture.text().contains("Question 3: keep resolved?"));
    assert_eq!(fixture.inline_sizes().len(), 1);
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(fixture.text().contains("Question 2: keep resolved?"));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(fixture.text().contains("Question 1: keep resolved?"));
    fixture.click("[Latest]");
    assert!(fixture.text().contains("Question 3: keep resolved?"));
}

#[test]
fn native_search_selection_and_comment_editor_survive_window_resizing() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let unit = fixture.comparison.checkpoint.review_unit.clone();
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: unit.clone(),
        result: Ok(review_threads::ReviewThreads::new(unit)),
    });
    fixture
        .app
        .update(UserInput::Paste("Keep my interview draft".into()));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('/')));
    for character in "resolved".chars() {
        fixture.app.update(UserInput::Key(Key::Char(character)));
    }
    fixture.app.update(UserInput::Key(Key::Enter));
    fixture.app.update(UserInput::Key(Key::Visual));
    fixture.app.update(UserInput::Key(Key::Char('a')));
    fixture
        .app
        .update(UserInput::Paste("Native code comment".into()));
    fixture.app.update(UserInput::Key(Key::Alt('j')));
    fixture.app.update(UserInput::Key(Key::Alt('k')));
    assert!(fixture.text().contains("Native code comment"));
    assert!(fixture.text().contains("Keep my interview draft"));
    let actions = fixture.app.update(UserInput::Key(Key::ControlEnter));
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Thread(review_threads::ThreadCommand::Post { .. })
    )));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::SetReviewed { .. }))
    );
}

#[test]
fn large_evidence_is_bounded_and_wheels_scroll_exactly_one_layer() {
    let mut policy = String::new();
    for index in 0..250 {
        writeln!(
            policy,
            "// line_{index:03} supporting context for a large policy change"
        )
        .unwrap();
    }
    let (mut fixture, request) = ExploreUi::with_policy(policy.as_bytes());
    fixture.app.update(UserInput::Resize {
        width: 100,
        height: 40,
    });
    let mut response = fixture.response(&request, 1);
    let question = response.next.as_mut().unwrap();
    question.text = "A long question about preserving behavior across separate callers, delayed delivery and several related policy boundaries. Which behavior should remain authoritative when these circumstances interact?".into();
    question.evidence[0]
        .location
        .lines
        .as_mut()
        .unwrap()
        .last_line = 120;
    question.evidence[1].location.lines = Some(GuideLineRange {
        first_line: 230,
        last_line: 230,
    });
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    assert!(fixture.inline_height(0) <= (fixture.app.height - 5) / 2);
    assert!(
        !fixture.text().contains("line_229"),
        "distant evidence must not enlarge the first window"
    );
    let question_position = fixture.point("A long question");
    let (column, row) = fixture.point("Diff ·");
    fixture.app.update(UserInput::MouseScroll {
        column: column + 5,
        row: row + 2,
        delta: 5,
    });
    assert_eq!(
        fixture.point("A long question"),
        question_position,
        "the diff wheel must not move the conversation"
    );
    let before = fixture.point("line_005");
    fixture.app.update(UserInput::MouseScroll {
        column: 0,
        row: 4,
        delta: 2,
    });
    let after = fixture.point("line_005");
    assert_eq!(
        before.1.saturating_sub(after.1),
        2,
        "conversation scroll must leave the code position intact"
    );
    assert_eq!(
        fixture.app.focus,
        ReviewPane::Navigation,
        "keyboard focus cannot remain on an offscreen viewer"
    );
}

#[test]
fn non_text_evidence_is_compact_and_never_opens_a_blank_diff() {
    let (mut fixture, request) = ExploreUi::with_policy(b"\0\xff");
    let mut response = fixture.response(&request, 1);
    response.next.as_mut().unwrap().evidence.truncate(1);
    response.next.as_mut().unwrap().evidence[0].location.lines = None;
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    let text = fixture.text();
    assert!(text.contains("non-text or unavailable source"));
    assert!(!text.contains("Diff ·"));
    assert!(text.contains("Your answer"));
    assert!(fixture.inline_sizes().is_empty());
}

#[test]
fn delayed_search_results_stay_with_their_evidence_window() {
    let mut policy = String::new();
    for index in 0..1600 {
        writeln!(
            policy,
            "// line_{index:04} supporting context for a substantial policy change"
        )
        .unwrap();
    }
    let (mut fixture, request) = ExploreUi::with_policy(policy.as_bytes());
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('/')));
    let actions = fixture.app.update(UserInput::Key(Key::Char('s')));
    let search = actions
        .into_iter()
        .find_map(|action| match action {
            Action::Search(Some(request)) => Some(request),
            _ => None,
        })
        .expect("large source uses the shared search worker");
    let evidence = fixture.response(&request, 1).next.unwrap().evidence;
    let open = |app: &mut ReviewApplication, reference| {
        app.publish(ui_events::ExploreEvidence {
            comparison: fixture.comparison.clone(),
            evidence: evidence.clone(),
            primary: evidence.len(),
            view: EvidenceView::Question { turn: 0, reference },
            reveal: false,
            required_only: false,
        })
    };
    open(&mut fixture.app, 1);
    let actions = open(&mut fixture.app, 0);
    assert!(
        actions.iter().any(
            |action| matches!(action, Action::Search(Some(request)) if request.id == search.id)
        ),
        "reopening resumes work cancelled by another viewer"
    );
    open(&mut fixture.app, 1);
    fixture.app.publish(search.search());
    assert_eq!(fixture.viewer_path(0, 1), "policy.rs");
    let actions = open(&mut fixture.app, 0);
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Search(_))),
        "the inactive viewer retained its completed search"
    );
    assert!(fixture.text().contains("/s"));
}

#[test]
fn cancelling_the_first_capture_rejects_its_late_completion() {
    let (mut fixture, _) = ExploreUi::new();
    fixture.app = ReviewApplication::new(Theme::default(), None, fixture.files.root().into());
    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Explore));
    fixture.app.update(UserInput::Key(Key::Char('s')));
    fixture.app.update(UserInput::Key(Key::Char('c')));
    let retry = fixture.app.update(UserInput::Key(Key::Char('r')));
    assert!(
        !retry
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Start))),
        "a retry must wait until the cancelled capture drains"
    );
    let actions = fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Turn(_))))
    );
    fixture.app.update(UserInput::Key(Key::Char('r')));
    ExploreUi::request(fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    }));
}

#[test]
fn cancelling_a_new_capture_keeps_the_previous_evidence_viewer() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste(
        "Retain this draft during cancellation".into(),
    ));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('n')));
    fixture.app.update(UserInput::Key(Key::Char('n')));
    fixture.app.update(UserInput::Key(Key::Char('c')));
    assert!(!fixture.text().contains("[Send]"));
    for key in [
        Key::ControlEnter,
        Key::Char('1'),
        Key::Char('d'),
        Key::Char('r'),
        Key::Char('n'),
    ] {
        let actions = fixture.app.update(UserInput::Key(key));
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::Explore(_))),
            "submission/start cannot bypass capture cancellation: {key:?}"
        );
    }
    fixture.app.publish(ExploreCaptured {
        result: Ok(fixture.comparison.clone()),
    });
    assert_eq!(fixture.inline_sizes().len(), 1);
    assert!(fixture.text().contains("Question 1: keep resolved?"));
    assert_eq!(fixture.viewer_path(0, 0), "policy.rs");
    assert!(
        fixture
            .text()
            .contains("Retain this draft during cancellation")
    );
}

#[test]
fn editing_and_correcting_always_reveal_the_composer_turn() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Paste("First context".into()));
    assert!(fixture.text().contains("Your answer"));
    assert!(fixture.text().contains("First context"));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture.app.update(UserInput::Key(Key::Char('[')));
    fixture.app.update(UserInput::Key(Key::Char('x')));
    assert!(fixture.text().contains("Your answer"));
    assert!(fixture.text().contains("You: Keep resolved"));
    assert!(fixture.text().contains("First context"));
    assert!(fixture.text().contains("Correction appends"));
}

#[test]
fn a_post_finishing_in_files_clears_the_parked_evidence_editor() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let unit = fixture.comparison.checkpoint.review_unit.clone();
    let mut book = review_threads::ReviewThreads::new(unit.clone());
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: unit.clone(),
        result: Ok(book.clone()),
    });
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('a')));
    fixture
        .app
        .update(UserInput::Paste("First ordinary comment".into()));
    let actions = fixture.app.update(UserInput::Key(Key::ControlEnter));
    let post = actions
        .into_iter()
        .find_map(|action| match action {
            Action::Thread(review_threads::ThreadCommand::Post { post, .. }) => Some(post),
            _ => None,
        })
        .expect("comment post");
    fixture.app.update(UserInput::Key(Key::Control('t')));
    book.post(post.clone()).unwrap();
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: unit.clone(),
        result: Ok(book),
    });
    fixture.app.publish(ui_events::ThreadPostFinished {
        review_unit: unit,
        message_id: post.message().id.clone(),
        result: Ok(()),
    });
    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Explore));
    fixture.app.update(UserInput::Key(Key::Char('a')));
    fixture
        .app
        .update(UserInput::Paste("Second ordinary comment".into()));
    let actions = fixture.app.update(UserInput::Key(Key::ControlEnter));
    assert!(actions.iter().any(|action| matches!(action, Action::Thread(review_threads::ThreadCommand::Post { post: next, .. }) if next.message().id != post.message().id)));
}

#[test]
fn shared_comment_drafts_clear_or_keep_divergent_text_after_another_view_posts() {
    for (modified, cached_draft) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut fixture, request) = ExploreUi::new();
        fixture.respond(&request, 1);
        let unit = fixture.comparison.checkpoint.review_unit.clone();
        let mut book = fixture.shared_comment_draft(cached_draft);
        if modified {
            fixture
                .app
                .update(UserInput::Paste(" independently changed".into()));
        }
        fixture.switch_comment_reference('b');
        let original =
            ExploreUi::posted_comment(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        book.post(original.clone()).unwrap();
        let actions = fixture.app.publish(ui_events::ReviewThreadsLoaded {
            review_unit: unit.clone(),
            result: Ok(book.clone()),
        });
        for action in actions {
            if let Action::Thread(review_threads::ThreadCommand::SaveDraft { draft, .. }) = action {
                assert!(modified);
                assert_ne!(draft.message_id(), &original.message().id);
                assert!(draft.text.contains("independently changed"));
                book.save_draft(draft).unwrap();
            }
        }
        if modified {
            assert!(
                book.drafts()
                    .iter()
                    .any(|draft| draft.text.contains("independently changed")
                        && draft.message_id() != &original.message().id)
            );
        }
        fixture.app.publish(ui_events::ThreadPostFinished {
            review_unit: unit,
            message_id: original.message().id.clone(),
            result: Ok(()),
        });
        fixture.switch_comment_reference('e');
        if !modified {
            let actions = fixture.app.update(UserInput::Key(Key::ControlEnter));
            assert!(!actions.iter().any(|action| matches!(
                action,
                Action::Thread(review_threads::ThreadCommand::Post { .. })
            )));
            fixture.app.update(UserInput::Key(Key::Char('a')));
            fixture
                .app
                .update(UserInput::Paste("Fresh source comment".into()));
        }
        let next = ExploreUi::posted_comment(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        assert_ne!(next.message().id, original.message().id);
        if modified {
            assert!(next.message().text.contains("independently changed"));
        }
        book.post(next)
            .expect("the remaining editor must be publishable with its own identity");
    }
}

#[test]
fn replying_from_history_answers_the_displayed_question() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 100,
    });
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("First context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture.app.update(UserInput::Key(Key::Char('[')));
    fixture.click("[Reply]");
    fixture.click("Your answer");
    fixture
        .app
        .update(UserInput::Paste("About the first question".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request.answer.unwrap().question.as_ref().unwrap().version,
        1
    );
}

#[test]
fn a_conclusion_is_separate_from_the_viewed_question_and_history_restores_it() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 70,
    });
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("First context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture.app.update(UserInput::Key(Key::Char('[')));
    fixture
        .app
        .update(UserInput::Paste("Explain the first question".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.app.update(UserInput::Key(Key::Char(']')));
    let before = fixture.point("Question 2: keep resolved?");
    let mut response = fixture.response(&request, 3);
    response.next = None;
    response.conclusion = Some(conclusion("Further human file inspection remains required"));
    response.interpretation = None;
    response.reply = Some(review_explore::Reply {
        text: "Additional grounded explanation\n".repeat(8),
        evidence: vec![],
    });
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    assert!(!fixture.text().contains("Question 2: keep resolved?"));
    fixture.click("[Previous]");
    assert_eq!(fixture.point("Question 2: keep resolved?"), before);
}

#[test]
fn history_controls_stay_visible_while_scrolling_a_question() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("First context".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    fixture.app.update(UserInput::Resize {
        width: 40,
        height: 20,
    });
    let previous = fixture.point("[Previous]");
    for _ in 0..8 {
        fixture.app.update(UserInput::Key(Key::PageDown));
    }
    assert_eq!(fixture.point("[Previous]"), previous);
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Question 1/2"));
    fixture.click("[Next]");
    assert!(fixture.text().contains("Question 2/2"));
    fixture.click("[Opening]");
    assert!(!fixture.text().contains("Question 2/2"));
    assert!(!fixture.text().contains("[Previous]"));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(!fixture.text().contains("Question 1/2"));
    assert!(!fixture.text().contains("Question 2/2"));
    fixture.app.update(UserInput::Key(Key::Char(']')));
    assert!(fixture.text().contains("Question 1/2"));
}
