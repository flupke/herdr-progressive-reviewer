use std::path::Path;

use super::editor_command;

#[test]
fn editor_command_passes_one_based_line_and_path_to_the_configured_editor() {
    let command = editor_command("nvim -p".into(), Path::new("/repo/src/lib.rs"), Some(9));

    assert_eq!(command.get_program(), "sh");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["-c", r#"nvim -p "$@""#, "editor", "+10", "/repo/src/lib.rs"]
    );
}

#[test]
fn editor_command_omits_line_when_the_cursor_has_no_new_line() {
    let command = editor_command("vi".into(), Path::new("gone.rs"), None);

    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["-c", r#"vi "$@""#, "editor", "gone.rs"]
    );
}
