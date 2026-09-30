use component_core::InputResolution;

use super::*;
use crate::table::ShortcutBinding;

/// How many owning scopes select `command`.
fn owner_count(command: ShortcutCommand) -> usize {
    [
        ApplicationShortcut::select(command).is_some(),
        SearchShortcut::select(command).is_some(),
        MovementShortcut::select(command).is_some(),
        DiffShortcut::select(command).is_some(),
        DiffGlobalShortcut::select(command).is_some(),
        FilesShortcut::select(command).is_some(),
        OverlayShortcut::select(command).is_some(),
        RevisionShortcut::select(command).is_some(),
        ThreadsShortcut::select(command).is_some(),
        ConversationShortcut::select(command).is_some(),
        ExploreShortcut::select(command).is_some(),
        ExploreGlobalShortcut::select(command).is_some(),
    ]
    .into_iter()
    .filter(|owned| *owned)
    .count()
}

fn resolve<S: ShortcutSubscription>(sequence: ShortcutSequence) -> InputResolution<S> {
    let mut matcher = ShortcutMatcher::<S>::new();
    match sequence {
        ShortcutSequence::One(key) => matcher.resolve_key(key),
        ShortcutSequence::Two(first, second) => {
            assert!(
                matches!(
                    matcher.resolve_key(first),
                    InputResolution::AwaitingMoreInput
                ),
                "{first:?} must start a sequence"
            );
            matcher.resolve_key(second)
        }
    }
}

fn subscribed_bindings<S: ShortcutSubscription>()
-> impl Iterator<Item = (&'static ShortcutBinding, S)> {
    bindings().filter_map(|binding| S::select(binding.command).map(|command| (binding, command)))
}

fn conflicts(first: ShortcutSequence, second: ShortcutSequence) -> bool {
    match (first, second) {
        (ShortcutSequence::One(key), sequence) | (sequence, ShortcutSequence::One(key)) => {
            sequence == ShortcutSequence::One(key) || sequence.starts_with(key)
        }
        (first, second) => first == second,
    }
}

/// Every binding a subscription sees resolves to its command, and no two of
/// those bindings claim the same keys. A key listed under two help lines for
/// the same command is one binding shown twice, not a conflict.
fn assert_subscription_is_consistent<S>()
where
    S: ShortcutSubscription + std::fmt::Debug + PartialEq,
{
    let bindings = subscribed_bindings::<S>().collect::<Vec<_>>();
    for (index, (binding, command)) in bindings.iter().enumerate() {
        assert_eq!(
            resolve::<S>(binding.sequence),
            InputResolution::Matched(*command)
        );
        for (other, _) in &bindings[index + 1..] {
            let repeated = binding.sequence == other.sequence && binding.command == other.command;
            assert!(
                repeated || !conflicts(binding.sequence, other.sequence),
                "{:?} and {:?} claim the same keys in one subscription",
                binding.command,
                other.command
            );
        }
    }
}

#[test]
fn every_command_has_exactly_one_owner() {
    for binding in bindings() {
        assert_eq!(owner_count(binding.command), 1, "{:?}", binding.command);
    }
}

#[test]
fn owning_scopes_resolve_their_bindings_without_conflicts() {
    assert_subscription_is_consistent::<ApplicationShortcut>();
    assert_subscription_is_consistent::<SearchShortcut>();
    assert_subscription_is_consistent::<MovementShortcut>();
    assert_subscription_is_consistent::<DiffShortcut>();
    assert_subscription_is_consistent::<DiffGlobalShortcut>();
    assert_subscription_is_consistent::<FilesShortcut>();
    assert_subscription_is_consistent::<OverlayShortcut>();
    assert_subscription_is_consistent::<RevisionShortcut>();
    assert_subscription_is_consistent::<ThreadsShortcut>();
    assert_subscription_is_consistent::<ConversationShortcut>();
    assert_subscription_is_consistent::<ExploreShortcut>();
    assert_subscription_is_consistent::<ExploreGlobalShortcut>();
}

#[test]
fn combined_subscriptions_resolve_their_bindings_without_conflicts() {
    assert_subscription_is_consistent::<ApplicationCommand>();
    assert_subscription_is_consistent::<DiffPaneCommand>();
    assert_subscription_is_consistent::<ThreadsCommand>();
    assert_subscription_is_consistent::<ConversationCommand>();
    assert_subscription_is_consistent::<ExploreCommand>();
}

