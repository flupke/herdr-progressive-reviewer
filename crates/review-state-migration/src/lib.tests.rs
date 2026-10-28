use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use review_repository::repository::Repository;
use review_store::{LoadResult, ReviewStore, WorkspaceState};
use review_test_support::{JjFixture, JjLayout};
use review_types::ReviewUnit;
use tempfile::TempDir;

use super::{Error, migrate};

/// The name of the mark file of `a.txt`: the SHA-256 of the path.
const MARK_FILE: &str = "18b7cb099a9ea3f50ba899b5ba81e0d377a5f3b16f8f6eeb8b3e58cd4692b993.json";
const DELETED: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FOREIGN: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const EMPTY: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

struct Fixture {
    repository: JjFixture,
    _state_directory: TempDir,
    state: PathBuf,
    /// A change of the repository, and one it abandoned, so a hidden one.
    kept: ReviewUnit,
    abandoned: ReviewUnit,
}

impl Fixture {
    /// A repository with a second workspace; per-workspace states of its main workspace, of a
    /// deleted workspace that reviewed its abandoned change, of another repository, and an
    /// empty one.
    fn new() -> Self {
        let repository = JjFixture::new(JjLayout::NonColocated);
        repository.write("a.txt", b"a\n");
        repository.new_change("kept");
        let kept = repository.change_id().into();
        repository.write("a.txt", b"b\n");
        repository.new_change("abandoned");
        let abandoned: ReviewUnit = repository.change_id().into();
        repository.jj(["abandon", "@"]);
        let second = repository.root().with_file_name("second");
        repository.jj(["workspace", "add", second.to_str().unwrap()]);
        let state_directory = TempDir::new().unwrap();
        let state = state_directory.path().join("herdr.progressive-reviewer");
        let fixture = Self {
            repository,
            _state_directory: state_directory,
            state,
            kept,
            abandoned,
        };
        let main =
            WorkspaceState::key_of(&fs::canonicalize(fixture.repository.root()).unwrap()).unwrap();
        fixture.write_mark(&main, &fixture.kept, "2026-10-01T00:00:00Z");
        fixture.write_mark(DELETED, &fixture.abandoned, "2026-10-01T00:00:00Z");
        fixture.write_mark(FOREIGN, &"k".repeat(32).into(), "2026-10-01T00:00:00Z");
        fs::create_dir_all(fixture.state.join(EMPTY)).unwrap();
        fixture
    }

    fn main_key(&self) -> String {
        WorkspaceState::key_of(&fs::canonicalize(self.repository.root()).unwrap()).unwrap()
    }

    /// Write a mark of `a.txt` in the per-workspace state `key`, as builds before ADR 0013 did.
    fn write_mark(&self, key: &str, unit: &ReviewUnit, reviewed_at: &str) {
        let directory = self
            .state
            .join(key)
            .join("changes")
            .join(unit.as_str())
            .join("paths");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(MARK_FILE),
            format!(
                r#"{{"schema_version":1,"review_unit":"{}","path_encoding":"utf8","path":"a.txt","baseline_commit_id":"0123456789abcdef","reviewed_at":"{reviewed_at}","author":{{"kind":"reviewer"}}}}"#,
                unit.as_str()
            ),
        )
        .unwrap();
    }

    /// The reviewer of the second workspace opens, as the pane does.
    fn open(&self) -> (Repository, ReviewStore) {
        let repository =
            Repository::discover(self.repository.root().with_file_name("second")).unwrap();
        let store = ReviewStore::open(&self.state, repository.shared_store().unwrap()).unwrap();
        (repository, store)
    }

    fn backups(&self) -> Vec<PathBuf> {
        let mut backups = fs::read_dir(
            self.state
                .with_file_name("herdr.progressive-reviewer.backups"),
        )
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        backups.sort();
        backups
    }

    fn keys_left(&self) -> Vec<String> {
        WorkspaceState::list(&self.state)
            .unwrap()
            .iter()
            .map(|state| state.key().to_owned())
            .collect()
    }
}

fn reviewed_at(store: &ReviewStore, unit: &ReviewUnit) -> Option<String> {
    match store.load(unit, b"a.txt").unwrap() {
        LoadResult::Reviewed(record) => Some(record.reviewed_at),
        _ => None,
    }
}

/// Every file under `directory`, by its path relative to it, with its bytes.
fn files(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(directory, directory, &mut found);
    found
}

