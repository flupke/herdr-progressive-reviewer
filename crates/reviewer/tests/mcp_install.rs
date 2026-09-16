use std::fs;
use std::process::Command;

use test_case::test_case;

#[test_case(false; "default_user_paths")]
#[test_case(true; "custom_user_paths")]
fn installation_registers_selected_clients_outside_a_repository(custom_paths: bool) {
    let directory = tempfile::tempdir().unwrap();
    let working_directory = directory.path().join("working-directory");
    let user_home = directory.path().join("home");
    fs::create_dir(&working_directory).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_reviewer-control"));
    command
        .arg("mcp-install")
        .current_dir(&working_directory)
        .env("HOME", &user_home)
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR");
    let (codex, claude) = if custom_paths {
        let codex = directory.path().join("codex");
        let claude = directory.path().join("claude");
        command
            .env("CODEX_HOME", &codex)
            .env("CLAUDE_CONFIG_DIR", &claude);
        (codex.join("config.toml"), claude.join(".claude.json"))
    } else {
        (
            user_home.join(".codex/config.toml"),
            user_home.join(".claude.json"),
        )
    };
    let mut selected = Command::new(command.get_program());
    selected
        .args(command.get_args())
        .arg("codex")
        .current_dir(&working_directory);
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            selected.env(key, value);
        } else {
            selected.env_remove(key);
        }
    }
    let output = selected.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !claude.exists(),
        "a selected installation must leave the other client alone"
    );
    let original_codex = fs::read_to_string(&codex).unwrap();
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original_claude = fs::read_to_string(&claude).unwrap();
    assert!(original_claude.contains("herdr_reviewer"));
    assert!(original_codex.contains("herdr_reviewer"));
    assert!(command.output().unwrap().status.success());
    assert_eq!(fs::read_to_string(codex).unwrap(), original_codex);
    assert_eq!(fs::read_to_string(claude).unwrap(), original_claude);
    assert_eq!(fs::read_dir(working_directory).unwrap().count(), 0);
}
