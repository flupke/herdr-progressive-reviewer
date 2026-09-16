use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Barrier;

use super::*;

struct Fixture {
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        Self { directory }
    }

    fn configuration(&self, client: Client) -> UserConfig {
        UserConfig::new(
            client,
            self.directory.path(),
            "/fixture/reviewer-mcp".into(),
        )
    }

    fn write(&self, client: Client, text: &str) {
        let path = self.configuration(client).path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o640)).unwrap();
    }

    fn read(&self, client: Client) -> String {
        fs::read_to_string(self.configuration(client).path()).unwrap()
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
        assert!(fixture.configuration(client).install().unwrap());
        let installed = fixture.read(client);
        assert!(!fixture.configuration(client).install().unwrap());
        assert_eq!(fixture.read(client), installed);
        assert_eq!(
            fs::metadata(fixture.configuration(client).path())
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
    assert_eq!(
        claude["mcpServers"]["herdr_reviewer"]["args"],
        serde_json::json!([])
    );
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
                    .map(|client| fixture.configuration(client).install().unwrap())
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
fn a_different_installation_cannot_replace_an_existing_registration() {
    let fixture = Fixture::new();
    for client in Client::ALL {
        fixture.configuration(client).install().unwrap();
        let original = fixture.read(client);
        let other = UserConfig::new(
            client,
            fixture.directory.path(),
            "/other/reviewer-mcp".into(),
        );
        assert!(other.install().unwrap_err().contains("launch override"));
        assert_eq!(fixture.read(client), original);
    }
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
        assert!(fixture.configuration(client).install().is_err());
        assert_eq!(fixture.read(client), text);
    }
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("config.json");
    fs::write(&target, "{}").unwrap();
    let link = fixture.configuration(Client::Claude).path();
    fs::remove_file(&link).unwrap();
    symlink(&target, &link).unwrap();
    assert!(fixture.configuration(Client::Claude).install().is_err());
    assert!(fs::symlink_metadata(link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "{}");
}

#[test]
fn existing_http_and_explicit_port_registrations_are_left_untouched() {
    let fixture = Fixture::new();
    for (client, text) in [
        (
            Client::Codex,
            "[mcp_servers.herdr_reviewer]\nurl='http://127.0.0.1:59123/mcp'\nhttp_headers={Authorization='custom'}\n",
        ),
        (
            Client::Claude,
            r#"{"mcpServers":{"herdr_reviewer":{"type":"http","url":"http://127.0.0.1:59123/mcp","headers":{"Authorization":"custom"}}}}"#,
        ),
        (
            Client::Codex,
            "[mcp_servers.herdr_reviewer]\ncommand='/fixture/reviewer-mcp'\nargs=['59123']\n",
        ),
        (
            Client::Claude,
            r#"{"mcpServers":{"herdr_reviewer":{"type":"stdio","command":"/fixture/reviewer-mcp","args":["59123"]}}}"#,
        ),
    ] {
        fixture.write(client, text);
        assert!(fixture.configuration(client).install().is_err());
        assert_eq!(fixture.read(client), text);
    }
}
