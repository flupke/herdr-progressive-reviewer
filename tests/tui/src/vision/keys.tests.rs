use super::*;

#[test]
fn herdr_encodes_the_keys_it_names() {
    for name in ["enter", "ctrl+enter", "Escape", "shift+tab", "?", "1"] {
        assert_eq!(KeyInput::parse(name), KeyInput::Herdr(name.into()));
    }
}

#[test]
fn navigation_keys_are_encoded_here_in_any_spelling() {
    for name in ["PageDown", "pagedown", "page_down", "page-down"] {
        assert_eq!(KeyInput::parse(name), KeyInput::Bytes("\x1b[6~".into()));
    }
    assert_eq!(KeyInput::parse("Home"), KeyInput::Bytes("\x1b[H".into()));
    assert_eq!(KeyInput::parse("delete"), KeyInput::Bytes("\x1b[3~".into()));
}

#[test]
fn modifiers_go_into_the_sequence() {
    assert_eq!(
        KeyInput::parse("ctrl+PageUp"),
        KeyInput::Bytes("\x1b[5;5~".into())
    );
    assert_eq!(
        KeyInput::parse("shift+end"),
        KeyInput::Bytes("\x1b[1;2F".into())
    );
}

#[test]
fn an_unknown_modifier_is_left_for_herdr_to_refuse() {
    assert_eq!(
        KeyInput::parse("hyper+home"),
        KeyInput::Herdr("hyper+home".into())
    );
}
