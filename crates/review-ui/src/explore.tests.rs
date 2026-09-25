use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use review_explore::{
    Alternative, Command, Comparison, EvidenceRef, Interpretation, InterviewUpdate, Question,
    Topic, TopicStatus, TurnRequest,
};
use review_repository::repository::{RepoType, Repository};
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use std::sync::Arc;
use ui_events::{
    ExploreCaptured, ExploreCoverageRefresh, ExploreFinished, ExploreRestored, ReviewNavigation,
    ReviewNavigationChanged,
};

#[path = "explore_choices.tests.rs"]
mod choices;
#[path = "explore_conclusion.tests.rs"]
mod conclusion_tests;
#[path = "explore_editor.tests.rs"]
mod editor;
#[path = "explore_flow.tests.rs"]
mod flow;
#[path = "explore_recovery.tests.rs"]
mod recovery;

struct ExploreUi {
    app: ReviewApplication,
    comparison: Arc<Comparison>,
    files: Box<dyn ReviewRepositoryFixture>,
    _state: tempfile::TempDir,
}

impl ExploreUi {
    fn new() -> (Self, TurnRequest) {
        Self::with_policy(b"pub fn policy() -> bool { true } // resolved() continuation_alpha continuation_beta continuation_gamma continuation_delta continuation_epsilon continuation_zeta\n// untouched middle\n// other evidence\n")
    }

    fn with_policy(policy: &[u8]) -> (Self, TurnRequest) {
        Self::with_versions(b"pub fn policy() -> bool { false }\n", policy)
    }

    fn with_versions(base: &[u8], policy: &[u8]) -> (Self, TurnRequest) {
        let files = repository_fixture(RepoType::Git);
        files.write("Cargo.toml", b"[package]\nname = \"explore_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\npath = \"lib.rs\"\n");
        files.write("lib.rs", b"pub mod policy;\npub mod caller;\n");
        files.write(".gitignore", b"target/\nCargo.lock\n");
        files.write(
            "caller.rs",
            b"pub fn caller() -> bool { crate::policy::policy() }\n",
        );
        files.write("policy.rs", base);
        files.write("support/policy.rs", b"pub fn support_policy() {}\n");
        files.new_change("base");
        files.write("policy.rs", policy);
        files.write("tests.rs", b"fn test_policy() { policy(); }\n");
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let snapshot = complete_repository_snapshot(&repository);
        let comparison = Arc::new(Comparison::prepare(&repository, &snapshot).unwrap());
        let mut app = ReviewApplication::new(Theme::default(), None, files.root().into());
        app.update(UserInput::Resize {
            width: 140,
            height: 45,
        });
        publish_repository(
            &mut app,
            comparison.checkpoint.clone(),
            "Explore fixture".into(),
            snapshot
                .files
                .iter()
                .map(|file| FileSummary::new(file.review_path().display(), ReviewStatus::Reviewed))
                .collect(),
        );
        app.publish(ReviewNavigationChanged(ReviewNavigation::Explore));
        app.update(UserInput::Key(Key::Char('s')));
        let actions = app.publish(ExploreCaptured {
            result: Ok(comparison.clone()),
        });
        app.publish(ExploreCoverageRefresh);
        let request = Self::request(actions);
        (
            Self {
                app,
                comparison,
                files,
                _state: state,
            },
            request,
        )
    }

