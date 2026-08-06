use std::path::Path;

use super::{Cancellation, RepoPath, RepositoryProcess};
use crate::Error;

#[test]
fn repository_paths_preserve_non_utf8_bytes() {
    let path = RepoPath::from_bytes(b"invalid-\xff.txt");

    assert_eq!(path.0, b"invalid-\xff.txt");
    assert_eq!(path.display(), r"invalid-\xff.txt");
}

#[test]
fn cancellation_stops_a_child_command() {
    let cancellation = Cancellation::default();
    cancellation.cancel();

    let error = RepositoryProcess::new("jj", Path::new("."), "test jj cancellation", &cancellation)
        .output(["version"])
        .unwrap_err();

    assert!(matches!(error, Error::CommandCancelled { .. }));
}
