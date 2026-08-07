use std::path::Path;

use super::{Cancellation, RepoPath, RepoType, RepositoryProcess};
use crate::Error;

#[test]
fn repository_type_converts_to_and_from_lowercase_text() {
    assert_eq!(RepoType::Git.to_string(), "git");
    assert_eq!(RepoType::Jj.to_string(), "jj");
    assert_eq!("git".parse(), Ok(RepoType::Git));
    assert_eq!("jj".parse(), Ok(RepoType::Jj));
    assert!("unknown".parse::<RepoType>().is_err());
}

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