/// The diff, the Threads list and the conversation view resolve movement
/// from the same table entries, so rebinding one moves all three.
#[test]
fn list_and_conversation_views_share_the_diff_movement_bindings() {
    for (binding, movement) in subscribed_bindings::<MovementShortcut>() {
        assert_eq!(
            resolve::<DiffPaneCommand>(binding.sequence),
            InputResolution::Matched(DiffPaneCommand::Movement(movement))
        );
        assert_eq!(
            resolve::<ThreadsCommand>(binding.sequence),
            InputResolution::Matched(ThreadsCommand::Movement(movement))
        );
        assert_eq!(
            resolve::<ConversationCommand>(binding.sequence),
            InputResolution::Matched(ConversationCommand::Movement(movement))
        );
    }
}

#[test]
fn threads_and_conversation_help_comes_from_the_table() {
    let lines = help_lines().collect::<Vec<_>>();
    assert!(lines.contains(&("1 / 2".to_owned(), "Threads: show Unresolved / All")));
    assert!(lines.contains(&(
        "Esc".to_owned(),
        "Conversation: close the peek, or return to Files"
    )));
}

#[test]
fn explore_help_comes_from_the_table() {
    let lines = help_lines().collect::<Vec<_>>();
    for line in [
        (
            "Up / Down / j / k",
            "Explore: select an answer, including None of the above",
        ),
        (
            "Enter",
            "Explore: send the selected answer with any additional text",
        ),
        (
            "Tab",
            "Explore: focus conversation, inline evidence, then answer",
        ),
        (
            "Alt-j / Alt-k",
            "Explore: grow / shrink the evidence window",
        ),
        (
            "Alt-0",
            "Explore: fit evidence to its wrapped relevant range",
        ),
        ("[ / ]", "Explore: visit previous / next interview turn"),
    ] {
        assert!(
            lines.contains(&(line.0.to_owned(), line.1)),
            "missing help line {line:?}"
        );
    }
}

/// Explore's single-key commands win over longer sequences other scopes
/// start with the same key, so `g` and `[` act at once in Explore.
#[test]
fn explore_single_keys_do_not_wait_for_sequences() {
    let mut explore = ShortcutMatcher::<ExploreCommand>::new();
    assert_eq!(
        explore.resolve_key(Key::Char('g')),
        InputResolution::Matched(ExploreCommand::Explore(ExploreShortcut::Coverage(
            ExploreCoverageShortcut::ToggleOverview
        )))
    );
    assert_eq!(
        explore.resolve_key(Key::Char('[')),
        InputResolution::Matched(ExploreCommand::Explore(ExploreShortcut::Turn(
            ExploreTurnShortcut::PreviousTurn
        )))
    );
    assert_eq!(
        explore.resolve_key(Key::Tab),
        InputResolution::Matched(ExploreCommand::Global(ExploreGlobalShortcut::CycleFocus))
    );
    assert_eq!(
        explore.resolve_key(Key::Char('f')),
        InputResolution::NoMatch
    );
}

#[test]
fn a_subscription_ignores_bindings_owned_by_other_scopes() {
    let mut revision = ShortcutMatcher::<RevisionShortcut>::new();
    assert_eq!(
        revision.resolve_key(Key::Char('j')),
        InputResolution::NoMatch
    );
    assert_eq!(
        revision.resolve_key(Key::Char('g')),
        InputResolution::NoMatch
    );

    let mut movement = ShortcutMatcher::<MovementShortcut>::new();
    assert_eq!(
        movement.resolve_key(Key::Char('[')),
        InputResolution::NoMatch
    );
}

#[test]
fn a_combined_subscription_resolves_each_scope_it_listens_to() {
    let mut application = ShortcutMatcher::<ApplicationCommand>::new();
    assert_eq!(
        application.resolve_key(Key::Char('/')),
        InputResolution::Matched(ApplicationCommand::Search(SearchShortcut::Begin))
    );
    assert_eq!(
        application.resolve_key(Key::Char('q')),
        InputResolution::Matched(ApplicationCommand::Application(ApplicationShortcut::Quit))
    );
    assert_eq!(
        application.resolve_key(Key::Char('n')),
        InputResolution::NoMatch
    );
}

