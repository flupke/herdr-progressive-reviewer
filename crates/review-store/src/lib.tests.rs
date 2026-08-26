use std::fs;

use tempfile::TempDir;

use super::{OutputTarget, ReviewStore};

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

#[path = "checkpoint.tests.rs"]
mod checkpoint_tests;

#[path = "guide.tests.rs"]
mod guide_tests;
