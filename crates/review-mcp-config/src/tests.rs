use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Barrier;

use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    configuration: ProjectConfig,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::for_repository(directory.path(), Some(59123)).unwrap();
        let configuration =
            ProjectConfig::new(directory.path(), endpoint, "/fixture/reviewer-mcp".into());
        Self {
            directory,
            configuration,
        }
    }

    fn write(&self, client: Client, text: &str) {
        let path = self.directory.path().join(client.config_path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
    }

    fn read(&self, client: Client) -> String {
        fs::read_to_string(self.directory.path().join(client.config_path())).unwrap()
    }
}

#[test]
fn registration_preserves_settings_comments_permissions_and_repeat_runs() {
    let fixture = Fixture::new();
    let codex = "# Keep this comment\nmodel = 'custom'\n\n[mcp_servers.existing]\nurl = 'https://example.com/mcp' # and this\n";
    fixture.write(Client::Codex, codex);
    fixture.write(
        Client::Claude,
        r#"{"setting":"keep","mcpServers":{"existing":{"type":"stdio","command":"other","args":["a"]}}}"#,
    );
    for client in Client::ALL {
        assert!(fixture.configuration.install(client).unwrap());
        let installed = fixture.read(client);
        assert!(!fixture.configuration.install(client).unwrap());
        assert_eq!(fixture.read(client), installed);
        assert_eq!(
            fs::metadata(fixture.directory.path().join(client.config_path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
    assert!(fixture.read(Client::Codex).starts_with(codex));
    let claude: serde_json::Value = serde_json::from_str(&fixture.read(Client::Claude)).unwrap();
    assert_eq!(claude["setting"], "keep");
    assert_eq!(claude["mcpServers"]["existing"]["args"][0], "a");
    assert_eq!(claude["mcpServers"]["herdr_reviewer"]["args"][0], "59123");
}

#[test]
fn concurrent_setup_registers_each_client_once_without_adding_lock_files() {
    let fixture = Fixture::new();
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let handles = [0, 1].map(|_| {
            scope.spawn(|| {
                barrier.wait();
                Client::ALL
                    .map(|client| fixture.configuration.install(client).unwrap())
                    .into_iter()
                    .filter(|added| *added)
                    .count()
            })
        });
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .sum::<usize>(),
            2
        );
    });
    assert_eq!(fs::read_dir(fixture.directory.path()).unwrap().count(), 2);
}

#[test]
fn a_second_workspace_cannot_replace_the_first_reviewers_url_or_permissions() {
    let fixture = Fixture::new();
    for client in Client::ALL {
        fixture.configuration.install(client).unwrap();
        let original = fixture.read(client);
        let other = ProjectConfig::new(
            fixture.directory.path(),
            Endpoint::for_repository(fixture.directory.path(), Some(59124)).unwrap(),
            "/fixture/reviewer-mcp".into(),
        );
        assert!(
            other
                .install(client)
                .unwrap_err()
                .contains("launch override")
        );
        assert_eq!(fixture.read(client), original);
    }
    fixture.write(
        Client::Codex,
        "[mcp_servers.herdr_reviewer]\nurl='http://127.0.0.1:59123/mcp'\nenabled=false\ndefault_tools_approval_mode='prompt'\n",
    );
    assert!(fixture.configuration.install(Client::Codex).unwrap());
    let original = fixture.read(Client::Codex);
    assert!(original.contains("enabled=false"));
    assert!(original.contains("default_tools_approval_mode='prompt'"));
    assert!(!fixture.configuration.install(Client::Codex).unwrap());
    assert_eq!(fixture.read(Client::Codex), original);
}

#[test]
fn invalid_configuration_and_symlinks_are_left_untouched() {
    let fixture = Fixture::new();
    for (client, text) in [
        (Client::Codex, "[unfinished"),
        (Client::Codex, "mcp_servers=123"),
        (Client::Claude, "{unfinished"),
        (Client::Claude, "[]"),
        (Client::Claude, "{\"mcpServers\":123}"),
    ] {
        fixture.write(client, text);
        assert!(fixture.configuration.install(client).is_err());
        assert_eq!(fixture.read(client), text);
    }
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("config.json");
    fs::write(&target, "{}").unwrap();
    let link = fixture.directory.path().join(Client::Claude.config_path());
    fs::remove_file(&link).unwrap();
    symlink(&target, &link).unwrap();
    assert!(fixture.configuration.install(Client::Claude).is_err());
    assert!(fs::symlink_metadata(link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "{}");
}

#[test]
fn legacy_http_migration_keeps_tool_policies_and_rejects_custom_transports() {
    let fixture = Fixture::new();
    fixture.write(Client::Codex,
        "[mcp_servers.herdr_reviewer]\nurl='http://127.0.0.1:59123/mcp'\nenabled=false # retain\n[ mcp_servers.herdr_reviewer.tools.reply ]\napproval_mode='prompt'\n");
    fixture.write(
        Client::Claude,
        r#"{"mcpServers":{"herdr_reviewer":{"type":"http","url":"http://127.0.0.1:59123/mcp"}}}"#,
    );
    for client in Client::ALL {
        assert!(fixture.configuration.install(client).unwrap());
        assert!(!fixture.configuration.install(client).unwrap());
        assert!(fixture.read(client).contains("/fixture/reviewer-mcp"));
        assert!(!fixture.read(client).contains("http://"));
    }
    let migrated = fixture.read(Client::Codex);
    assert!(migrated.contains("enabled=false # retain"));
    assert!(migrated.contains("approval_mode='prompt'"));
    for (client, text) in [
        (
            Client::Codex,
            "[mcp_servers.herdr_reviewer]\nurl='http://127.0.0.1:59123/mcp'\nhttp_headers={Authorization='custom'}\n",
        ),
        (
            Client::Claude,
            r#"{"mcpServers":{"herdr_reviewer":{"type":"http","url":"http://127.0.0.1:59123/mcp","headers":{"Authorization":"custom"}}}}"#,
        ),
    ] {
        fixture.write(client, text);
        assert!(fixture.configuration.install(client).is_err());
        assert_eq!(fixture.read(client), text);
    }
}
