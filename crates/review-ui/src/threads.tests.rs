use super::*;
use ratatui::buffer::Buffer;
use review_guide::{DiffRangeAnchor, GuideAnchorKind};
use review_threads::{MessageId, Post, Resolution, ReviewThreads, ThreadCommand, ThreadId};
use ui_events::{ReviewNavigation, ReviewThreadsLoaded, ThreadPostFinished};

#[path = "threads/visibility.tests.rs"]
mod visibility;

#[path = "threads/live_peek.tests.rs"]
mod live_peek;

#[path = "threads/paths.tests.rs"]
mod paths;

struct ThreadUi {
    app: ReviewApplication,
    book: ReviewThreads,
    ids: Vec<ThreadId>,
    width: u16,
    height: u16,
}

impl ThreadUi {
    fn new(width: u16) -> Self {
        let mut app = ReviewApplication::new(Theme::default(), Some(34), PathBuf::from("/repo"));
        let height = 32;
        app.update(UserInput::Resize { width, height });
        publish_repository(
            &mut app,
            ReviewCheckpoint::new("change", "now"),
            "Thread navigation".into(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
        );
        let mut book = ReviewThreads::new("change".into());
        let ids = [
            ("src/lib.rs", "Explain this branch"),
            ("gone.rs", "Keep the removed file conversation"),
        ]
        .into_iter()
        .map(|(path, question)| {
            let post = Post::start(
                DiffRangeAnchor {
                    source_checkpoint: "original".into(),
                    old_path: None,
                    new_path: Some(path.into()),
                    old_lines: None,
                    new_lines: Some(0..8),
                    target_kind: GuideAnchorKind::Lines,
                    source_hunk_count: 1,
                    old_content: None,
                    new_content: Some(b"original\n".to_vec()),
                    diff_hash: String::new(),
                },
                "+first original line\n+second\n+third\n+fourth\n+fifth\n+sixth\n+seventh\n+eighth"
                    .into(),
                question.into(),
            );
            let id = post.thread_id().clone();
            book.post(post).unwrap();
            id
        })
        .collect();
        let mut fixture = Self {
            app,
            book,
            ids,
            width,
            height,
        };
        fixture.publish_book();
        fixture
    }

    fn publish_book(&mut self) {
        self.app.publish(ReviewThreadsLoaded {
            review_unit: self.book.review_unit.clone(),
            result: Ok(self.book.clone()),
        });
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        let actions = self.app.update(UserInput::Key(key));
        for action in &actions {
            self.accept(action);
        }
        actions
    }

    fn accept(&mut self, action: &Action) {
        match action {
            Action::Thread(ThreadCommand::MarkRepliesRead { messages, .. }) => {
                self.book.mark_replies_read(messages);
            }
            Action::Thread(ThreadCommand::MarkRead {
                thread_id, through, ..
            }) => self.book.mark_read(thread_id, *through).unwrap(),
            Action::Thread(ThreadCommand::SetResolution {
                thread_id,
                resolution,
                ..
            }) => self.book.set_resolution(thread_id, *resolution).unwrap(),
            Action::Thread(ThreadCommand::Post { post, .. }) => {
                self.book.post(post.clone()).unwrap();
                self.publish_book();
                self.app.publish(ThreadPostFinished {
                    review_unit: self.book.review_unit.clone(),
                    message_id: post.message().id.clone(),
                    result: Ok(()),
                });
            }
            _ => return,
        }
        self.publish_book();
    }

    fn load_reviewed_diff(&mut self, rows: Vec<DiffRow>) {
        self.height = 70;
        self.app.update(UserInput::Resize {
            width: self.width,
            height: self.height,
        });
        self.app.publish(ui_events::DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "now"),
            path: "src/lib.rs".into(),
            rows,
            old_content: None,
            new_content: Some(b"original\n".to_vec()),
        });
    }

    fn click_text(&mut self, text: &str) {
        let (column, row) =
            rendered_text_position(&self.app, text, self.width, self.height).unwrap();
        let actions = self.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        for action in &actions {
            self.accept(action);
        }
    }

    fn present(&mut self) -> Vec<Action> {
        self.buffer();
        let actions = self.app.publish(ui_events::FrameRendered);
        for action in &actions {
            self.accept(action);
        }
        actions
    }

    fn buffer(&self) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(self.width, self.height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(self.app.frame(), frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(&self) -> String {
        self.buffer()
            .content()
            .chunks(usize::from(self.width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn paste(&mut self, text: &str) {
        self.app.update(UserInput::Paste(text.into()));
    }

    fn assert_no_conversation(&mut self) {
        let text = self.text();
        for hidden in [
            "Explain this branch",
            "first original line",
            "Preserved reply",
            "Unresolve thread",
            "Ctrl-Enter post",
        ] {
            assert!(!text.contains(hidden), "{hidden}: {text}");
        }
        for key in [Key::Char('r'), Key::Char('a'), Key::ControlEnter] {
            assert!(self.key(key).is_empty());
        }
    }

    fn answer(&mut self, thread: usize, message_id: &str, text: &str) {
        self.book
            .post(Post::agent_reply(
                self.ids[thread].clone(),
                MessageId::parse(message_id).unwrap(),
                text.into(),
            ))
            .unwrap();
        self.publish_book();
    }
}

#[test]
fn threads_open_without_loading_reviewed_or_removed_files_in_wide_and_narrow_layouts() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        assert!(ui.text().contains("[F]iles"));
        assert!(ui.text().contains("[T]hreads"));
        ui.key(Key::Tab);
        assert_eq!(ui.app.focus, ReviewPane::Detail);
        assert!(ui.key(Key::Char('t')).is_empty());
        assert_eq!(ui.app.navigation, ReviewNavigation::Threads);
        ui.key(Key::Char('T'));
        assert_eq!(ui.app.navigation, ReviewNavigation::Threads);
        ui.key(Key::Down);
        assert!(ui.key(Key::Enter).is_empty());
        let text = ui.text();
        assert!(text.contains("Keep the removed"), "{text}");
        assert!(text.contains("first original line"), "{text}");
        assert!(
            !text.contains("Back to files") && !text.contains("Tab panes"),
            "{text}"
        );
        assert!(!text.contains("Ctrl-Enter post"));
        assert!(ui.key(Key::Char('f')).is_empty());
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        ui.key(Key::Char('F'));
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        assert_eq!(ui.app.focus, ReviewPane::Detail);
        assert!(ui.text().contains("1/1 reviewed"));
    }
}

#[test]
fn replies_preserve_each_composer_and_post_to_its_original_thread() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('A'));
    ui.paste("First unposted reply");
    ui.key(Key::Control('t'));
    ui.key(Key::Control('t'));
    ui.key(Key::Down);
    ui.key(Key::Enter);
    ui.key(Key::Char('A'));
    ui.paste("Second unposted reply");
    ui.key(Key::Control('t'));
    ui.key(Key::Control('t'));
    ui.key(Key::Up);
    ui.key(Key::Enter);
    assert!(ui.text().contains("First unposted reply"), "{}", ui.text());
    ui.key(Key::ControlEnter);
    assert_eq!(
        ui.book
            .thread(&ui.ids[0])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "First unposted reply"
    );
    assert_eq!(ui.book.thread(&ui.ids[1]).unwrap().messages.len(), 1);
    ui.key(Key::Tab);
    ui.key(Key::Down);
    ui.key(Key::Enter);
    ui.key(Key::ControlEnter);
    assert_eq!(
        ui.book
            .thread(&ui.ids[1])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "Second unposted reply"
    );
}