#[test]
fn the_states_of_the_repository_are_merged_after_a_backup_of_every_state() {
    let fixture = Fixture::new();
    let (repository, store) = fixture.open();

    let migration = migrate(&repository, &store).unwrap();

    assert!(reviewed_at(&store, &fixture.kept).is_some());
    assert!(reviewed_at(&store, &fixture.abandoned).is_some());
    let merged = fixture.state.join("merged-workspaces");
    let mut moved = migration.merged.clone();
    moved.sort();
    let mut expected = vec![merged.join(fixture.main_key()), merged.join(DELETED)];
    expected.sort();
    assert_eq!(moved, expected);
    assert!(migration.unreadable.is_empty());
    assert_eq!(fixture.keys_left(), [EMPTY, FOREIGN]);
    // An empty state belongs to no repository: it is checked again at each open.
    assert_eq!(
        store.foreign_workspace_states().unwrap(),
        [FOREIGN.to_owned()].into()
    );
    let [backup] = fixture.backups().try_into().unwrap();
    let mut copied = fs::read_dir(&backup)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    copied.sort();
    let mut all = vec![
        "RESTORE.md".to_owned(),
        DELETED.to_owned(),
        EMPTY.to_owned(),
        FOREIGN.to_owned(),
        fixture.main_key(),
    ];
    all.sort();
    assert_eq!(copied, all);
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.state.join("migration.json")).unwrap()).unwrap();
    assert_eq!(record["backup"], backup.to_str().unwrap());

    let again = migrate(&repository, &store).unwrap();

    assert!(again.merged.is_empty());
    assert_eq!(fixture.backups().len(), 1);
}

#[test]
fn a_failed_backup_merges_and_moves_nothing() {
    let fixture = Fixture::new();
    let (repository, store) = fixture.open();
    let backups = fixture
        .state
        .with_file_name("herdr.progressive-reviewer.backups");
    fs::create_dir_all(&backups).unwrap();
    fs::set_permissions(&backups, fs::Permissions::from_mode(0o500)).unwrap();
    let before = files(&fixture.state);

    let failed = migrate(&repository, &store);

    fs::set_permissions(&backups, fs::Permissions::from_mode(0o700)).unwrap();
    let error = failed.unwrap_err();
    assert!(matches!(error, Error::Backup(_)));
    assert!(
        error
            .to_string()
            .starts_with("Could not back up the review state; nothing was merged: ")
    );
    let mut after = files(&fixture.state);
    after
        .retain(|path, _| !path.starts_with("repositories") && path != Path::new("migration.lock"));
    let mut unchanged = before;
    unchanged.retain(|path, _| !path.starts_with("repositories"));
    assert_eq!(after, unchanged);
    assert!(reviewed_at(&store, &fixture.kept).is_none());
    assert!(store.foreign_workspace_states().unwrap().is_empty());
}

#[test]
fn following_restore_md_brings_back_the_state_from_before_the_merge() {
    let fixture = Fixture::new();
    let (repository, store) = fixture.open();
    let mut before = files(&fixture.state);
    before.retain(|path, _| !path.starts_with("repositories"));
    migrate(&repository, &store).unwrap();
    let [backup] = fixture.backups().try_into().unwrap();
    let steps = fs::read_to_string(backup.join("RESTORE.md")).unwrap();

    for removed in ["repositories", "merged-workspaces"] {
        fs::remove_dir_all(fixture.state.join(removed)).unwrap();
    }
    for removed in ["migration.json", "migration.lock"] {
        fs::remove_file(fixture.state.join(removed)).unwrap();
    }
    let script = steps
        .split("```sh\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap();
    let restored = Command::new("sh").arg("-c").arg(script).status().unwrap();

    assert!(restored.success());
    assert_eq!(files(&fixture.state), before);
}

#[test]
fn a_state_an_earlier_build_wrote_again_is_backed_up_again_and_merged() {
    let fixture = Fixture::new();
    let (repository, store) = fixture.open();
    migrate(&repository, &store).unwrap();
    fixture.write_mark(&fixture.main_key(), &fixture.kept, "2099-01-01T00:00:00Z");

    let migration = migrate(&repository, &store).unwrap();

    assert_eq!(
        reviewed_at(&store, &fixture.kept).as_deref(),
        Some("2099-01-01T00:00:00Z")
    );
    assert_eq!(
        migration.merged,
        [fixture
            .state
            .join("merged-workspaces")
            .join(format!("{}-2", fixture.main_key()))]
    );
    let [_, second] = fixture.backups().try_into().unwrap();
    let shared = store.directory().strip_prefix(&fixture.state).unwrap();
    assert!(second.join(fixture.main_key()).join("changes").is_dir());
    assert!(second.join(shared).join("repository.json").is_file());
    assert!(!second.join(FOREIGN).exists());
}

#[test]
fn reviewers_that_open_at_once_merge_each_state_once() {
    let fixture = Fixture::new();
    let opened = [fixture.open(), fixture.open()];

    let merged = std::thread::scope(|scope| {
        let runs = opened
            .iter()
            .map(|(repository, store)| scope.spawn(move || migrate(repository, store).unwrap()))
            .collect::<Vec<_>>();
        runs.into_iter()
            .map(|run| run.join().unwrap().merged.len())
            .collect::<Vec<_>>()
    });

    assert_eq!(merged.iter().sum::<usize>(), 2);
    assert_eq!(fixture.backups().len(), 1);
    assert!(reviewed_at(&opened[0].1, &fixture.abandoned).is_some());
}
