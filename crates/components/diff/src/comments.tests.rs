use super::*;
use review_threads::{MessageId, Post, ReviewThreads};

#[path = "comments/scrolling.tests.rs"]
mod scrolling;

#[path = "comments/collapsed.tests.rs"]
mod collapsed;

struct CommentFixture {
    registry: ComponentEventBus<Action>,
    target: ComponentTarget,
}

impl CommentFixture {
    fn new() -> Self {
        let (mut registry, reviewable_files, target) = registry_with_observer();
        reviewable_files.replace(["src/lib.rs".to_owned()].into());
        publish_repository(&mut registry, "checkpoint");
        registry
            .publish(ui_events::ReviewThreadsLoaded {
                review_unit: "change".into(),
                result: Ok(ReviewThreads::new("change".into())),
            })
            .unwrap();
        registry
            .publish(FileSelected {
                path: "src/lib.rs".into(),
            })
            .unwrap();
        registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: "src/lib.rs".into(),
                rows: [
                    vec![
                        DiffRow::FileHeader {
                            old_path: None,
                            new_path: None,
                            text: "diff --git a/src/lib.rs b/src/lib.rs".into(),
                        },
                        DiffRow::Meta {
                            text: "--- a/src/lib.rs".into(),
                        },
                        DiffRow::Meta {
                            text: "+++ b/src/lib.rs".into(),
                        },
                    ],
                    changed_rows(),
                ]
                .concat(),
                old_content: Some(Vec::new()),
                new_content: Some(b"changed\n".to_vec()),
            })
            .unwrap();
        registry
            .publish(DiffViewportChanged {
                width: 78,
                height: 10,
            })
            .unwrap();
        Self { registry, target }
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        let actions = self.key_without_ack(key);
        self.accept(&actions);
        actions
    }

    fn key_without_ack(&mut self, key: Key) -> Vec<Action> {
        dispatch_key(&mut self.registry, self.target, key)
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect()
    }

    fn accept(&mut self, actions: &[Action]) {
        for action in actions {
            if let Action::Thread(review_threads::ThreadCommand::MarkRepliesRead {
                messages, ..
            }) = action
            {
                let mut book = self.book().clone();
                book.mark_replies_read(messages);
                self.registry
                    .publish(ui_events::ReviewThreadsLoaded {
                        review_unit: book.review_unit.clone(),
                        result: Ok(book),
                    })
                    .unwrap();
                continue;
            }

            let Action::Thread(review_threads::ThreadCommand::Post { review_unit, post }) = action
            else {
                continue;
            };
            let mut book = self.book().clone();
            book.post(post.clone()).unwrap();
            self.registry
                .publish(ui_events::ReviewThreadsLoaded {
                    review_unit: review_unit.clone(),
                    result: Ok(book),
                })
                .unwrap();
            self.registry
                .publish(ui_events::ThreadPostFinished {
                    review_unit: review_unit.clone(),
                    message_id: post.message().id.clone(),
                    result: Ok(()),
                })
                .unwrap();
        }
    }

    fn component(&self) -> &DiffComponent {
        self.registry.get(self.target).unwrap()
    }

    fn add(&mut self, text: &str) -> Vec<Action> {
        self.key(Key::Last);
        self.key(Key::Char('a'));
        self.registry
            .publish(ui_events::TextPasted(text.into()))
            .unwrap();
        self.key(Key::ControlEnter)
    }

    fn book(&self) -> &ReviewThreads {
        self.component().comments.book.as_ref().unwrap()
    }

    fn answer(&mut self, answer: &str) {
        let mut book = self.book().clone();
        book.post(Post::agent_reply(
            book.threads()[0].id.clone(),
            MessageId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
            answer.into(),
        ))
        .unwrap();
        self.registry
            .publish(ui_events::ReviewThreadsLoaded {
                review_unit: book.review_unit.clone(),
                result: Ok(book),
            })
            .unwrap();
    }

    fn render_thread(&self) -> Buffer {
        self.render_in(Rect::new(0, 0, 80, 24))
    }

    fn render_in(&self, area: Rect) -> Buffer {
        let mut buffer = Buffer::empty(area);
        self.component()
            .render(area, &mut buffer, Theme::default().palette, true, None)
            .render(&mut buffer);
        buffer
    }

    fn click_text(&mut self, text: &str) -> Vec<Action> {
        let buffer = self.render_thread();
        let (row, column) = Self::text_position(&buffer, text);
        self.click(row, column)
    }

    fn text_position(buffer: &Buffer, text: &str) -> (u16, u16) {
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .enumerate()
            .find_map(|(index, row)| {
                let line = row
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>();
                line.find(text).map(|offset| {
                    (
                        u16::try_from(index).unwrap(),
                        u16::try_from(unicode_width::UnicodeWidthStr::width(&line[..offset]))
                            .unwrap(),
                    )
                })
            })
            .unwrap_or_else(|| panic!("{text:?} is not visible in {buffer:?}"))
    }

    fn click(&mut self, row: u16, column: u16) -> Vec<Action> {
        let actions = self
            .registry
            .dispatch_hovered_input(
                &EventEnvelope::new(pointer_input(
                    PointerInputKind::Click { insert: false },
                    row,
                    column,
                )),
                self.target,
            )
            .unwrap()
            .into_results()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect::<Vec<_>>();
        self.accept(&actions);
        actions
    }
}