#[test]
fn late_replies_on_resolved_threads_are_findable_without_moving_focus() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('r'));
    assert_eq!(
        ui.book.thread(&ui.ids[0]).unwrap().resolution,
        Resolution::Resolved
    );
    ui.key(Key::Char('f'));
    let focus = ui.app.focus;
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000011",
        "Late explanation",
    );
    assert_eq!(ui.app.focus, focus);
    assert_eq!(ui.app.navigation, ReviewNavigation::Files);
    assert!(!ui.text().contains("new replies"));
    assert!(ui.text().contains("1/1 reviewed"));
    assert!(
        ui.text()
            .lines()
            .any(|line| line.contains("lib.rs ●") && !line.contains("💬"))
    );
    ui.key(Key::Alt('u'));
    ui.key(Key::Enter);
    assert!(ui.text().contains("Late explanation"), "{}", ui.text());
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
    assert_eq!(
        ui.book.thread(&ui.ids[0]).unwrap().resolution,
        Resolution::Resolved
    );
}

#[test]
fn source_peek_is_read_only_and_rejects_results_from_a_previous_thread() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    let [
        Action::LoadSource {
            snapshot_id,
            location,
            mode: ui_events::SourceLoadMode::ThreadPeek,
        },
        Action::WatchSource(Some(watched)),
    ] = actions.as_slice()
    else {
        panic!("unexpected actions: {actions:?}");
    };
    assert_eq!(location.path, PathBuf::from("/repo/src/lib.rs"));
    assert_eq!(watched, &location.path);
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: b"current source\n".to_vec(),
        mode: ui_events::SourceLoadMode::ThreadPeek,
    });
    assert!(ui.text().contains("current source"));
    assert!(ui.text().contains("1/1 reviewed"));
    ui.key(Key::Escape);
    assert!(ui.text().contains("first original line"));
    ui.key(Key::Tab);
    ui.key(Key::Down);
    ui.key(Key::Enter);
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: b"stale peek\n".to_vec(),
        mode: ui_events::SourceLoadMode::ThreadPeek,
    });
    assert!(!ui.text().contains("stale peek"));
    assert!(ui.text().contains("Keep the removed"));
}

#[test]
fn mouse_tabs_and_conversation_actions_work_after_resizing() {
    let mut ui = ThreadUi::new(110);
    ui.app.update(UserInput::MouseClick {
        column: 12,
        row: 1,
        insert_path: false,
    });
    assert_eq!(ui.app.navigation, ReviewNavigation::Threads);
    ui.app.update(UserInput::MouseClick {
        column: 5,
        row: 4,
        insert_path: false,
    });
    assert_eq!(ui.app.focus, ReviewPane::Detail);
    ui.width = 48;
    ui.app.update(UserInput::Resize {
        width: ui.width,
        height: ui.height,
    });
    ui.key(Key::Last);
    ui.click_text("Reply…");
    ui.paste("Reply after resizing");
    ui.key(Key::ControlEnter);
    assert_eq!(
        ui.book
            .thread(&ui.ids[0])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "Reply after resizing"
    );
}