    fn request(actions: Vec<Action>) -> TurnRequest {
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::SetReviewed { .. }))
        );
        actions
            .into_iter()
            .find_map(|action| match action {
                Action::Explore(Command::Turn(request) | Command::Retry(request)) => Some(*request),
                _ => None,
            })
            .expect("interview request")
    }

    fn response(&self, request: &TurnRequest, version: u32) -> InterviewUpdate {
        let source = self
            .comparison
            .sources
            .iter()
            .find(|source| {
                source.display_path == "policy.rs" && source.side == review_explore::SourceSide::New
            })
            .unwrap();
        InterviewUpdate {
            reply: Some(review_explore::Reply {
                text: "The source supports this context.".into(),
                evidence: vec![],
            }),
            agenda: vec![],
            instance: request.instance.clone(),
            request: request.request.clone(),
            checkpoint: request.checkpoint.clone(),
            interpretation: request.answer.as_ref().map(|answer| Interpretation {
                answer: answer.id.clone(),
                status: TopicStatus::Open,
                recap: "Recorded domain context; clarify the consequence.".into(),
                follow_ups: vec![],
            }),
            topics: vec![Topic {
                prompt: String::new(),
                prerequisites: vec![],
                rank: 0,
                id: "policy".into(),
                title: "Resolution".into(),
                entries: vec![review_explore::CodeLocation {
                    path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
                    side: review_explore::SourceSide::New,
                    lines: None,
                }],
                status: TopicStatus::Open,
            }],
            next: Some(Question {
                id: "q".into(),
                version,
                topic: "policy".into(),
                text: format!("Question {version}: keep resolved?"),
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
                        id: "inspect".into(),
                        text: "Inspect the caller".into(),
                        outcome: TopicStatus::Open,
                        recommendation: None,
                    },
                ],
                evidence: [1, 3]
                    .into_iter()
                    .map(|line| EvidenceRef {
                        location: review_explore::CodeLocation {
                            path: source.path.clone(),
                            side: source.side,
                            lines: Some(GuideLineRange {
                                first_line: line,
                                last_line: line,
                            }),
                        },
                        notes:
                            "This policy determines whether the proposed recovery is sufficient."
                                .into(),
                    })
                    .collect(),
            }),
            conclusion: None,
            limitations: vec![],
            findings: vec![],
        }
    }

    fn respond(&mut self, request: &TurnRequest, version: u32) {
        let response = self.response(request, version);
        let actions = self.app.publish(ExploreFinished {
            instance: request.instance.clone(),
            request: request.request.clone(),
            result: Ok(response),
        });
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::SetReviewed { .. }))
        );
    }

    fn buffer(&self) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, self.app.width, self.app.height));
        self.app.frame().render(buffer.area, &mut buffer);
        buffer
    }

    fn text(&self) -> String {
        self.buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }
}

#[test]
fn stopped_jev_bar_schedules_one_idle_expiry_redraw() {
    use std::time::{Duration, Instant};

    let (mut fixture, request) = ExploreUi::new();
    let mut pass = review_explore::ExplorePass::new(review_explore::Exploration::new(
        fixture.comparison.clone(),
    ));
    pass.exploration.instance = request.instance;
    pass.coverage
        .restart_classification("test", "attempt".into());
    pass.coverage.jev_total_windows = 2;
    pass.coverage.finish_classification(false, 100);
    fixture.app.publish(ExploreRestored {
        result: Ok(Some(Arc::new(pass))),
        view: None,
        historical: false,
        storage_error: None,
    });
    fixture.app.publish(ExploreCoverageRefresh);
    assert!(fixture.text().contains("Jev stopped"));

    let now = Instant::now();
    assert!(!fixture.app.needs_tick(now, now));
    assert!(fixture.app.needs_tick(now, now + Duration::from_secs(6)));
}

#[test]
fn outlines_keep_wrapping_disjoint_ranges_and_deleted_lines_separate() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    let old = fixture
        .comparison
        .sources
        .iter()
        .find(|source| {
            source.display_path == "policy.rs" && source.side == review_explore::SourceSide::Old
        })
        .unwrap();
    response.next.as_mut().unwrap().evidence.push(EvidenceRef {
        location: review_explore::CodeLocation {
            path: old.path.clone(),
            side: old.side,
            lines: Some(GuideLineRange {
                first_line: 1,
                last_line: 1,
            }),
        },
        notes: "Previous behavior determines whether the proposed recovery is sufficient.".into(),
    });
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    let buffer = fixture.buffer();
    let rows: Vec<_> = buffer.content.chunks(140).collect();
    let framed = |row: &&[ratatui::buffer::Cell]| {
        row.iter()
            .any(|cell| cell.symbol() == "│" && cell.fg == ratatui::style::Color::Yellow)
    };
    let text = |row: &&[ratatui::buffer::Cell]| {
        row.iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
    };
    for needle in [
        "false",
        "resolved()",
        "continuation_gamma",
        "continuation_zeta",
        "other evidence",
    ] {
        let row = rows
            .iter()
            .find(|row| text(row).contains(needle))
            .unwrap_or_else(|| panic!("missing {needle}: {}", fixture.text()));
        assert!(framed(row), "unframed {needle}");
    }
    let middle = rows
        .iter()
        .find(|row| text(row).contains("untouched middle"))
        .unwrap();
    assert!(
        !framed(middle),
        "disjoint outlines must not enclose intervening code"
    );
    let tops = rows
        .iter()
        .flat_map(|row| row.iter())
        .filter(|cell| cell.symbol() == "╭" && cell.fg == ratatui::style::Color::Yellow)
        .count();
    assert_eq!(tops, 3, "one frame per exact old/new range");

    fixture.app.update(UserInput::Key(Key::Char('e')));
    fixture.app.update(UserInput::Key(Key::Char('e')));
    fixture.app.update(UserInput::Key(Key::Tab));
    let actions = fixture.app.update(UserInput::Key(Key::Char('K')));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Lsp { .. })),
        "deleted coordinates cannot query the new document"
    );
}

