use super::*;

#[test]
fn every_documented_binding_resolves_to_its_command() {
    for definition in SHORTCUTS {
        for binding in definition.bindings {
            let lookup = match binding.sequence {
                ShortcutSequence::One(key) => lookup(None, key),
                ShortcutSequence::Two(first, second) => {
                    let ShortcutLookup::Prefix(prefix) = lookup(None, first) else {
                        panic!("{first:?} did not start a shortcut sequence");
                    };
                    lookup(Some(prefix), second)
                }
            };
            assert_eq!(lookup, ShortcutLookup::Command(binding.command));
        }
    }
}

#[test]
fn unknown_keys_and_incomplete_sequences_do_not_run_commands() {
    assert_eq!(lookup(None, Key::Char('x')), ShortcutLookup::None);
    let ShortcutLookup::Prefix(go_to_prefix) = lookup(None, Key::Char('g')) else {
        panic!("g did not start a shortcut sequence");
    };
    assert_eq!(
        lookup(Some(go_to_prefix), Key::Char('x')),
        ShortcutLookup::None
    );
}

#[test]
fn shortcut_matcher_resolves_a_sequence_for_its_subscription() {
    let mut matcher = ShortcutMatcher::new(ShortcutSet::Guide);

    assert_eq!(
        matcher.resolve_key(Key::Char('r')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('f')),
        InputResolution::Matched(ShortcutCommand::Guide(GuideShortcut::GenerateSelectedFile))
    );
}

#[test]
fn shortcut_matcher_retries_a_failed_sequence_as_new_input() {
    let mut matcher = ShortcutMatcher::new(ShortcutSet::Guide);

    assert_eq!(
        matcher.resolve_key(Key::Char('r')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char(']')),
        InputResolution::AwaitingMoreInput
    );
    assert_eq!(
        matcher.resolve_key(Key::Char('r')),
        InputResolution::Matched(ShortcutCommand::Guide(GuideShortcut::GoToNextComment))
    );
}

#[test]
fn help_labels_are_generated_from_visible_bindings() {
    let full_guide = SHORTCUTS
        .iter()
        .find(|definition| definition.description == Some("Generate a file / full review guide"))
        .and_then(ShortcutDefinition::help_line);

    assert_eq!(
        full_guide,
        Some(("rf / ra".to_owned(), "Generate a file / full review guide"))
    );
}

#[test]
fn help_close_label_is_generated_from_close_bindings() {
    assert_eq!(help_close_label(), "? or Esc");
    assert!(closes_help(Key::Char('?')));
    assert!(closes_help(Key::Escape));
    assert!(!closes_help(Key::Char('q')));
}
