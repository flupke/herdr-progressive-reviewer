use review_explore::{Comparison, SourceSide};
use review_repository::repository::{RepoType, Repository};
use review_test_support::{complete_repository_snapshot, repository_fixture};
use test_case::test_case;

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn comparison_includes_the_whole_change_and_reads_unchanged_sources_from_disk(kind: RepoType) {
    let fixture = repository_fixture(kind);
    fixture.write("policy.rs", b"old policy\n");
    fixture.write("caller.rs", b"original caller\n");
    fixture.write("deleted.rs", b"deleted code\n");
    fixture.write("renamed.rs", b"renamed code\n");
    fixture.new_change("base");
    fixture.write("policy.rs", b"new policy\n");
    fixture.write("tests.rs", b"test policy\n");
    fixture.write("binary", &[0, 255, 1]);
    fixture.remove("deleted.rs");
    fixture.rename("renamed.rs", "moved.rs");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &snapshot).unwrap();
    assert_eq!(comparison.files, snapshot.files);
    assert!(comparison.manifest.len() >= snapshot.files.len());
    // The metadata also identifies historical text through ordinary VCS tools,
    // without access to reviewer-private files or an MCP source-reading endpoint.
    let (program, arguments) = match kind {
        RepoType::Git => (
            "git",
            vec![
                "show".into(),
                format!("{}:policy.rs", comparison.checkpoint.review_unit.as_str()),
            ],
        ),
        RepoType::Jj => (
            "jj",
            vec![
                "--ignore-working-copy".into(),
                "file".into(),
                "show".into(),
                "-r".into(),
                format!("{}-", comparison.checkpoint.checkpoint),
                "--".into(),
                "policy.rs".into(),
            ],
        ),
    };
    let historical = std::process::Command::new(program)
        .args(arguments)
        .current_dir(fixture.root())
        .output()
        .unwrap();
    assert!(
        historical.status.success(),
        "{}",
        String::from_utf8_lossy(&historical.stderr)
    );
    assert_eq!(historical.stdout, b"old policy\n");
    assert_eq!(
        std::fs::read(fixture.root().join("tests.rs")).unwrap(),
        b"test policy\n"
    );
    assert!(
        comparison
            .sources
            .iter()
            .any(|source| source.display_path == "binary"
                && source.read_text(fixture.root()).is_err())
    );
    assert!(
        comparison
            .sources
            .iter()
            .any(|source| source.display_path == "deleted.rs" && source.side == SourceSide::Old)
    );
    fixture.write("caller.rs", b"changed caller outside evidence\n");
    let caller = comparison
        .working_source_at(std::path::Path::new("caller.rs"))
        .unwrap();
    assert!(caller.content.is_none());
    assert_eq!(
        caller.read_text(fixture.root()).unwrap(),
        "changed caller outside evidence\n"
    );
    assert_ne!(
        repository.current_identity().unwrap().snapshot_id(),
        comparison.checkpoint.checkpoint
    );
    assert!(
        comparison
            .working_source_at(std::path::Path::new("../caller.rs"))
            .is_none()
    );
}

#[test]
fn citations_preserve_utf8_and_raw_unix_paths_without_source_registration() {
    use review_explore::CodeLocation;
    use review_repository::repository::RepoPath;
    for bytes in [b"src/caller.rs".as_slice(), b"src/\xff.rs"] {
        let location = CodeLocation {
            path: RepoPath::from_bytes(bytes),
            side: SourceSide::New,
            lines: None,
        };
        let json = serde_json::to_value(&location).unwrap();
        assert_eq!(json["path"].is_string(), std::str::from_utf8(bytes).is_ok());
        assert_eq!(
            serde_json::from_value::<CodeLocation>(json).unwrap(),
            location
        );
    }
}

#[test]
fn large_unchanged_assets_do_not_expand_the_comparison_or_impose_a_repository_size_limit() {
    let fixture = repository_fixture(RepoType::Git);
    fixture.write("policy.rs", b"old policy\n");
    fixture.write("asset.bin", b"\0");
    std::fs::OpenOptions::new()
        .write(true)
        .open(fixture.root().join("asset.bin"))
        .unwrap()
        .set_len(65 * 1024 * 1024)
        .unwrap();
    fixture.new_change("base with large asset");
    fixture.write("policy.rs", b"new policy\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &snapshot).unwrap();
    assert_eq!(comparison.files.len(), 1);
    let asset = comparison
        .working_source_at(std::path::Path::new("asset.bin"))
        .unwrap();
    assert!(asset.content.is_none());
    assert_eq!(comparison.sources.len(), 2);
    assert!(
        !comparison
            .sources
            .iter()
            .any(|source| source.display_path == "asset.bin")
    );
    assert!(
        comparison
            .sources
            .iter()
            .filter(|source| source.side == SourceSide::New)
            .all(|source| source.content.is_none())
    );
}

#[test]
fn working_copy_sources_do_not_follow_symlinks_outside_the_repository() {
    let fixture = repository_fixture(RepoType::Git);
    fixture.write("caller.rs", b"caller\n");
    fixture.new_change("base");
    fixture.write("policy.rs", b"policy\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &snapshot).unwrap();
    let caller = comparison
        .working_source_at(std::path::Path::new("caller.rs"))
        .unwrap();
    let outside = state.path().join("outside.rs");
    std::fs::write(&outside, "outside\n").unwrap();
    fixture.remove("caller.rs");
    std::os::unix::fs::symlink(outside, fixture.root().join("caller.rs")).unwrap();
    assert!(caller.read(fixture.root()).is_err());
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn uncataloged_old_citations_resolve_the_review_base(kind: RepoType) {
    let fixture = repository_fixture(kind);
    fixture.write("caller.rs", b"original caller\n");
    fixture.new_change("base");
    fixture.write("changed.rs", b"new code\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &snapshot).unwrap();
    assert!(
        !comparison
            .sources
            .iter()
            .any(|source| source.display_path == "caller.rs")
    );
    fixture.write("caller.rs", b"current caller\n");
    let location = review_explore::CodeLocation {
        path: review_repository::repository::RepoPath::from_bytes(b"caller.rs"),
        side: SourceSide::Old,
        lines: None,
    };
    assert_eq!(
        comparison
            .source(&location)
            .unwrap()
            .read_text(fixture.root())
            .unwrap(),
        "original caller\n"
    );
}