#[test]
fn clipped_evidence_keeps_continuations_open_at_the_viewport_edges() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    let question = response.next.as_mut().unwrap();
    question.evidence.truncate(1);
    question.evidence[0].location.lines = Some(GuideLineRange {
        first_line: 1,
        last_line: 3,
    });
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 20,
    });
    fixture.app.update(UserInput::Key(Key::Tab));
    assert_eq!(fixture.app.focus, ui_events::ReviewPane::Detail);
    let render = |fixture: &ExploreUi| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 140, 20));
        fixture.app.frame().render(buffer.area, &mut buffer);
        buffer
    };
    let buffer = render(&fixture);
    let yellow_corner = |buffer: &Buffer, symbol: &str| {
        buffer
            .content
            .chunks(140)
            .flat_map(|row| row.iter())
            .filter(|cell| cell.fg == ratatui::style::Color::Yellow && cell.symbol() == symbol)
            .count()
    };
    assert_eq!(
        yellow_corner(&buffer, "╰"),
        0,
        "offscreen end must not gain a closing rule"
    );
    let code_row = buffer
        .content
        .chunks(140)
        .position(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .contains("pub fn policy")
        })
        .unwrap();
    fixture.app.update(UserInput::MouseScroll {
        column: 130,
        row: u16::try_from(code_row).unwrap(),
        delta: 4,
    });
    let buffer = render(&fixture);
    assert_eq!(
        yellow_corner(&buffer, "╭"),
        0,
        "offscreen start must not gain an opening rule"
    );
    assert!(
        yellow_corner(&buffer, "╰") > 0,
        "the true endpoint remains visible after scrolling"
    );
}

#[test]
fn ordinary_comments_keep_comparison_context_and_do_not_mark_files_reviewed() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let review_unit = fixture.comparison.checkpoint.review_unit.clone();
    fixture.app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: review_unit.clone(),
        result: Ok(review_threads::ReviewThreads::new(review_unit)),
    });
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('a')));
    fixture
        .app
        .update(UserInput::Paste("Ordinary source question".into()));
    assert!(fixture.text().contains("Ordinary source question"));
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
fn a_current_turn_with_wrong_payload_identity_is_visible_and_retryable() {
    let (mut fixture, request) = ExploreUi::new();
    let mut response = fixture.response(&request, 1);
    response.request = "wrong request".into();
    fixture.app.publish(ExploreFinished {
        instance: request.instance,
        request: request.request,
        result: Ok(response),
    });
    assert!(fixture.text().contains("Response identity"));
    let retry = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Char('r'))));
    assert!(retry.response_error.as_ref().unwrap().contains("identity"));
    fixture.respond(&retry, 1);
    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn new_pass_retains_history_and_failed_capture_preserves_text() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.update(UserInput::Paste("Keep my draft".into()));
    fixture.app.update(UserInput::Key(Key::Tab));
    let actions = fixture.app.update(UserInput::Key(Key::Char('n')));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Start)))
    );
    assert!(
        fixture
            .text()
            .contains("keeps this investigation in history")
    );
    let actions = fixture.app.update(UserInput::Key(Key::Char('n')));
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Start)))
    );
    fixture.app.publish(ExploreCaptured {
        result: Err("Capture failed".into()),
    });
    assert!(fixture.text().contains("Keep my draft"));
    assert!(fixture.text().contains("Question 1"));
}

#[test]
fn interview_auto_advances_and_restores_unposted_text_from_history() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 45,
    });
    fixture.respond(&request, 1);
    assert!(fixture.text().contains("Question 1"));
    assert!(fixture.text().contains("resolved()"));
    fixture
        .app
        .update(UserInput::Paste("Compliance archives these.".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.respond(&request, 2);
    assert!(fixture.text().contains("Question 2"));
    fixture
        .app
        .update(UserInput::Paste("Preserve history.".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture
        .app
        .update(UserInput::Paste("New unposted thought".into()));
    fixture.respond(&request, 3);
    assert!(fixture.text().contains("Question 3"));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(fixture.text().contains("Question 2"));
    assert!(fixture.text().contains("New unposted thought"));
    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Files));
    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Threads));
    fixture
        .app
        .publish(ReviewNavigationChanged(ReviewNavigation::Explore));
    assert!(fixture.text().contains("Question 2"));
    assert!(fixture.text().contains("New unposted thought"));
}