#[test]
fn threads_resolution_stays_at_bottom_right_without_a_peek_button() {
    for width in [110, 48] {
        let mut ui = ThreadUi::new(width);
        ui.height = 70;
        ui.app.update(UserInput::Resize {
            width,
            height: ui.height,
        });
        ui.answer(
            0,
            "00000000-0000-4000-8000-000000000090",
            "The complete answer",
        );
        ui.key(Key::Char('t'));
        ui.key(Key::Char('2'));
        ui.key(Key::Enter);
        let (resolve_column, resolve_row) =
            rendered_text_position(&ui.app, "Resolve thread", width, ui.height).unwrap();
        let (_, reply_row) = rendered_text_position(&ui.app, "Reply…", width, ui.height).unwrap();
        assert!(resolve_row > reply_row);
        assert!(!ui.text().contains("Peek current file"));
        assert_eq!(
            resolve_column + u16::try_from("Resolve thread ".len()).unwrap(),
            width - 4
        );
        let actions = ui.app.update(UserInput::MouseClick {
            column: resolve_column - 2,
            row: resolve_row,
            insert_path: false,
        });
        assert!(actions.is_empty(), "padding is not a button");
        ui.click_text("Resolve thread");
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Resolved
        );
        assert!(!ui.text().contains("The complete answer"));
        ui.key(Key::Tab);
        ui.key(Key::First);
        ui.key(Key::Enter);
        assert!(
            ui.text().contains("The complete answer"),
            "All keeps resolved history available when selected again"
        );
        ui.click_text("Unresolve thread");
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Open
        );
        let actions = ui.key(Key::Char('p'));
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::LoadSource {
                mode: ui_events::SourceLoadMode::ThreadPeek,
                ..
            }
        )));
        ui.key(Key::Escape);
        assert!(ui.text().contains("The complete answer"));
    }
}

#[test]
fn search_matches_paths_and_replies_across_the_entire_review() {
    let mut ui = ThreadUi::new(110);
    ui.answer(
        1,
        "00000000-0000-4000-8000-000000000012",
        "Unique explanation",
    );
    ui.key(Key::Char('t'));
    ui.key(Key::Char('/'));
    for character in "unique".chars() {
        ui.key(Key::Char(character));
    }
    ui.key(Key::Enter);
    ui.key(Key::Enter);
    assert!(ui.text().contains("Keep the removed"));
    assert!(!ui.text().contains("Explain this branch"));
    ui.key(Key::Char('/'));
    for _ in 0..6 {
        ui.key(Key::Backspace);
    }
    for character in "src/lib".chars() {
        ui.key(Key::Char(character));
    }
    ui.key(Key::Enter);
    ui.key(Key::Enter);
    assert!(ui.text().contains("Explain this branch"));
    assert!(!ui.text().contains("Keep the removed"));
}

#[test]
fn an_incoming_reply_keeps_the_conversation_viewport_and_draft() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('A'));
    ui.paste("Still writing");
    let before = ui.buffer();
    let focus = ui.app.focus;
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000013",
        "A new answer\n".repeat(10).as_str(),
    );
    assert_eq!(ui.app.focus, focus);
    assert!(ui.text().contains("Still writing"), "{}", ui.text());
    assert!(ui.text().contains("Cancel") && ui.text().contains("Post"));
    // Adding messages below the original context cannot move that context.
    let after = ui.buffer();
    for row in 4..10 {
        for column in 36..108 {
            assert_eq!(before[(column, row)], after[(column, row)]);
        }
    }
    ui.key(Key::ControlEnter);
    assert_eq!(
        ui.book
            .thread(&ui.ids[0])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "Still writing"
    );
}

#[test]
fn reply_editor_follows_short_conversations_and_stays_visible_when_resized() {
    let mut ui = ThreadUi::new(110);
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000087",
        "Last line of the answer",
    );
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('A'));
    ui.paste("A nearby follow-up");
    let mut editor_row = None;
    for height in [70, 100, 32, 70] {
        ui.height = height;
        ui.app.update(UserInput::Resize {
            width: ui.width,
            height,
        });
        let (_, row) =
            rendered_text_position(&ui.app, "A nearby follow-up", ui.width, height).unwrap();
        assert!(ui.text().contains("Cancel") && ui.text().contains("Post"));
        if height > 32 {
            let (_, answer_row) =
                rendered_text_position(&ui.app, "Last line of the answer", ui.width, height)
                    .unwrap();
            assert!(
                row > answer_row && row - answer_row < 8,
                "editor must follow the answer"
            );
            assert_eq!(
                *editor_row.get_or_insert(row),
                row,
                "extra height belongs below the thread"
            );
        }
    }
    let (column, row) = rendered_text_position(&ui.app, "Post", ui.width, ui.height).unwrap();
    let actions = ui.app.update(UserInput::MouseClick {
        column,
        row,
        insert_path: false,
    });
    for action in &actions {
        ui.accept(action);
    }
    assert_eq!(
        ui.book
            .thread(&ui.ids[0])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "A nearby follow-up"
    );
    assert_eq!(ui.app.navigation, ReviewNavigation::Threads);
}

