use super::*;

fn one_of_each_variant() -> impl Iterator<Item = Key> {
    [Key::Char('x'), Key::Control('t'), Key::Alt('u')]
        .into_iter()
        .chain(NAMED_KEYS.iter().map(|named| named.key))
}

fn event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn every_key_variant_comes_from_a_terminal_event() {
    for key in one_of_each_variant() {
        assert_eq!(Key::from_terminal(key.to_terminal()), Some(key));
    }
}

#[test]
fn each_named_key_has_its_own_terminal_event() {
    for (index, named) in NAMED_KEYS.iter().enumerate() {
        for other in &NAMED_KEYS[index + 1..] {
            assert_ne!(named.key, other.key);
            assert_ne!(
                (named.code, named.modifiers),
                (other.code, other.modifiers),
                "{:?} and {:?}",
                named.key,
                other.key
            );
        }
    }
}

#[test]
fn characters_keep_their_case_and_space_stays_a_character() {
    assert_eq!(
        Key::from_terminal(event(KeyCode::Char('V'), KeyModifiers::SHIFT)),
        Some(Key::Char('V'))
    );
    assert_eq!(
        Key::from_terminal(event(KeyCode::Char(' '), KeyModifiers::NONE)),
        Some(Key::Char(' '))
    );
    assert_eq!(Key::Char(' ').label(), "Space");
}

#[test]
fn control_characters_name_their_location_and_page_commands() {
    for (character, key) in [
        ('d', Key::HalfPageDown),
        ('u', Key::HalfPageUp),
        ('o', Key::PreviousLocation),
        ('i', Key::NextLocation),
        ('t', Key::Control('t')),
    ] {
        assert_eq!(
            Key::from_terminal(event(KeyCode::Char(character), KeyModifiers::CONTROL)),
            Some(key)
        );
    }
    assert_eq!(
        Key::from_terminal(event(KeyCode::Enter, KeyModifiers::CONTROL)),
        Some(Key::ControlEnter)
    );
    assert_eq!(
        Key::from_terminal(event(KeyCode::Left, KeyModifiers::CONTROL)),
        None
    );
}

#[test]
fn other_modifiers_do_not_change_named_keys() {
    for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
        assert_eq!(
            Key::from_terminal(event(KeyCode::Left, modifiers)),
            Some(Key::Left)
        );
    }
    assert_eq!(
        Key::from_terminal(event(KeyCode::Char('u'), KeyModifiers::ALT)),
        Some(Key::Alt('u'))
    );
}

#[test]
fn key_releases_and_unmapped_keys_are_ignored() {
    let mut release = event(KeyCode::Char('q'), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(Key::from_terminal(release), None);
    assert_eq!(
        Key::from_terminal(event(KeyCode::F(5), KeyModifiers::NONE)),
        None
    );
}