#[test]
fn supporting_paths_cannot_alias_changed_files_with_the_same_basename() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture.app.publish(ui_events::SourceLocationAccepted {
        location: SourceLocation {
            path: fixture.files.root().join("support/policy.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
    });
    assert!(fixture.text().contains("support_policy()"));
    assert!(!fixture.text().contains("resolved()"));
    fixture.app.update(UserInput::Key(Key::Char('b')));
    assert!(fixture.text().contains("resolved()"));
    assert!(!fixture.text().contains("support_policy()"));
}

#[test]
fn location_list_previews_read_live_sources_and_reject_external_destinations() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    fixture
        .files
        .write("caller.rs", b"pub fn live_caller() {}\n");
    let location = SourceLocation {
        path: fixture.files.root().join("caller.rs"),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 0,
    };
    let actions = fixture
        .app
        .publish(ui_events::SourceLocationPreviewRequested {
            location: location.clone(),
        });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::LoadSource { .. } | Action::LoadDiff { .. }))
    );
    assert!(fixture.text().contains("fn live_caller()"));
    let actions = fixture
        .app
        .publish(ui_events::SourceLocationPreviewRequested {
            location: SourceLocation {
                path: fixture.files.root().join("../external.rs"),
                ..location
            },
        });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::LoadSource { .. } | Action::LoadDiff { .. }))
    );
    assert!(fixture.text().contains("resolved()"));
}

#[test]
fn working_copy_navigation_preserves_outlines_and_allows_interview_answers() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let buffer = fixture.buffer();
    let yellow = buffer
        .content
        .iter()
        .filter(|cell| cell.fg == ratatui::style::Color::Yellow && cell.symbol() == "│")
        .count();
    assert!(yellow >= 4, "yellow range outlines missing");
    let actions = fixture.app.publish(ui_events::SourceLocationAccepted {
        location: SourceLocation {
            path: fixture.files.root().join("caller.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
    });
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::LoadSource { .. }))
    );
    assert!(fixture.text().contains("fn caller()"));
    fixture.app.update(UserInput::Key(Key::Tab));
    let actions = fixture.app.update(UserInput::Key(Key::Char('K')));
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Lsp { .. }))
    );
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture
        .app
        .update(UserInput::Paste("Keep this policy".into()));
    ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert!(fixture.text().contains("Keep this policy"));
}

#[test]
#[ignore = "requires a working rust-analyzer and Rust toolchain"]
fn real_rust_lsp_navigates_working_copy_sources_and_rejects_other_view_results() {
    use std::time::{Duration, Instant};
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let path = fixture.files.root().join("caller.rs");
    let text = std::fs::read_to_string(&path).unwrap();
    let column = text.rfind("policy()").unwrap();
    fixture.app.publish(ui_events::SourceLocationAccepted {
        location: SourceLocation {
            path: path.clone(),
            line: 0,
            byte_column: column,
            end_line: 0,
            end_byte_column: column + 6,
        },
    });
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('g')));
    let actions = fixture.app.update(UserInput::Key(Key::Char('d')));
    let query = actions
        .into_iter()
        .find_map(|action| {
            if let Action::Lsp { query, .. } = action {
                Some(query)
            } else {
                None
            }
        })
        .expect("normal new-side LSP request");
    assert!(query.snapshot_id.starts_with("explore:"));
    let worker = review_lsp::Worker::start(fixture.files.root().into());
    let events = worker.event_receiver();
    worker.open_document(path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let event = events.recv_timeout(Duration::from_secs(45)).unwrap();
        if matches!(event, LspEvent::Ready(_)) {
            break;
        }
        assert!(!matches!(event, LspEvent::Failed { .. }), "{event:?}");
    }
    let event = loop {
        worker
            .request(Operation::Definition, query.clone())
            .unwrap();
        let event = events.recv_timeout(Duration::from_secs(45)).unwrap();
        if let LspEvent::Locations { locations, .. } = &event
            && !locations.is_empty()
        {
            break event;
        }
        assert!(
            Instant::now() < deadline,
            "Rust indexing did not produce a definition: {event:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        matches!(&event, LspEvent::Locations { locations, .. } if locations.iter().any(|location| location.path.ends_with("policy.rs")))
    );
    fixture.app.publish(event.clone());
    assert!(fixture.text().contains("resolved()"));
    // Switching evidence windows invalidates responses for the earlier viewer.
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('e')));
    fixture.app.publish(ui_events::SourceLocationAccepted {
        location: SourceLocation {
            path: fixture.files.root().join("caller.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
    });
    fixture.app.publish(event);
    assert!(
        fixture.text().contains("fn caller()"),
        "delayed LSP response must not navigate"
    );
}

#[path = "explore_adaptive.tests.rs"]
mod adaptive;

#[path = "explore_evidence.tests.rs"]
mod evidence;

fn conclusion(summary: &str) -> review_explore::Conclusion {
    review_explore::Conclusion {
        summary: summary.into(),
        ..Default::default()
    }
}