#[test]
fn reviewed_files_show_full_inline_replies_with_or_without_remaining_diff_rows() {
    for rows in [
        Vec::new(),
        vec![DiffRow::Add {
            new_line: 1,
            text: "reviewed_code_should_be_hidden".into(),
        }],
    ] {
        let mut ui = ThreadUi::new(110);
        ui.load_reviewed_diff(rows);
        ui.answer(
            0,
            "00000000-0000-4000-8000-000000000086",
            "The answer remains visible on a reviewed file.",
        );
        let text = ui.text();
        assert!(!text.contains("reviewed_code_should_be_hidden"), "{text}");
        assert!(!text.contains("first original line"), "{text}");
        assert!(text.contains("Explain this branch"), "{text}");
        assert!(text.contains("The answer remains visible"), "{text}");
        assert!(text.contains("Reply…"), "{text}");
        assert!(!text.contains("Open thread"), "{text}");
        ui.present();
        assert_eq!(ui.book.counts().unread, 0);
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        let (column, row) = rendered_text_position(&ui.app, "Reply…", ui.width, ui.height).unwrap();
        ui.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        ui.paste("Inline follow-up on a reviewed file");
        ui.key(Key::ControlEnter);
        assert_eq!(
            ui.book
                .thread(&ui.ids[0])
                .unwrap()
                .messages
                .last()
                .unwrap()
                .text,
            "Inline follow-up on a reviewed file"
        );
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        let (column, row) =
            rendered_text_position(&ui.app, "Resolve thread", ui.width, ui.height).unwrap();
        assert_eq!(
            column + u16::try_from("Resolve thread ".len()).unwrap(),
            ui.width - 4
        );
        let (_, draft_row) = rendered_text_position(
            &ui.app,
            "Inline follow-up on a reviewed file",
            ui.width,
            ui.height,
        )
        .unwrap();
        assert!(row > draft_row);
        assert!(
            ui.app
                .update(UserInput::MouseClick {
                    column: column - 2,
                    row,
                    insert_path: false,
                })
                .is_empty(),
            "the gap between Retry and Resolve must not trigger either action"
        );
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Open
        );
        let actions = ui.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        for action in &actions {
            ui.accept(action);
        }
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Resolved
        );
        assert_eq!(
            ui.book.thread(&ui.ids[1]).unwrap().resolution,
            Resolution::Open
        );
        assert!(!ui.text().contains("The answer remains visible"));
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        assert!(ui.text().contains("Resolved · Explain this branch"));
        assert!(ui.text().contains("Unresolve thread"));
        assert!(!ui.text().contains("Reply…"));
    }
}

#[test]
fn collapsed_inline_threads_preserve_drafts_and_unread_answers_until_reopened() {
    let mut ui = ThreadUi::new(110);
    ui.load_reviewed_diff(Vec::new());
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000089",
        "An earlier answer",
    );
    ui.click_text("Reply…");
    ui.paste("Keep my draft");
    ui.click_text("Resolve thread");
    assert!(ui.text().contains("Resolved · Explain this branch"));
    assert!(!ui.text().contains("Keep my draft"));
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000088",
        "A late answer in a collapsed thread",
    );
    ui.present();
    assert_eq!(ui.book.counts().unread, 1, "collapsed answers stay unread");
    ui.click_text("Unresolve thread");
    assert_eq!(
        ui.book.thread(&ui.ids[0]).unwrap().resolution,
        Resolution::Open
    );
    assert!(ui.text().contains("An earlier answer"));
    assert!(ui.text().contains("A late answer in a collapsed thread"));
    assert!(ui.text().contains("Keep my draft"));
    assert!(ui.text().contains("1/1 reviewed"));
    assert_eq!(ui.app.navigation, ReviewNavigation::Files);
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
}

#[test]
fn narrow_navigation_keeps_replies_unread_until_the_answer_is_displayed() {
    let mut ui = ThreadUi::new(48);
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000031",
        "Unread answer\n\nMore detail\n\nAnother paragraph\n\nThe rest of this answer starts below the viewport.",
    );
    ui.key(Key::Alt('u'));
    assert_eq!(ui.app.focus, ReviewPane::Navigation);
    assert_eq!(ui.book.counts().unread, 1);
    ui.key(Key::Down);
    assert_eq!(ui.book.counts().unread, 1);
    ui.key(Key::Tab);
    assert_eq!(ui.app.focus, ReviewPane::Detail);
    assert_eq!(ui.book.counts().unread, 1);
    ui.key(Key::Alt('u'));
    assert!(ui.text().contains("Unresolved / [All]"));
    ui.key(Key::Tab);
    ui.present();
    assert_eq!(
        ui.book.counts().unread,
        1,
        "the reply is below the visible context"
    );
    ui.key(Key::PageDown);
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
}

