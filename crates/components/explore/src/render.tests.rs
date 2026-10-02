use std::sync::Arc;

use component_core::ComponentEventBus;
use ratatui::layout::Rect;
use review_explore::{
    CodeLocation, Comparison, Exploration, ReopenedLines, ReviewerAnswer, SourceSide, TurnMarks,
};
use review_repository::repository::RepoPath;
use review_source::{ReviewCheckpoint, SourceLineRange};
use review_types::MarkAuthor;
use ui_actions::Action;

use crate::ExploreComponent;
use crate::flow::{Content, ConversationLayout};

fn at(side: SourceSide, first_line: u32, last_line: u32) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(b"src/lib.rs"),
        side,
        lines: Some(SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

fn answer() -> ReviewerAnswer {
    ReviewerAnswer {
        id: "answer".into(),
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        question: None,
        in_reply_to: "turn".into(),
        option: None,
        text: "Keep it.".into(),
        author: "reviewer".into(),
    }
}

/// An agent turn for `request`, following `answer` when it has one.
fn turn(request: &str, answer: Option<&str>) -> review_explore::ConversationTurn {
    review_explore::ConversationTurn {
        answer: answer.map(str::to_owned),
        update: serde_json::from_value(serde_json::json!({
            "instance": "round", "request": request,
            "checkpoint": {"review_unit": "review", "checkpoint": "checkpoint"},
            "interpretation": null, "topics": [], "next": null, "conclusion": null,
            "limitations": [], "findings": []
        }))
        .unwrap(),
    }
}

/// The text of each laid-out row: control labels and text lines.
fn rows(layout: &ConversationLayout) -> Vec<String> {
    layout
        .items
        .iter()
        .flat_map(|item| match &item.content {
            Content::Controls(buttons) => vec![
                buttons
                    .iter()
                    .map(|button| button.text.clone())
                    .collect::<String>(),
            ],
            Content::Text(text, _) => text.lines.iter().map(ToString::to_string).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn an_answers_marks_show_below_it_and_expand_to_their_lines() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|events| {
        ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default())
    });
    let component = bus.get_mut::<ExploreComponent>(target).unwrap();
    let mut exploration = Exploration::new(Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: "/tmp".into(),
        files: vec![],
        diffs: vec![],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    }));
    exploration.answers.push(answer());
    exploration.conversation.push(turn("kickoff", None));
    exploration
        .conversation
        .push(turn("request", Some("answer")));
    component.exploration = Some(exploration);
    component.marks.insert(
        "kickoff".into(),
        TurnMarks {
            not_relevant: vec![at(SourceSide::New, 20, 29)],
            ..TurnMarks::default()
        },
    );
    component.marks.insert(
        "request".into(),
        TurnMarks {
            answer: Some("answer".into()),
            reviewed: vec![at(SourceSide::New, 3, 5), at(SourceSide::Old, 2, 2)],
            not_relevant: vec![at(SourceSide::New, 40, 41)],
            reopened: vec![ReopenedLines {
                location: at(SourceSide::New, 9, 9),
                author: MarkAuthor::Jev,
            }],
            problem: None,
        },
    );
    let palette = ui_theme::Theme::default().palette;
    let layout = |component: &ExploreComponent| {
        let mut layout = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        component.answer_marks(&answer(), &mut layout, palette);
        rows(&layout)
    };

    assert_eq!(
        layout(component),
        ["[▸ Marked 4 lines reviewed · 2 lines not relevant · reopened 1 line]"]
    );

    component.toggle_marks(1);

    assert_eq!(
        layout(component),
        [
            "[▾ Marked 4 lines reviewed · 2 lines not relevant · reopened 1 line]",
            "  ✓ src/lib.rs new 3-5",
            "  ✓ src/lib.rs old 2",
            "  – src/lib.rs new 40-41 (not relevant)",
            "  ↺ src/lib.rs new 9",
        ]
    );

    // The kickoff follows no answer: its marks close the page of what it asked.
    let mut kickoff = ConversationLayout::new(Rect::new(0, 0, 80, 40));
    component.opening_marks(0, &mut kickoff, palette);
    assert_eq!(rows(&kickoff), ["[▸ Marked 10 lines not relevant]"]);
}

#[test]
fn a_summary_names_only_what_changed() {
    let counts = |reviewed_lines, reviewed_files, reopened_lines| review_explore::MarkCounts {
        reviewed_lines,
        reviewed_files,
        reopened_lines,
        ..review_explore::MarkCounts::default()
    };

    assert_eq!(super::marks_summary(counts(0, 0, 0)), "");
    assert_eq!(super::marks_summary(counts(0, 0, 2)), "Reopened 2 lines");
    assert_eq!(
        super::marks_summary(counts(3, 1, 0)),
        "Marked 3 lines and 1 whole file reviewed"
    );
    let not_relevant = review_explore::MarkCounts {
        not_relevant_lines: 5,
        not_relevant_files: 1,
        ..counts(0, 0, 2)
    };
    assert_eq!(
        super::marks_summary(not_relevant),
        "Marked 5 lines and 1 whole file not relevant · reopened 2 lines"
    );
}