#[test]
fn post_buttons_publish_comments_and_replies_after_resizing_and_scrolling() {
    for width in [80, 26] {
        let mut fixture = CommentFixture::new();
        fixture.key(Key::Last);
        fixture.key(Key::Char('a'));
        fixture
            .registry
            .publish(ui_events::TextPasted("First comment".into()))
            .unwrap();
        fixture
            .registry
            .publish(DiffViewportChanged {
                width: width - 2,
                height: 10,
            })
            .unwrap();
        let buffer = fixture.render_in(Rect::new(0, 0, width, 12));
        let (row, column) = CommentFixture::text_position(&buffer, "Post");
        CommentFixture::text_position(&buffer, "Cancel");
        assert!(
            fixture
                .component()
                .displayed_document()
                .unwrap()
                .document
                .scroll
                > 0
        );
        assert!(fixture.click(row, column - 2).is_empty());
        assert!(fixture.component().comments.editing.is_some());
        assert!(fixture.book().threads().is_empty());

        let actions = fixture.click(row, column + 3);
        assert!(fixture.component().comments.editing.is_none());
        assert_eq!(
            fixture.book().threads()[0].messages[0].text,
            "First comment"
        );
        assert!(matches!(
            actions.as_slice(),
            [Action::Thread(review_threads::ThreadCommand::Post { .. })]
        ));
        assert!(fixture.click(row, column).is_empty());

        fixture.key(Key::Char('A'));
        fixture
            .registry
            .publish(ui_events::TextPasted("Follow-up".into()))
            .unwrap();
        let buffer = fixture.render_in(Rect::new(0, 0, width, 12));
        let (row, column) = CommentFixture::text_position(&buffer, "Post");
        assert!(matches!(
            fixture.click(row, column).as_slice(),
            [Action::Thread(review_threads::ThreadCommand::Post { .. })]
        ));
        assert_eq!(fixture.book().threads().len(), 1);
        assert_eq!(fixture.book().threads()[0].messages[1].text, "Follow-up");
    }
}

