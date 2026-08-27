use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use super::{Error, MAX_STATE_FILE_BYTES, OutputTarget, ReviewStore};

struct Fixture {
    temporary: TempDir,
    state: std::path::PathBuf,
    repository: std::path::PathBuf,
    change: String,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let state = temporary.path().join("state");
        let repository = temporary.path().join("repository");
        fs::create_dir(&repository).unwrap();
        Self {
            temporary,
            state,
            repository,
            change: "a".repeat(64),
        }
    }

    fn store(&self) -> ReviewStore {
        ReviewStore::open(&self.state, &self.repository).unwrap()
    }
}

#[test]
fn settings_are_shared_between_repositories() {
    let fixture = Fixture::new();
    let other_repository = fixture.temporary.path().join("other");
    fs::create_dir(&other_repository).unwrap();
    fixture.store().save_file_pane_width(42).unwrap();
    fixture
        .store()
        .save_output_target(OutputTarget::Clipboard)
        .unwrap();

    let settings = ReviewStore::open(&fixture.state, other_repository).unwrap();
    assert_eq!(settings.file_pane_width().unwrap(), Some(42));
    assert_eq!(settings.output_target().unwrap(), OutputTarget::Clipboard);
}

#[test]
fn parent_sync_rejects_a_target_outside_the_state_root() {
    let fixture = Fixture::new();
    let store = fixture.store();

    assert!(matches!(
        store.sync_parent(&fixture.repository.join("record.json")),
        Err(Error::InvalidStateKey {
            field: "record parent"
        })
    ));
}

#[test]
fn bounded_reads_accept_the_limit_and_reject_one_extra_byte() {
    let fixture = Fixture::new();
    let target = fixture.temporary.path().join("bounded");
    let limit: usize = MAX_STATE_FILE_BYTES.try_into().unwrap();
    assert_eq!(limit, 1_048_576);
    fs::write(&target, vec![b'x'; limit]).unwrap();

    assert_eq!(
        ReviewStore::read_bytes(&target, "test bounded read", Some(MAX_STATE_FILE_BYTES))
            .unwrap()
            .unwrap()
            .len(),
        limit
    );

    fs::write(&target, vec![b'x'; limit + 1]).unwrap();
    assert_eq!(
        ReviewStore::read_bytes(&target, "test bounded read", Some(MAX_STATE_FILE_BYTES)).unwrap(),
        None
    );
}

#[test]
fn writes_remove_only_temporary_files_older_than_one_day() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let old = fixture.state.join(".tmp-old");
    let two_hours_old = fixture.state.join(".tmp-two-hours");
    let thirty_minutes_old = fixture.state.join(".tmp-thirty-minutes");
    let ordinary = fixture.state.join("ordinary");
    for path in [&old, &two_hours_old, &thirty_minutes_old, &ordinary] {
        fs::write(path, b"temporary").unwrap();
    }
    let set_age = |path: &std::path::Path, age| {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(SystemTime::now() - age))
            .unwrap();
    };
    set_age(&old, Duration::from_secs(25 * 60 * 60));
    set_age(&two_hours_old, Duration::from_secs(2 * 60 * 60));
    set_age(&thirty_minutes_old, Duration::from_secs(30 * 60));
    set_age(&ordinary, Duration::from_secs(25 * 60 * 60));

    store.save_file_pane_width(40).unwrap();

    assert!(!old.exists());
    assert!(two_hours_old.exists());
    assert!(thirty_minutes_old.exists());
    assert!(ordinary.exists());
}

#[test]
fn reads_do_not_follow_symbolic_links() {
    let fixture = Fixture::new();
    let source = fixture.temporary.path().join("source");
    let link = fixture.temporary.path().join("link");
    fs::write(&source, b"content").unwrap();
    symlink(&source, &link).unwrap();

    assert!(matches!(
        ReviewStore::read_bytes(&link, "test symbolic link", None),
        Err(Error::StateIo { .. })
    ));
}

#[path = "checkpoint.tests.rs"]
mod checkpoint_tests;

#[path = "guide.tests.rs"]
mod guide_tests;
