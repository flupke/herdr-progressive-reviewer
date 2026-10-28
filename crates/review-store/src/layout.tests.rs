use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::{Error, ReviewStore};

/// The folder the review state of the store at `store` lives in, by the format of ADR 0013.
fn expected_folder(state: &Path, slug: &str, store: &Path) -> PathBuf {
    let store = fs::canonicalize(store).unwrap();
    let hash = Sha256::digest(store.to_str().unwrap().as_bytes());
    let hex = hash.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    state
        .join("repositories")
        .join(format!("{slug}-{}", &hex[..16]))
}

fn record(folder: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(folder.join("repository.json")).unwrap()).unwrap()
}

#[test]
fn a_jj_repository_is_named_after_the_workspace_that_holds_its_store() {
    let temporary = TempDir::new().unwrap();
    let store = temporary.path().join("my repo/.jj/repo");
    fs::create_dir_all(&store).unwrap();
    let state = temporary.path().join("state");

    ReviewStore::open(&state, &store).unwrap();

    let folder = expected_folder(&state, "my_repo", &store);
    assert_eq!(
        record(&folder),
        serde_json::json!({
            "version": 1,
            "kind": "jj",
            "store": fs::canonicalize(&store).unwrap().to_str().unwrap(),
        })
    );
}

#[test]
fn a_git_repository_is_named_after_its_main_worktree_or_its_bare_directory() {
    let temporary = TempDir::new().unwrap();
    let common = temporary.path().join("project/.git");
    let bare = temporary.path().join("bare.git");
    fs::create_dir_all(&common).unwrap();
    fs::create_dir_all(&bare).unwrap();
    let state = temporary.path().join("state");

    ReviewStore::open(&state, &common).unwrap();
    ReviewStore::open(&state, &bare).unwrap();

    assert_eq!(
        record(&expected_folder(&state, "project", &common))["kind"],
        "git"
    );
    assert_eq!(
        record(&expected_folder(&state, "bare.git", &bare))["kind"],
        "git"
    );
}

#[test]
fn a_long_unicode_name_is_cut_to_64_safe_bytes() {
    let temporary = TempDir::new().unwrap();
    let name = format!("é{}", "a".repeat(100));
    let store = temporary.path().join(&name).join(".git");
    fs::create_dir_all(&store).unwrap();
    let state = temporary.path().join("state");

    ReviewStore::open(&state, &store).unwrap();

    let slug = format!("__{}", "a".repeat(62));
    assert!(
        expected_folder(&state, &slug, &store)
            .join("repository.json")
            .is_file()
    );
}

#[test]
fn a_folder_that_names_another_repository_is_refused() {
    let temporary = TempDir::new().unwrap();
    let store = temporary.path().join("project/.git");
    fs::create_dir_all(&store).unwrap();
    let state = temporary.path().join("state");
    let folder = expected_folder(&state, "project", &store);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("repository.json"),
        br#"{"version":1,"kind":"git","store":"/elsewhere/.git"}"#,
    )
    .unwrap();

    let opened = ReviewStore::open(&state, &store);

    assert!(matches!(opened, Err(Error::StateCollision { .. })));
}