#[test]
fn cancel_discards_unposted_text_and_keeps_the_conversation() {
    let mut fixture = CommentFixture::new();
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Discard this".into()))
        .unwrap();
    assert!(fixture.click_text("Cancel").is_empty());
    assert!(fixture.component().comments.editing.is_none());
    assert!(fixture.book().threads().is_empty());

    fixture.add("Posted comment");
    fixture.answer("Agent answer");
    let original = fixture.book().clone();
    fixture.key(Key::Char('A'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Unsaved text".into()))
        .unwrap();
    assert!(fixture.click_text("Unsaved text").is_empty());
    assert!(fixture.click_text("Cancel").is_empty());
    assert!(fixture.component().comments.editing.is_none());
    assert_eq!(fixture.book(), &original);
}

#[test]
fn finalized_selection_opens_editor_with_vim_escape_and_explicit_post() {
    let mut fixture = CommentFixture::new();
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 78,
            height: 22,
        })
        .unwrap();
    fixture.key(Key::Last);
    fixture.key(Key::Visual);
    fixture.key(Key::Visual);
    assert!(fixture.component().comments.editing.is_some());
    let buffer = fixture.render_thread();
    CommentFixture::text_position(&buffer, "You  Ctrl-Enter post");
    CommentFixture::text_position(&buffer, "Vim · INSERT");
    fixture
        .registry
        .publish(ui_events::TextPasted("Explain this\nsecond line".into()))
        .unwrap();
    for (key, mode) in [
        (Key::Escape, "Normal"),
        (Key::Escape, "Normal"),
        (Key::Char('v'), "Visual"),
        (Key::Escape, "Normal"),
        (Key::Char('/'), "Search"),
        (Key::Escape, "Normal"),
    ] {
        assert!(fixture.key(key).is_empty());
        let editor = &fixture
            .component()
            .comments
            .editing
            .as_ref()
            .unwrap()
            .editor;
        assert_eq!(editor.mode(), mode);
        assert_eq!(editor.text(), "Explain this\nsecond line");
    }
    assert!(fixture.book().threads().is_empty());
    assert!(matches!(
        fixture.key(Key::ControlEnter).as_slice(),
        [Action::Thread(review_threads::ThreadCommand::Post { .. })]
    ));
    assert!(fixture.component().comments.editing.is_none());
    assert_eq!(
        fixture.book().threads()[0].messages[0].text,
        "Explain this\nsecond line"
    );
}

#[test]
fn posted_messages_are_immutable_and_ctrl_s_does_not_retry_or_post() {
    let mut fixture = CommentFixture::new();
    fixture.add("Original message");
    let original = fixture.book().clone();
    fixture.click_text("Original message");
    assert!(fixture.key(Key::Char('e')).is_empty());
    assert!(fixture.component().comments.editing.is_none());
    assert_eq!(fixture.book(), &original);
    assert!(fixture.key(Key::Control('s')).is_empty());
    let rendered = fixture
        .render_thread()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(!rendered.contains("Ctrl-s"));
    fixture.answer("Agent answer");
    fixture.click_text("Agent answer");
    assert!(fixture.component().comments.editing.is_none());
    fixture.click_text("Reply…");
    fixture
        .registry
        .publish(ui_events::TextPasted("Correction".into()))
        .unwrap();
    assert!(fixture.key(Key::Control('s')).is_empty());
    assert_eq!(fixture.book().threads()[0].messages.len(), 2);
    fixture.key(Key::ControlEnter);
    let messages = &fixture.book().threads()[0].messages;
    assert_eq!(messages[0], original.threads()[0].messages[0]);
    assert_eq!(
        messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
        ["Original message", "Agent answer", "Correction"]
    );
}

#[test]
fn an_agent_reply_preserves_unposted_text_and_speaker_order() {
    let mut fixture = CommentFixture::new();
    fixture.add("Original message");
    fixture.key(Key::Char('A'));
    fixture
        .registry
        .publish(ui_events::TextPasted("One more detail".into()))
        .unwrap();
    fixture.answer("Agent answer");
    assert_eq!(
        fixture
            .component()
            .comments
            .editing
            .as_ref()
            .unwrap()
            .editor
            .text(),
        "One more detail"
    );
    fixture.key(Key::ControlEnter);
    assert_eq!(
        fixture.book().threads()[0]
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["Original message", "Agent answer", "One more detail"]
    );
    fixture.key(Key::First);
    let buffer = fixture.render_in(Rect::new(0, 0, 80, 40));
    let (user, _) = CommentFixture::text_position(&buffer, "Original message");
    let (agent, _) = CommentFixture::text_position(&buffer, "Agent answer");
    let (followup, _) = CommentFixture::text_position(&buffer, "One more detail");
    assert!(user < agent && agent < followup);
}