#[test]
fn unknown_keys_and_incomplete_sequences_do_not_run_commands() {
    let mut matcher = ShortcutMatcher::<DiffPaneCommand>::new();
    assert_eq!(
        matcher.resolve_key(Key::Char('x')),
        InputResolution::NoMatch
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('g')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('x')),
        InputResolution::NoMatch
    );
}

#[test]
fn shortcut_matcher_resolves_a_sequence_for_its_subscription() {
    let mut matcher = ShortcutMatcher::<DiffGlobalShortcut>::new();

    assert_eq!(
        matcher.resolve_key(Key::Char(']')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('h')),
        InputResolution::Matched(DiffGlobalShortcut::Hunk(HunkShortcut::GoToNextModified))
    );
}

#[test]
fn shortcut_matcher_retries_a_failed_sequence_as_new_input() {
    let mut matcher = ShortcutMatcher::<DiffGlobalShortcut>::new();

    assert_eq!(
        matcher.resolve_key(Key::Char('[')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char(']')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('h')),
        InputResolution::Matched(DiffGlobalShortcut::Hunk(HunkShortcut::GoToNextModified))
    );
}

#[test]
fn unreviewed_file_navigation_help_uses_file_shortcuts() {
    let file_navigation = SHORTCUTS
        .iter()
        .find(|definition| definition.description == Some("Go to previous / next unreviewed file"))
        .and_then(ShortcutDefinition::help_line);

    assert_eq!(
        file_navigation,
        Some((
            "[f / ]f".to_owned(),
            "Go to previous / next unreviewed file"
        ))
    );
}

#[test]
fn modified_hunk_navigation_help_uses_hunk_shortcuts() {
    let hunk_navigation = SHORTCUTS
        .iter()
        .find(|definition| definition.description == Some("Go to previous / next modified hunk"))
        .and_then(ShortcutDefinition::help_line);

    assert_eq!(
        hunk_navigation,
        Some(("[h / ]h".to_owned(), "Go to previous / next modified hunk"))
    );
}

#[test]
fn help_close_label_is_generated_from_close_bindings() {
    assert_eq!(help_close_label(), "? or Esc");
    assert!(closes_help(Key::Char('?')));
    assert!(closes_help(Key::Escape));
    assert!(!closes_help(Key::Char('q')));
}

#[test]
fn every_help_line_names_its_keys() {
    for (keys, description) in help_lines() {
        assert!(!keys.is_empty(), "{description:?} needs a visible shortcut");
    }
    for line in [
        ("V", "Select diff lines"),
        ("l", "Expand an unchanged section"),
        ("Space", "Mark a file as reviewed"),
        ("c", "Show the commit message"),
        ("q", "Quit"),
    ] {
        assert!(
            help_lines().any(|(keys, description)| (keys.as_str(), description) == line),
            "{line:?}"
        );
    }
}

/// Every single-key binding a subscription resolves is found by a key
/// lookup and listed among its command's keys.
fn assert_single_keys_are_queryable<S>()
where
    S: ShortcutSubscription + std::fmt::Debug,
{
    for (binding, command) in subscribed_bindings::<S>() {
        let ShortcutSequence::One(key) = binding.sequence else {
            continue;
        };
        assert_eq!(S::bound_to(key), Some(command), "{key:?}");
        assert!(command.keys().any(|bound| bound == key), "{command:?}");
    }
}

#[test]
fn single_key_bindings_are_queryable_by_key_and_by_command() {
    assert_single_keys_are_queryable::<ApplicationShortcut>();
    assert_single_keys_are_queryable::<OverlayShortcut>();
    assert_single_keys_are_queryable::<ExploreCommand>();
}

#[test]
fn key_queries_ignore_sequences_and_other_scopes() {
    assert_eq!(RevisionShortcut::bound_to(Key::Char('v')), None);
    assert_eq!(ApplicationShortcut::bound_to(Key::Char('?')), None);
    assert_eq!(
        OverlayShortcut::bound_to(Key::Char('?')),
        Some(OverlayShortcut::OpenHelp)
    );
    assert_eq!(RevisionShortcut::OpenSelector.keys().count(), 0);
}

#[test]
fn a_command_lists_its_aliases_among_its_keys() {
    assert_eq!(
        ApplicationShortcut::OpenFiles.keys().collect::<Vec<_>>(),
        [Key::Char('f'), Key::Char('F')]
    );
}
