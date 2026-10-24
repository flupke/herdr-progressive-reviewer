use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;

use super::*;

/// The command of the hook in the marketplace written in `directory`.
fn written_command(directory: &Path) -> String {
    let hooks: Value =
        serde_json::from_slice(&fs::read(directory.join("plugin/hooks/hooks.json")).unwrap())
            .unwrap();
    hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn the_written_hook_runs_the_checkout_s_program_whatever_its_path() {
    let checkout = tempfile::tempdir().unwrap();
    let bin = checkout.path().join("it's a bin");
    fs::create_dir(&bin).unwrap();
    let control = bin.join("reviewer-control");
    let ran = checkout.path().join("ran");
    fs::write(
        &control,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", ran.display()),
    )
    .unwrap();
    fs::set_permissions(&control, fs::Permissions::from_mode(0o755)).unwrap();
    let marketplace = marketplace_of(&control);
    fs::create_dir_all(marketplace.join("plugin/stale")).unwrap();

    write_marketplace(&marketplace, &control).unwrap();

    // As Codex runs it, with the user's shell.
    assert!(
        Command::new("sh")
            .args(["-c", &written_command(&marketplace)])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read_to_string(ran).unwrap(), "codex-hook\n");
    assert!(
        !marketplace.join("plugin/stale").exists(),
        "an earlier installation's files are gone"
    );
    let listed: Value = serde_json::from_slice(
        &fs::read(marketplace.join(".agents/plugins/marketplace.json")).unwrap(),
    )
    .unwrap();
    let plugin: Value = serde_json::from_slice(
        &fs::read(marketplace.join("plugin/.codex-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        format!(
            "{}@{}",
            plugin["name"].as_str().unwrap(),
            listed["name"].as_str().unwrap()
        ),
        PLUGIN
    );
    assert_eq!(listed["plugins"][0]["name"], plugin["name"]);
}

#[test]
fn the_trust_of_other_hooks_stays() {
    let state = json!({
        "progressive-reviewer@herdr-progressive-reviewer:hooks/hooks.json:user_prompt_submit:0:0": {"trusted_hash": "sha256:1"},
        "progressive-reviewer@herdr-progressive-reviewer:hooks/hooks.json:stop:0:0": {"trusted_hash": "sha256:2"},
        "progressive-reviewer@elsewhere:hooks/hooks.json:user_prompt_submit:0:0": {"trusted_hash": "sha256:3"},
        "/home/u/.codex/hooks.json:stop:0:0": {"trusted_hash": "sha256:4"},
    });

    let (removed, kept) = without_plugin(state.as_object().unwrap().clone());

    assert_eq!(removed, 2);
    assert_eq!(
        Value::Object(kept),
        json!({
            "progressive-reviewer@elsewhere:hooks/hooks.json:user_prompt_submit:0:0": {"trusted_hash": "sha256:3"},
            "/home/u/.codex/hooks.json:stop:0:0": {"trusted_hash": "sha256:4"},
        })
    );
}