#[test]
fn post_waits_for_durable_ack_and_failure_keeps_the_editor_text() {
    let mut fixture = CommentFixture::new();
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Keep this text".into()))
        .unwrap();
    let actions = fixture.key_without_ack(Key::ControlEnter);
    let [Action::Thread(review_threads::ThreadCommand::Post { review_unit, post })] =
        actions.as_slice()
    else {
        panic!("expected post")
    };
    assert!(fixture.book().threads().is_empty());
    assert!(fixture.component().comments.editing.is_some());
    assert!(fixture.key_without_ack(Key::ControlEnter).is_empty());
    fixture
        .registry
        .publish(ui_events::TextPasted("blocked while posting".into()))
        .unwrap();
    fixture
        .registry
        .publish(ui_events::ThreadPostFinished {
            review_unit: review_unit.clone(),
            message_id: post.message().id.clone(),
            result: Err("disk full".into()),
        })
        .unwrap();
    let editing = fixture.component().comments.editing.as_ref().unwrap();
    assert!(editing.posting.is_none());
    assert_eq!(editing.editor.text(), "Keep this text");
    fixture.key(Key::ControlEnter);
    assert_eq!(
        fixture.book().threads()[0].messages[0].text,
        "Keep this text"
    );
}

#[test]
fn switching_reviews_keeps_unposted_text_private_and_restores_it() {
    let mut fixture = CommentFixture::new();
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Still composing".into()))
        .unwrap();
    assert!(
        fixture
            .registry
            .publish(RepositoryFilesChanged {
                review_checkpoint: ReviewCheckpoint::new("other", "other-checkpoint"),
                files: vec![],
            })
            .unwrap()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .all(|a| !matches!(
                a,
                Action::Thread(review_threads::ThreadCommand::Post { .. })
            ))
    );
    assert!(fixture.component().comments.editing.is_none());
    publish_repository(&mut fixture.registry, "checkpoint");
    fixture
        .registry
        .publish(ui_events::ReviewThreadsLoaded {
            review_unit: "change".into(),
            result: Ok(ReviewThreads::new("change".into())),
        })
        .unwrap();
    assert!(fixture.book().threads().is_empty());
    assert_eq!(
        fixture
            .component()
            .comments
            .editing
            .as_ref()
            .unwrap()
            .editor
            .text(),
        "Still composing"
    );
}

#[test]
fn f2_switches_keymaps_through_comment_input_and_remembers_the_choice() {
    let mut fixture = CommentFixture::new();
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture.key(Key::EditorMode);
    fixture.key(Key::Escape);
    fixture.key(Key::Char('j'));
    let editor = &fixture
        .component()
        .comments
        .editing
        .as_ref()
        .unwrap()
        .editor;
    assert_eq!(editor.keymap(), comment_editor::EditorKeymap::Regular);
    assert_eq!(editor.text(), "j");
    fixture.key(Key::ControlEnter);
    fixture.key(Key::Char('a'));
    let editor = &fixture
        .component()
        .comments
        .editing
        .as_ref()
        .unwrap()
        .editor;
    assert_eq!(editor.keymap(), comment_editor::EditorKeymap::Regular);
    assert_eq!(editor.mode(), "Insert");
    fixture.key(Key::EditorMode);
    let editor = &fixture
        .component()
        .comments
        .editing
        .as_ref()
        .unwrap()
        .editor;
    assert_eq!(editor.keymap(), comment_editor::EditorKeymap::Vim);
    assert_eq!(editor.mode(), "Normal");
}

#[test]
fn the_reply_field_starts_a_new_message_even_before_the_agent_answers() {
    let mut fixture = CommentFixture::new();
    fixture.add("first draft");
    let original = fixture.book().threads()[0].messages[0].clone();

    fixture.click_text("Reply…");
    let editing = fixture.component().comments.editing.as_ref().unwrap();
    assert!(editing.posting.is_none());
    assert_eq!(editing.reply_to.as_ref(), Some(&original.id));
    assert!(editing.editor.text().is_empty());
    fixture
        .registry
        .publish(ui_events::TextPasted("One more detail".into()))
        .unwrap();
    fixture.key(Key::ControlEnter);

    assert_eq!(fixture.book().threads().len(), 1);
    assert_eq!(fixture.book().threads()[0].messages[0], original);
    assert_eq!(
        fixture.book().threads()[0].messages[1].text,
        "One more detail"
    );
}