#[test]
fn filename_opens_files_and_preserves_the_conversation_draft() {
    for width in [110, 48] {
        let mut ui = ThreadUi::new(width);
        ui.key(Key::Char('t'));
        ui.key(Key::Down);
        ui.key(Key::Enter);
        ui.key(Key::Char('A'));
        ui.paste("Private draft");
        let mut terminal = Terminal::new(TestBackend::new(width, ui.height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(ui.app.frame(), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let (column, row) = (0..ui.height)
            .find_map(|row| {
                (0..width)
                    .find(|column| {
                        buffer[(*column, row)]
                            .modifier
                            .contains(ratatui::style::Modifier::UNDERLINED)
                    })
                    .map(|column| (column, row))
            })
            .expect("the filename is an underlined link");
        ui.app.update(UserInput::MouseClick {
            column: width - 3,
            row,
            insert_path: false,
        });
        assert_eq!(
            ui.app.navigation,
            ReviewNavigation::Threads,
            "padding is not a link"
        );
        ui.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        assert_eq!(ui.app.navigation, ReviewNavigation::Files);
        assert_eq!(ui.app.focus, ReviewPane::Detail);
        assert!(ui.text().contains("Diff · gone.rs"), "{}", ui.text());
        assert_eq!(ui.book.thread(&ui.ids[1]).unwrap().messages.len(), 1);
        ui.key(Key::Char('t'));
        ui.key(Key::Tab);
        ui.key(Key::ControlEnter);
        assert_eq!(
            ui.book
                .thread(&ui.ids[1])
                .unwrap()
                .messages
                .last()
                .unwrap()
                .text,
            "Private draft"
        );
    }
}

#[test]
fn current_source_peek_highlights_only_a_verified_range() {
    let mut ui = ThreadUi::new(110);
    let mut value = ui.book.threads()[0].anchor.clone();
    value.new_lines = Some(0..1);
    let post = Post::start(
        value,
        "+original".into(),
        "Highlight the current range".into(),
    );
    let id = post.thread_id().clone();
    ui.book.post(post).unwrap();
    ui.publish_book();
    ui.key(Key::Char('t'));
    ui.key(Key::Last);
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    let [
        Action::LoadSource {
            snapshot_id,
            location,
            ..
        },
        Action::WatchSource(Some(_)),
    ] = actions.as_slice()
    else {
        panic!("expected source peek");
    };
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        mode: ui_events::SourceLoadMode::ThreadPeek,
        content: b"inserted\noriginal\n".to_vec(),
    });
    assert!(ui.text().contains("Current file"));
    let buffer = ui.buffer();
    let highlighted = buffer
        .content()
        .iter()
        .filter(|cell| {
            cell.bg == Theme::default().palette.selection
                || cell.bg == Theme::default().palette.cursor
        })
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(highlighted.contains("original"), "{highlighted}");
    assert!(!highlighted.contains("inserted"));
    assert_eq!(ui.book.thread(&id).unwrap().excerpt, "+original");
}

#[test]
fn thread_badges_stay_on_reviewed_files_and_never_appear_on_directories() {
    let mut ui = ThreadUi::new(110);
    ui.answer(0, "00000000-0000-4000-8000-000000000032", "Keep this badge");
    let lines = ui.text();
    let file = lines
        .lines()
        .find(|line| line.contains("✓ lib.rs"))
        .unwrap();
    assert!(file.contains("💬  ●"), "{file}");
    let directory = lines.lines().find(|line| line.contains("▾ src/")).unwrap();
    assert!(
        !directory.contains("💬") && !directory.contains('●'),
        "{directory}"
    );
    ui.app.update(UserInput::MouseClick {
        column: 1,
        row: 2,
        insert_path: false,
    });
    let lines = ui.text();
    let directory = lines
        .lines()
        .find(|line| line.contains("▸ src/"))
        .unwrap_or_else(|| panic!("{lines}"));
    assert!(
        !directory.contains("💬") && !directory.contains('●'),
        "{directory}"
    );
    ui.key(Key::Char('t'));
    assert!(ui.text().contains("File reviewed"), "{}", ui.text());
}

#[test]
fn bottom_filters_keep_resolved_history_in_all_with_boxed_entries() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        for id in &ui.ids {
            ui.book.set_resolution(id, Resolution::Resolved).unwrap();
        }
        ui.publish_book();
        ui.key(Key::Char('t'));
        ui.key(Key::Char('1'));
        assert!(!ui.text().contains("No unresolved threads"));
        assert!(!ui.text().contains("/ search"));
        assert!(!ui.text().contains("2 All"));
        let (_, row) =
            rendered_text_position(&ui.app, "[Unresolved] / All", ui.width, ui.height).unwrap();
        assert_eq!(row, ui.height - 3);
        ui.key(Key::Char('2'));
        let text = ui.text();
        assert!(text.contains("Unresolved / [All]"), "{text}");
        assert!(text.contains("Explain this branch"), "{text}");
        assert!(text.contains("Keep the removed file"), "{text}");
        let buffer = ui.buffer();
        assert_eq!(buffer[(1, 2)].symbol(), "┌");
        assert_eq!(buffer[(1, 7)].symbol(), "└");
        assert_eq!(buffer[(1, 8)].symbol(), "┌");
        ui.key(Key::Enter);
        ui.key(Key::Last);
        assert!(ui.text().contains("Unresolve thread"), "{}", ui.text());
        ui.key(Key::Char('r'));
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Open
        );
        ui.key(Key::Tab);
        let (column, row) =
            rendered_text_position(&ui.app, "Unresolved", ui.width, ui.height).unwrap();
        ui.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        assert!(ui.text().contains("[Unresolved] / All"));
        assert!(ui.text().contains("Explain this branch"));
        assert!(!ui.text().contains("Keep the removed file"));
        let (column, row) = rendered_text_position(&ui.app, "All", ui.width, ui.height).unwrap();
        assert_eq!(row, ui.height - 3);
        ui.app.update(UserInput::MouseClick {
            column,
            row,
            insert_path: false,
        });
        assert!(ui.text().contains("Keep the removed file"));
        assert!(ui.text().contains("Explain this branch"));
        ui.height = 24;
        ui.app.update(UserInput::Resize {
            width,
            height: ui.height,
        });
        let (_, row) = rendered_text_position(&ui.app, "[All]", ui.width, ui.height).unwrap();
        assert_eq!(row, ui.height - 3);
    }
}

