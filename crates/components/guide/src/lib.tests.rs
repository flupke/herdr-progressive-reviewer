use component_core::{Component, ComponentEventBus, ComponentSubscriptions, EventEnvelope};
use review_guide::{GuideItemStatus, GuideLineRange, GuideTarget, ReviewCheckpoint};
use review_types::ReviewUnit;
use ui_events::DisplayedDiffRow;
use ui_shortcuts::Key;

use super::*;

#[test]
fn guide_shortcut_emits_generation_action() {
    let mut registry = ComponentEventBus::new();
    registry.mount(GuideComponent::new);
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "src/lib.rs".to_owned(),
        }))
        .unwrap();

    let prefix = registry
        .dispatch_global_input(&EventEnvelope::new(Key::Char('r')))
        .unwrap();
    assert!(prefix.into_results().is_empty());
    let result = registry
        .dispatch_global_input(&EventEnvelope::new(Key::Char('f')))
        .unwrap();
    let actions = result
        .into_results()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec![Action::GenerateReviewGuide {
            scope: GuideScope::File {
                path: "src/lib.rs".to_owned(),
            },
        }]
    );
}

#[test]
fn stale_guide_results_do_not_replace_the_visible_guide() {
    let mut registry = ComponentEventBus::new();
    registry.mount(GuideComponent::new);
    registry.mount(|_| PathObserver);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            files: Vec::new(),
        }))
        .unwrap();
    let current_item = GuideItem {
        target: GuideTarget::File {
            path: "current.rs".to_owned(),
        },
        text: "Current".to_owned(),
        status: GuideItemStatus::Matched,
    };
    let current_results = registry
        .publish_envelope(EventEnvelope::new(ReviewGuideChanged {
            review_checkpoint: ReviewCheckpoint {
                review_unit: ReviewUnit::from("change"),
                checkpoint: "current".to_owned(),
            },
            items: vec![current_item],
        }))
        .unwrap();
    assert_eq!(output_texts(current_results), vec!["current.rs"]);

    let stale_results = registry
        .publish_envelope(EventEnvelope::new(ReviewGuideChanged {
            review_checkpoint: ReviewCheckpoint {
                review_unit: ReviewUnit::from("change"),
                checkpoint: "stale".to_owned(),
            },
            items: vec![GuideItem {
                target: GuideTarget::File {
                    path: "stale.rs".to_owned(),
                },
                text: "Stale".to_owned(),
                status: GuideItemStatus::Matched,
            }],
        }))
        .unwrap();

    assert!(output_texts(stale_results).is_empty());
}

#[test]
fn generation_state_blocks_another_generation_request() {
    let mut registry = ComponentEventBus::new();
    registry.mount(GuideComponent::new);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            files: Vec::new(),
        }))
        .unwrap();
    registry
        .publish_envelope(EventEnvelope::new(ReviewGuideStatusChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            generating: true,
            message: None,
        }))
        .unwrap();

    assert!(shortcut_actions(&mut registry, Key::Char('r'), Key::Char('a')).is_empty());

    registry
        .publish_envelope(EventEnvelope::new(ReviewGuideStatusChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            generating: false,
            message: None,
        }))
        .unwrap();
    assert_eq!(
        shortcut_actions(&mut registry, Key::Char('r'), Key::Char('a')),
        vec![Action::GenerateReviewGuide {
            scope: GuideScope::All,
        }]
    );
}

#[test]
fn navigation_uses_diff_positions_and_wraps() {
    let mut registry = ComponentEventBus::new();
    registry.mount(GuideComponent::new);
    registry.mount(|_| PathObserver);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            files: Vec::new(),
        }))
        .unwrap();
    registry
        .publish_envelope(EventEnvelope::new(ReviewGuideChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "current"),
            items: vec![
                guide_item("first.rs", 3),
                guide_item("first.rs", 9),
                guide_item("second.rs", 1),
            ],
        }))
        .unwrap();
    registry
        .publish_envelope(EventEnvelope::new(DisplayedDiffViewportsChanged {
            viewports: vec![
                displayed_viewport("first.rs", 0, 9, &[(2, 3), (8, 9)]),
                displayed_viewport("second.rs", 1, 1, &[(0, 1)]),
            ],
            current_file_index: 0,
            current_row: 2,
        }))
        .unwrap();

    let next = shortcut_results(&mut registry, Key::Char(']'), Key::Char('r'));
    assert_eq!(output_texts(next), vec!["first.rs:8"]);

    registry
        .publish_envelope(EventEnvelope::new(DisplayedDiffViewportsChanged {
            viewports: vec![
                displayed_viewport("first.rs", 0, 9, &[(2, 3), (8, 9)]),
                displayed_viewport("second.rs", 1, 1, &[(0, 1)]),
            ],
            current_file_index: 0,
            current_row: 0,
        }))
        .unwrap();
    let previous = shortcut_results(&mut registry, Key::Char('['), Key::Char('r'));
    assert_eq!(output_texts(previous), vec!["second.rs:0"]);
}

struct PathObserver;

impl PathObserver {
    #[allow(clippy::unused_self)]
    fn paths_changed(&mut self, event: &GuidePathsChanged) -> Vec<Action> {
        vec![Action::Output {
            text: event.paths.join(","),
        }]
    }

    #[allow(clippy::unused_self)]
    fn jump_requested(&mut self, event: &GuideJumpRequested) -> Vec<Action> {
        vec![Action::Output {
            text: format!(
                "{}:{}",
                event.target.path(),
                event
                    .row
                    .map_or_else(|| "unloaded".to_owned(), |row| row.to_string())
            ),
        }]
    }
}

impl Component<Action> for PathObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::paths_changed);
        subscriptions.subscribe(Self::jump_requested);
    }
}

fn shortcut_results(
    registry: &mut ComponentEventBus<Action>,
    prefix: Key,
    key: Key,
) -> Vec<component_core::DispatchResult<Action>> {
    let prefix_result = registry
        .dispatch_global_input(&EventEnvelope::new(prefix))
        .unwrap();
    assert!(prefix_result.into_results().is_empty());
    registry
        .dispatch_global_input(&EventEnvelope::new(key))
        .unwrap()
        .into_results()
}

fn shortcut_actions(
    registry: &mut ComponentEventBus<Action>,
    prefix: Key,
    key: Key,
) -> Vec<Action> {
    shortcut_results(registry, prefix, key)
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect()
}

fn guide_item(path: &str, line: u32) -> GuideItem {
    GuideItem {
        target: GuideTarget::Lines {
            path: path.to_owned(),
            old: None,
            new: Some(GuideLineRange {
                first_line: line,
                last_line: line,
            }),
        },
        text: "Comment".to_owned(),
        status: GuideItemStatus::Matched,
    }
}

fn displayed_viewport(
    path: &str,
    file_index: usize,
    row_count: usize,
    lines: &[(usize, u32)],
) -> DisplayedDiffViewport {
    let mut rows = vec![DisplayedDiffRow::default(); row_count];
    for (row, line) in lines {
        rows[*row].new_line = Some(*line);
    }
    DisplayedDiffViewport {
        path: path.to_owned(),
        file_index,
        rows,
        can_show_file: true,
    }
}

fn output_texts(results: Vec<component_core::DispatchResult<Action>>) -> Vec<String> {
    results
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .filter_map(|action| match action {
            Action::Output { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}