#[test]
fn deleted_anchor_and_deleted_file_retain_accessible_threads() {
    let mut fixture = CommentFixture::new();
    fixture.add("Keep this discussion");
    fixture
        .registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".into(),
            rows: vec![],
            old_content: Some(Vec::new()),
            new_content: Some(Vec::new()),
        })
        .unwrap();
    fixture.key(Key::First);
    let rendered = rendered_diff(&fixture.registry, fixture.target);
    assert!(rendered.contains("original context"), "{rendered}");
    assert!(rendered.contains("changed"));
    fixture
        .registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "new"),
            files: vec![],
        })
        .unwrap();
    fixture.key(Key::Char(']'));
    fixture.key(Key::Char('c'));
    assert!(
        fixture
            .component()
            .selected_document()
            .unwrap()
            .comments_only
    );
    assert_eq!(
        fixture.book().threads()[0].messages[0].text,
        "Keep this discussion"
    );
    let rendered = fixture
        .render_thread()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Keep this discussion"));
}

#[test]
fn comments_follow_unchanged_code_when_lines_are_inserted_before_it() {
    let mut fixture = CommentFixture::new();
    fixture.add("sticky");
    fixture
        .registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".into(),
            rows: vec![
                DiffRow::Add {
                    new_line: 1,
                    text: "+before".into(),
                },
                DiffRow::Add {
                    new_line: 2,
                    text: "+changed".into(),
                },
            ],
            old_content: Some(Vec::new()),
            new_content: Some(b"before\nchanged\n".to_vec()),
        })
        .unwrap();
    let lines = rendered_diff_lines(&fixture.registry, fixture.target);
    let code = lines
        .iter()
        .position(|line| line.contains("changed"))
        .unwrap();
    assert!(lines[code + 3].contains("You"));
    assert!(lines[code + 5].contains("sticky"));
    assert!(!lines.join("\n").contains("outdated"));
}

#[test]
fn standalone_messages_match_the_inline_thread_cells() {
    let mut fixture = CommentFixture::new();
    fixture.add("**Original question**\n\n`inline code`");
    fixture.answer("The agent answer.");
    fixture.key(Key::First);
    let area = Rect::new(0, 0, 80, 60);
    let inline = fixture.render_in(area);
    fixture
        .registry
        .publish(ui_events::ReviewNavigationChanged(
            ui_events::ReviewNavigation::Threads,
        ))
        .unwrap();
    let thread_id = fixture.book().threads()[0].id.clone();
    fixture
        .registry
        .publish(ui_events::ThreadSelected { thread_id })
        .unwrap();
    let standalone = fixture.render_in(area);
    for text in [
        "You",
        "Original question",
        "inline code",
        "Agent",
        "The agent answer.",
        "Reply…",
    ] {
        let (inline_row, _) = CommentFixture::text_position(&inline, text);
        let (standalone_row, _) = CommentFixture::text_position(&standalone, text);
        for column in 5..78 {
            assert_eq!(
                inline[(column, inline_row)],
                standalone[(column, standalone_row)],
                "{text} at column {column}"
            );
        }
    }
}