#[test]
fn unread_dots_are_red_on_the_tab_file_and_thread_card_until_read() {
    let mut ui = ThreadUi::new(110);
    ui.app.publish(ui_events::GuidePathsChanged {
        paths: vec!["src/lib.rs".into()],
    });
    ui.answer(0, "00000000-0000-4000-8000-000000000041", "Unread answer");
    let buffer = ui.buffer();
    let file = ui
        .text()
        .lines()
        .find(|line| line.contains("✓ lib.rs"))
        .unwrap()
        .to_owned();
    assert!(file.contains("lib.rs 📄  💬  ●"), "{file}");
    let tab_dot = (0..34)
        .find(|column| buffer[(*column, 1)].symbol() == "●")
        .unwrap();
    assert_eq!(buffer[(tab_dot, 1)].fg, Theme::default().palette.deletion);
    let file_dot = buffer
        .content()
        .chunks(110)
        .skip(2)
        .flat_map(|row| &row[..34])
        .find(|cell| cell.symbol() == "●")
        .unwrap();
    assert_eq!(file_dot.fg, Theme::default().palette.deletion);
    ui.key(Key::Char('t'));
    let buffer = ui.buffer();
    let card_dot = (1..33)
        .find(|column| buffer[(*column, 5)].symbol() == "●")
        .unwrap();
    assert_eq!(buffer[(card_dot, 5)].fg, Theme::default().palette.deletion);
    ui.key(Key::Enter);
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
    assert!(!(0..34).any(|column| ui.buffer()[(column, 1)].symbol() == "●"));
}

#[test]
fn pasting_into_search_leaves_a_parked_reply_unchanged() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('A'));
    ui.paste("Keep this draft");
    let (column, row) = rendered_text_position(&ui.app, "All", ui.width, ui.height).unwrap();
    ui.app.update(UserInput::MouseClick {
        column,
        row,
        insert_path: false,
    });
    ui.key(Key::Char('/'));
    ui.paste("gone.rs");
    ui.key(Key::Enter);
    assert!(!ui.text().contains("Explain this branch"));
    ui.key(Key::Char('/'));
    for _ in 0..7 {
        ui.key(Key::Backspace);
    }
    ui.paste("src/lib.rs");
    ui.key(Key::Enter);
    ui.key(Key::Enter);
    ui.key(Key::ControlEnter);
    assert_eq!(
        ui.book
            .thread(&ui.ids[0])
            .unwrap()
            .messages
            .last()
            .unwrap()
            .text,
        "Keep this draft"
    );
}

#[test]
fn a_renamed_threads_file_review_notice_follows_the_current_path() {
    let mut ui = ThreadUi::new(110);
    let mut renamed = FileSummary::new("src/renamed.rs", ReviewStatus::Reviewed);
    renamed.file.change = review_repository::repository::ChangeKind::Renamed;
    renamed.file.old_path =
        review_repository::repository::ChangedFile::modified("src/lib.rs").old_path;
    publish_repository(
        &mut ui.app,
        ReviewCheckpoint::new("change", "now"),
        "Thread navigation".into(),
        vec![renamed],
    );
    ui.key(Key::Char('t'));
    assert!(ui.text().contains("File reviewed"), "{}", ui.text());
    ui.app.publish(ui_events::ReviewStateSaved {
        review_unit: ui.book.review_unit.clone(),
        path: "src/renamed.rs".into(),
        result: Ok(FileSummary::new("src/renamed.rs", ReviewStatus::Unreviewed).review_state),
    });
    assert!(!ui.text().contains("File reviewed"), "{}", ui.text());
}

