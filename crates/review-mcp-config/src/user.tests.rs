use std::fs;
use std::os::unix::fs::symlink;

use super::{Client, UserConfig};

#[test]
fn user_registration_preserves_settings_policies_and_a_symlinked_codex_home() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("dotfiles");
    fs::create_dir(&directory).unwrap();
    let home = root.path().join("codex");
    symlink(&directory, &home).unwrap();
    let path = directory.join("config.toml");
    let original = "# Keep this\nmodel = 'custom'\n[mcp_servers.other]\ncommand = 'other'\n";
    fs::write(&path, original).unwrap();
    let config = UserConfig::new(Client::Codex, &home, "/fixture/reviewer-mcp".into());
    assert!(config.install().unwrap());
    let installed = fs::read_to_string(&path).unwrap();
    assert!(installed.starts_with(original));
    let document: toml_edit::DocumentMut = installed.parse().unwrap();
    assert!(
        document["mcp_servers"]["herdr_reviewer"]["args"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        document["mcp_servers"]["herdr_reviewer"]["command"].as_str(),
        Some("/fixture/reviewer-mcp")
    );
    let restricted = format!(
        "{installed}enabled = false\n[mcp_servers.herdr_reviewer.tools.reply]\napproval_mode = 'prompt'\n"
    );
    fs::write(&path, &restricted).unwrap();
    assert!(!config.install().unwrap());
    assert_eq!(fs::read_to_string(path).unwrap(), restricted);
    assert!(fs::symlink_metadata(home).unwrap().is_symlink());
}

#[test]
fn user_registration_keeps_conflicting_servers_and_symlinked_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original =
        "[mcp_servers.herdr_reviewer]\ncommand = '/fixture/reviewer-mcp'\nargs = ['59123']\n";
    fs::write(&path, original).unwrap();
    let config = UserConfig::new(
        Client::Codex,
        directory.path(),
        "/fixture/reviewer-mcp".into(),
    );
    assert!(config.install().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::rename(&path, directory.path().join("original.toml")).unwrap();
    symlink("original.toml", &path).unwrap();
    assert!(config.install().is_err());
    assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn user_registration_adds_port_forwarding_without_replacing_environment_or_policy() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let original = "[mcp_servers.herdr_reviewer]\ncommand = '/fixture/reviewer-mcp'\nargs = []\nenv_vars = ['PATH']\nenabled = false\n[mcp_servers.herdr_reviewer.env]\nCUSTOM = 'keep'\n[mcp_servers.herdr_reviewer.tools.reply]\napproval_mode = 'prompt'\n";
    fs::write(&path, original).unwrap();
    let config = UserConfig::new(
        Client::Codex,
        directory.path(),
        "/fixture/reviewer-mcp".into(),
    );
    assert!(config.install().unwrap());
    let installed = fs::read_to_string(&path).unwrap();
    let document: toml_edit::DocumentMut = installed.parse().unwrap();
    let server = &document["mcp_servers"]["herdr_reviewer"];
    assert_eq!(
        server["env_vars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        ["PATH", "HERDR_REVIEWER_MCP_PORT"]
    );
    assert_eq!(server["enabled"].as_bool(), Some(false));
    assert_eq!(server["env"]["CUSTOM"].as_str(), Some("keep"));
    assert_eq!(
        server["tools"]["reply"]["approval_mode"].as_str(),
        Some("prompt")
    );
    assert!(!config.install().unwrap());
    assert_eq!(fs::read_to_string(path).unwrap(), installed);
}