#[test]
fn resolving_collapses_inline_history_and_preserves_a_reply_for_the_threads_tab() {
    let mut fixture = CommentFixture::new();
    fixture.add("Keep my history");
    fixture.answer("Full answer stays in history");
    assert!(fixture.key(Key::Char('d')).is_empty());
    assert!(fixture.key(Key::Char('c')).is_empty());
    assert_eq!(fixture.book().threads().len(), 1);
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 78,
            height: 50,
        })
        .unwrap();
    fixture.key(Key::Char('A'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Park this reply".into()))
        .unwrap();
    let buffer = fixture.render_in(Rect::new(0, 0, 80, 80));
    let (row, column) = CommentFixture::text_position(&buffer, "Resolve thread");
    let actions = fixture.click(row, column);
    let [
        Action::Thread(review_threads::ThreadCommand::SetResolution {
            thread_id,
            resolution,
            ..
        }),
    ] = actions.as_slice()
    else {
        panic!("{actions:?}")
    };
    let mut book = fixture.book().clone();
    book.set_resolution(thread_id, *resolution).unwrap();
    fixture
        .registry
        .publish(ui_events::ReviewThreadsLoaded {
            review_unit: book.review_unit.clone(),
            result: Ok(book),
        })
        .unwrap();
    let text = fixture
        .render_thread()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(text.contains("Resolved · Keep my history"));
    assert!(text.contains("Unresolve thread"));
    assert!(!text.contains("Full answer stays in history"));
    assert!(!text.contains("Park this reply"));
    assert!(
        fixture
            .render_thread()
            .content()
            .iter()
            .any(|cell| cell.bg == Theme::default().palette.cursor),
        "the hidden draft must not suppress the diff cursor"
    );
    assert!(fixture.key(Key::Control('s')).is_empty());
    fixture
        .registry
        .publish(ui_events::ReviewNavigationChanged(
            ui_events::ReviewNavigation::Threads,
        ))
        .unwrap();
    fixture
        .registry
        .publish(ui_events::ThreadSelected {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    assert_eq!(
        fixture
            .component()
            .comments
            .editing
            .as_ref()
            .unwrap()
            .editor
            .text(),
        "Park this reply"
    );
    assert_eq!(
        fixture.book().threads()[0].messages[0].text,
        "Keep my history"
    );
}

#[test]
fn comments_share_an_anchor_frame_but_keep_independent_messages() {
    let mut fixture = CommentFixture::new();
    fixture.add("first");
    fixture.add("second");
    fixture.key(Key::First);
    let area = Rect::new(0, 0, 80, 40);
    let mut buffer = Buffer::empty(area);
    fixture
        .component()
        .render(area, &mut buffer, Theme::default().palette, true, None)
        .render(&mut buffer);
    let rendered = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("first"));
    assert!(rendered.contains("second"));
    let border_column = fixture
        .component()
        .selected_document()
        .unwrap()
        .document
        .diff
        .line_number_width()
        + 3;
    assert_eq!(
        buffer
            .content()
            .chunks(80)
            .filter(|row| row[border_column].symbol() == "╭")
            .count(),
        1
    );
    assert_eq!(
        buffer
            .content()
            .chunks(80)
            .filter(|row| row[border_column].symbol() == "╰")
            .count(),
        1
    );
    assert_eq!(fixture.book().threads().len(), 2);
    assert_ne!(
        fixture.book().threads()[0].messages[0].id,
        fixture.book().threads()[1].messages[0].id
    );
}

#[test]
fn first_thread_frame_wraps_to_its_area_before_a_resize_event() {
    let mut fixture = CommentFixture::new();
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 300,
            height: 100,
        })
        .unwrap();
    fixture.add("Check the initial layout");
    let words = (0..90).map(|i| format!("word{i:03}")).collect::<Vec<_>>();
    fixture.answer(&words.join(" "));
    let area = Rect::new(0, 0, 110, 100);
    let initial = fixture.render_in(area);
    let rendered = initial
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    for word in words {
        assert!(rendered.contains(&word), "missing {word}");
    }
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 108,
            height: 98,
        })
        .unwrap();
    assert_eq!(initial, fixture.render_in(area));
}

#[test]
fn inline_replies_are_read_only_after_their_whole_body_has_been_displayed() {
    let mut fixture = CommentFixture::new();
    fixture.add("Question");
    fixture.answer("First paragraph.\n\nSecond paragraph.\n\nLast paragraph.");
    for height in [8, 60] {
        fixture.component().begin_reply_frame();
        let buffer = fixture.render_in(Rect::new(0, 0, 80, height));
        fixture.component().capture_reply_frame(&buffer);
        fixture.component().finish_reply_frame(&buffer);
        let actions = fixture
            .registry
            .publish(ui_events::FrameRendered)
            .unwrap()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect::<Vec<_>>();
        fixture.accept(&actions);
        assert_eq!(fixture.book().counts().unread, usize::from(height == 8));
    }
}
