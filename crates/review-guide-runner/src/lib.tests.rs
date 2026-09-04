use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Mutex;

use herdr_client::Result;
use herdr_client::protocol::{AgentPrompter, AgentStatus, PaneId, TabId, WorkspaceId};
use review_guide::{FrozenHunk, GuideTarget};

use super::*;

struct FakeHerdr {
    prompt: Mutex<Option<String>>,
}

impl FakeHerdr {
    fn new() -> Self {
        Self {
            prompt: Mutex::new(None),
        }
    }
}

impl AgentPrompter for FakeHerdr {
    fn prompt_agent(&self, _pane_id: &PaneId, prompt: &str) -> Result<()> {
        *self.prompt.lock().unwrap() = Some(prompt.to_owned());
        let value = |label: &str| {
            prompt
                .lines()
                .find_map(|line| line.strip_prefix(label))
                .unwrap()
                .trim_matches('`')
                .to_owned()
        };
        let temporary_path = PathBuf::from(value("- Temporary response: "));
        let response_path = PathBuf::from(value("- Final response: "));
        fs::write(
            &temporary_path,
            serde_json::json!({
                "schema_version": 1,
                "items": [{
                    "target": {
                        "kind": "hunks",
                        "path": "src/lib.rs",
                        "first_hunk": 1,
                        "last_hunk": 1
                    },
                    "text": "The central idea."
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::rename(temporary_path, response_path).unwrap();
        Ok(())
    }
}

fn agent() -> Agent {
    Agent {
        pane_id: PaneId("pane".to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        name: None,
        display_agent: Some("Codex".to_owned()),
        agent: Some("codex".to_owned()),
        agent_status: AgentStatus::Idle,
        agent_session: None,
        cwd: None,
    }
}

fn repository_snapshot(scope: GuideScope) -> GuideRepositorySnapshot {
    GuideRepositorySnapshot {
        repository_root: PathBuf::from("/repository"),
        review_checkpoint: ReviewCheckpoint::new("unit", "checkpoint"),
        scope,
        frozen_diff: "=== FILE src/lib.rs ===\n=== HUNK 1 ===\n@@ -1 +1 @@\n".to_owned(),
        files: vec![FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(b"old\n".to_vec()),
            new_content: Some(b"new\n".to_vec()),
            hunks: vec![FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            }],
            diff_hash: "diff".to_owned(),
        }],
        previous_items: Vec::new(),
        previous_anchored_items: Vec::new(),
    }
}

fn private_mailbox() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

#[test]
fn prepare_writes_the_deterministic_repository_snapshot_and_diff() {
    let mailbox = private_mailbox();
    fs::write(mailbox.path().join(RESPONSE_FILE), b"old response").unwrap();

    GuideRunner::<FakeHerdr>::prepare(
        repository_snapshot(GuideScope::All),
        mailbox.path().to_owned(),
    )
    .unwrap();

    assert!(!mailbox.path().join(RESPONSE_FILE).exists());
    assert!(mailbox.path().join(REPOSITORY_SNAPSHOT_FILE).is_file());
    assert_eq!(
        fs::read_to_string(mailbox.path().join(DIFF_FILE)).unwrap(),
        "=== FILE src/lib.rs ===\n=== HUNK 1 ===\n@@ -1 +1 @@\n"
    );
    let stored = GuideMailbox::open(mailbox.path().to_owned())
        .unwrap()
        .read_repository_snapshot()
        .unwrap();
    assert_eq!(
        stored.review_checkpoint,
        ReviewCheckpoint::new("unit", "checkpoint")
    );
    assert!(stored.frozen_diff.is_empty());
}

#[test]
fn submitted_response_is_loaded_without_a_request_identifier() {
    let mailbox = private_mailbox();
    let prepared = GuideRunner::<FakeHerdr>::prepare(
        repository_snapshot(GuideScope::All),
        mailbox.path().to_owned(),
    )
    .unwrap();
    let client = FakeHerdr::new();
    let runner = GuideRunner::new(&client);

    runner.submit_prepared(&agent(), &prepared).unwrap();
    let result = runner.finish_prepared(&prepared).unwrap();

    let prompt = client.prompt.lock().unwrap().clone().unwrap();
    assert!(prompt.contains("inherits the complete current conversation"));
    assert!(prompt.contains("The guide explains the delta introduced by the frozen diff."));
    assert!(prompt.contains("Would this text describe the old code equally well?"));
    assert!(!prompt.contains("Request ID"));
    assert_eq!(
        result.guide.items[0].target,
        GuideTarget::Hunks {
            path: "src/lib.rs".to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        }
    );
}

#[test]
fn response_version_changes_when_a_new_response_lands() {
    let mailbox = private_mailbox();
    let mailbox = GuideMailbox::open(mailbox.path().to_owned()).unwrap();
    assert_eq!(mailbox.response_version().unwrap(), None);
    fs::write(mailbox.directory.join(RESPONSE_FILE), b"{}").unwrap();
    let first = mailbox.response_version().unwrap().unwrap();
    fs::write(mailbox.directory.join(RESPONSE_FILE), b"[]").unwrap();
    let second = mailbox.response_version().unwrap().unwrap();
    assert_ne!(first, second);
}

#[test]
fn response_watch_ignores_file_access_and_unrelated_mailbox_events() {
    use notify::event::{AccessKind, CreateKind};
    use notify::{Event, EventKind};

    let response_access =
        Event::new(EventKind::Access(AccessKind::Any)).add_path(PathBuf::from(RESPONSE_FILE));
    let unrelated_creation =
        Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from(DIFF_FILE));
    let response_creation =
        Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from(RESPONSE_FILE));

    assert!(!is_response_publication_event(&response_access));
    assert!(!is_response_publication_event(&unrelated_creation));
    assert!(is_response_publication_event(&response_creation));
}

#[test]
fn response_read_accepts_the_limit_and_rejects_one_extra_byte() {
    let directory = private_mailbox();
    let mailbox = GuideMailbox::open(directory.path().to_owned()).unwrap();
    let path = mailbox.directory.join(RESPONSE_FILE);
    let response_limit = usize::try_from(RESPONSE_LIMIT).unwrap();
    fs::write(&path, vec![b'x'; response_limit]).unwrap();
    assert_eq!(
        mailbox.read_response().unwrap().unwrap().len() as u64,
        RESPONSE_LIMIT
    );

    fs::write(&path, vec![b'x'; response_limit + 1]).unwrap();
    assert!(matches!(mailbox.read_response(), Err(Error::LargeResponse)));

    fs::remove_file(&path).unwrap();
    assert_eq!(mailbox.read_response().unwrap(), None);

    let target = directory.path().join("target");
    fs::write(&target, b"{}").unwrap();
    symlink(target, &path).unwrap();
    assert!(matches!(
        mailbox.read_response(),
        Err(Error::Operation { .. })
    ));
}

#[test]
fn mailbox_must_be_a_private_directory() {
    let loose = tempfile::tempdir().unwrap();
    fs::set_permissions(loose.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(GuideMailbox::open(loose.path().to_owned()).is_err());

    let file = tempfile::NamedTempFile::new().unwrap();
    assert!(GuideMailbox::open(file.path().to_owned()).is_err());
}