#[test]
fn original_context_always_includes_the_full_saved_range() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let text = ui.text();
    assert!(
        text.contains("seventh") && text.contains("eighth"),
        "{text}"
    );
    assert!(!text.contains("Expand original") && !text.contains("Collapse original"));
    for removed in [
        "Original context",
        "checkpoint original",
        "No unread replies",
        "new lines",
        "old lines",
    ] {
        assert!(!text.contains(removed), "{text}");
    }
    let buffer = ui.buffer();
    let detail = (0..ui.height)
        .map(|row| {
            (36..ui.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let path_row = detail
        .iter()
        .position(|row| row.contains("src/lib.rs"))
        .unwrap();
    assert_eq!(
        detail[path_row]
            .trim_matches(|character: char| character == '│' || character.is_whitespace()),
        "src/lib.rs"
    );
    assert!(detail[path_row + 1].contains("────"));
    assert!(detail[path_row + 2].contains("first original line"));
    ui.key(Key::Char('x'));
    assert_eq!(text, ui.text());
}

#[test]
fn peek_uses_native_highlighting_navigation_and_lsp_without_changing_files() {
    let mut ui = ThreadUi::new(110);
    let files = ui.text();
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    let Action::LoadSource {
        snapshot_id,
        location,
        mode,
    } = &actions[0]
    else {
        panic!("source load");
    };
    let loaded = ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        mode: *mode,
        content: b"fn first() {}\nfn second() {}\n".to_vec(),
    });
    assert!(
        loaded.iter().any(
            |action| matches!(action, Action::OpenLspDocument(path) if path == &location.path)
        )
    );
    let request = loaded
        .iter()
        .find_map(|action| match action {
            Action::Highlight(request) => Some(request.clone()),
            _ => None,
        })
        .expect("native highlighting request");
    let ui_events::HighlightRequest::Source(content) = &request else {
        panic!("source highlighting");
    };
    let theme = Theme::default();
    let highlighted = diff_component::SyntaxHighlighter::new(theme.syntax, theme.palette.text)
        .highlight("src/lib.rs", Vec::new(), None, Some(&content.content));
    let plain = ui.buffer();
    ui.app.publish(ui_events::HighlightingFinished {
        request,
        highlighted,
    });
    assert_ne!(plain, ui.buffer(), "source syntax colors must be applied");
    ui.key(Key::Down);
    ui.key(Key::Char('l'));
    let actions = ui.key(Key::Char('K'));
    assert!(
        matches!(&actions[..], [Action::Lsp { operation: review_lsp::Operation::Hover, query }] if query.line == 1 && query.byte_column == 1 && query.expected_line == "fn second() {}" && query.snapshot_id == *snapshot_id),
        "{actions:?}"
    );
    ui.key(Key::Char('g'));
    let actions = ui.key(Key::Char('d'));
    let [Action::Lsp { operation, query }] = actions.as_slice() else {
        panic!("definition request: {actions:?}");
    };
    assert_eq!(*operation, review_lsp::Operation::Definition);
    let target = review_lsp::SourceLocation {
        path: "/repo/other.rs".into(),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 0,
    };
    let actions = ui.app.publish(review_lsp::Event::Locations {
        operation: *operation,
        toast_id: query.toast_id,
        snapshot_id: query.snapshot_id.clone(),
        locations: vec![target.clone()],
    });
    let Action::LoadSource {
        snapshot_id, mode, ..
    } = &actions[0]
    else {
        panic!("definition source load: {actions:?}");
    };
    assert_ne!(
        snapshot_id, "now",
        "peek navigation reads current disk content"
    );
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: target,
        mode: *mode,
        content: b"fn definition() {}\n".to_vec(),
    });
    assert!(ui.text().contains("fn definition() {}"));
    ui.key(Key::Char('/'));
    for character in "definition".chars() {
        ui.key(Key::Char(character));
    }
    ui.key(Key::Enter);
    assert!(ui.text().contains("/definition"));
    ui.key(Key::Escape);
    assert!(ui.text().contains("first original line"));
    ui.key(Key::Char('f'));
    assert_eq!(files, ui.text());
}

#[test]
fn delayed_lsp_results_cannot_leave_a_closed_or_replaced_peek() {
    for switch_thread in [false, true] {
        let mut ui = ThreadUi::new(110);
        ui.key(Key::Char('t'));
        ui.key(Key::Enter);
        let actions = ui.key(Key::Char('p'));
        let Action::LoadSource {
            snapshot_id,
            location,
            mode,
        } = &actions[0]
        else {
            panic!("source load");
        };
        ui.app.publish(SourceContentLoaded {
            snapshot_id: snapshot_id.clone(),
            location: location.clone(),
            mode: *mode,
            content: b"fn symbol() {}\n".to_vec(),
        });
        ui.key(Key::Char('g'));
        let actions = ui.key(Key::Char('d'));
        let Action::Lsp { operation, query } = &actions[0] else {
            panic!("definition request");
        };
        if switch_thread {
            ui.app.publish(ui_events::ThreadSelectionChanged {
                thread_id: Some(ui.ids[1].clone()),
            });
        } else {
            ui.key(Key::Escape);
            ui.key(Key::Char('f'));
        }
        let before = ui.text();
        let actions = ui.app.publish(review_lsp::Event::Locations {
            operation: *operation,
            toast_id: query.toast_id,
            snapshot_id: query.snapshot_id.clone(),
            locations: vec![location.clone()],
        });
        assert!(
            actions.is_empty(),
            "closed peek must not load a definition: {actions:?}"
        );
        assert_eq!(before, ui.text());
    }
}

#[test]
fn failed_definition_load_keeps_the_current_peek_and_original_context() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    let Action::LoadSource {
        snapshot_id,
        location,
        mode,
    } = &actions[0]
    else {
        panic!("source load");
    };
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        mode: *mode,
        content: b"fn symbol() {}\n".to_vec(),
    });
    ui.app.publish(ui_events::SourceContentLoadFailed {
        snapshot_id: snapshot_id.clone(),
        message: "Missing definition target".into(),
    });
    assert!(ui.text().contains("fn symbol() {}"));
    assert!(!ui.text().contains("Current file unavailable"));
    ui.key(Key::Escape);
    assert!(ui.text().contains("first original line"));
    assert!(!ui.text().contains("Current file unavailable"));
}

#[test]
fn files_search_results_arriving_during_peek_remain_available_on_return() {
    let mut ui = ThreadUi::new(110);
    let location = review_lsp::SourceLocation {
        path: "/repo/search.rs".into(),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 0,
    };
    ui.app.publish(SourceContentLoaded {
        snapshot_id: "now".into(),
        location,
        mode: ui_events::SourceLoadMode::External,
        content: "needle\n".repeat(10_000).into_bytes(),
    });
    ui.key(Key::Tab);
    ui.key(Key::Char('/'));
    let mut pending = None;
    for character in "needle".chars() {
        for action in ui.key(Key::Char(character)) {
            if let Action::Search(Some(request)) = action {
                pending = Some(request);
            }
        }
    }
    let request = pending.expect("large Files search is asynchronous");
    ui.key(Key::Enter);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    ui.key(Key::Char('p'));
    assert!(ui.text().contains("Current file"));
    ui.app.publish(request.search());
    ui.key(Key::Escape);
    ui.key(Key::Char('f'));
    let text = ui.text();
    assert!(
        text.contains("/needle") && text.contains("2/10000"),
        "{text}"
    );
}

#[test]
fn reply_actions_share_one_row_with_only_post_highlighted() {
    for navigation in [ReviewNavigation::Files, ReviewNavigation::Threads] {
        let mut ui = ThreadUi::new(110);
        ui.load_reviewed_diff(Vec::new());
        if navigation == ReviewNavigation::Threads {
            ui.key(Key::Char('t'));
            ui.key(Key::Enter);
        }
        ui.click_text("Reply…");
        ui.paste("A shared footer reply");
        let mut terminal = Terminal::new(TestBackend::new(ui.width, ui.height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(ui.app.frame(), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let palette = Theme::default().palette;
        let mut button_row = None;
        for (label, background) in [
            ("Cancel", palette.selection),
            ("Post", palette.insertion),
            ("Resolve thread", palette.selection),
        ] {
            let (column, row) =
                rendered_text_position(&ui.app, label, ui.width, ui.height).unwrap();
            assert_eq!(
                *button_row.get_or_insert(row),
                row,
                "all actions share one row"
            );
            assert_eq!(buffer[(column, row)].bg, background, "{label}");
        }
        ui.click_text("Post");
        assert_eq!(
            ui.book
                .thread(&ui.ids[0])
                .unwrap()
                .messages
                .last()
                .unwrap()
                .text,
            "A shared footer reply"
        );
        ui.click_text("Reply…");
        ui.paste("Discard this draft");
        ui.click_text("Cancel");
        assert_eq!(ui.book.thread(&ui.ids[0]).unwrap().messages.len(), 2);
        ui.click_text("Resolve thread");
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Resolved
        );
    }
}

#[test]
fn reviewed_files_with_only_resolved_threads_hide_cached_code() {
    let mut ui = ThreadUi::new(110);
    ui.app.publish(ui_events::ReviewStateSaved {
        review_unit: ui.book.review_unit.clone(),
        path: "src/lib.rs".into(),
        result: Ok(FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed).review_state),
    });
    ui.book
        .set_resolution(&ui.ids[0], Resolution::Resolved)
        .unwrap();
    ui.publish_book();
    ui.load_reviewed_diff(
        (1..=120)
            .map(|new_line| DiffRow::Add {
                new_line,
                text: "reviewed_code_should_be_hidden".into(),
            })
            .collect(),
    );
    assert!(ui.text().contains("reviewed_code_should_be_hidden"));
    ui.key(Key::Tab);
    ui.key(Key::Last);
    ui.app.publish(ui_events::ReviewStateSaved {
        review_unit: ui.book.review_unit.clone(),
        path: "src/lib.rs".into(),
        result: Ok(FileSummary::new("src/lib.rs", ReviewStatus::Reviewed).review_state),
    });
    let text = ui.text();
    assert!(text.contains("Resolved · Explain this branch"), "{text}");
    assert!(text.contains("Unresolve thread"), "{text}");
    assert!(!text.contains("reviewed_code_should_be_hidden"), "{text}");
    ui.click_text("Unresolve thread");
    let text = ui.text();
    assert!(text.contains("Reply…"), "{text}");
    assert!(!text.contains("reviewed_code_should_be_hidden"), "{text}");
}

#[test]
fn empty_filters_clear_conversation_actions_and_park_the_reply() {
    for (width, search) in [(48, false), (48, true), (110, false), (110, true)] {
        let mut ui = ThreadUi::new(width);
        for id in &ui.ids {
            ui.book.set_resolution(id, Resolution::Resolved).unwrap();
        }
        ui.publish_book();
        ui.key(Key::Char('t'));
        ui.key(Key::Char('2'));
        ui.key(Key::Enter);
        ui.key(Key::Char('A'));
        ui.paste("Preserved reply");
        // The tab shortcut reaches navigation even in a narrow composing pane.
        ui.key(Key::Control('t'));
        ui.key(Key::Control('t'));
        if search {
            ui.key(Key::Char('/'));
            ui.paste("no-match");
            ui.key(Key::Enter);
        } else {
            ui.key(Key::Char('1'));
        }
        ui.key(Key::Enter);
        ui.assert_no_conversation();
        assert_eq!(ui.book.counts().open, 0);
        assert_eq!(ui.book.thread(&ui.ids[0]).unwrap().messages.len(), 1);
        ui.key(Key::Tab);
        if search {
            ui.key(Key::Char('/'));
            for _ in 0..8 {
                ui.key(Key::Backspace);
            }
            ui.key(Key::Enter);
        } else {
            ui.key(Key::Char('2'));
        }
        ui.key(Key::Enter);
        assert!(
            ui.text().contains("Preserved reply"),
            "width {width}, search {search}: {}",
            ui.text()
        );
        ui.key(Key::ControlEnter);
        assert_eq!(
            ui.book
                .thread(&ui.ids[0])
                .unwrap()
                .messages
                .last()
                .unwrap()
                .text,
            "Preserved reply"
        );
    }
}

#[path = "threads/resolution.tests.rs"]
mod resolution;
