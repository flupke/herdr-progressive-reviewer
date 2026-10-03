use review_explore::{CodeLocation, Comparison, SourceSide, Uncitable};
use review_repository::diff::DiffRow;
use review_repository::repository::{RepoPath, RepoType, Repository};
use review_source::SourceLineRange;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use test_case::test_case;

/// A change that rewrites the third line of `policy.rs`, next to an unchanged `caller.rs` and
/// a changed binary file.
fn change(kind: RepoType) -> (Box<dyn ReviewRepositoryFixture>, Comparison) {
    let fixture = repository_fixture(kind);
    fixture.write("policy.rs", b"fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n");
    fixture.write("caller.rs", b"fn main() {}\nfn helper() {}\n");
    fixture.write("binary", &[0, 1, 2]);
    fixture.new_change("base");
    fixture.write(
        "policy.rs",
        b"fn a() {}\nfn b() {}\nfn c2() {}\nfn d() {}\n",
    );
    fixture.write("binary", &[0, 255, 1]);
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &snapshot).unwrap();
    (fixture, comparison)
}

fn location(path: &str, side: SourceSide, lines: Option<(u32, u32)>) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(path.as_bytes()),
        side,
        lines: lines.map(|(first_line, last_line)| SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

/// Each row as its old line and its new line.
fn numbers(rows: &[DiffRow]) -> Vec<(Option<u32>, Option<u32>)> {
    rows.iter()
        .map(|row| match row {
            DiffRow::Context {
                old_line, new_line, ..
            } => (Some(*old_line), Some(*new_line)),
            DiffRow::Delete { old_line, .. } => (Some(*old_line), None),
            DiffRow::Add { new_line, .. } => (None, Some(*new_line)),
            other => panic!("unexpected row {other:?}"),
        })
        .collect()
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_citation_of_changed_lines_shows_the_rows_of_the_change(kind: RepoType) {
    let (fixture, comparison) = change(kind);

    let new = comparison
        .cited_lines(
            &location("policy.rs", SourceSide::New, Some((1, 3))),
            fixture.root(),
        )
        .unwrap();
    assert_eq!(
        numbers(&new.rows),
        [
            (Some(1), Some(1)),
            (Some(2), Some(2)),
            (Some(3), None),
            (None, Some(3))
        ]
    );
    assert_eq!(new.path, "policy.rs");
    assert!(new.old_content.unwrap().starts_with(b"fn a() {}"));
    assert!(
        new.new_content
            .unwrap()
            .ends_with(b"fn c2() {}\nfn d() {}\n")
    );

    let old = comparison
        .cited_lines(
            &location("policy.rs", SourceSide::Old, Some((3, 4))),
            fixture.root(),
        )
        .unwrap();
    assert_eq!(
        numbers(&old.rows),
        [(Some(3), None), (None, Some(3)), (Some(4), Some(4))]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_citation_of_a_file_the_change_leaves_alone_shows_its_lines(kind: RepoType) {
    let (fixture, comparison) = change(kind);

    let lines = comparison
        .cited_lines(
            &location("caller.rs", SourceSide::New, Some((2, 2))),
            fixture.root(),
        )
        .unwrap();

    assert_eq!(
        lines.rows,
        [DiffRow::Context {
            old_line: 2,
            new_line: 2,
            text: " fn helper() {}".into()
        }]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn citations_without_lines_of_text_say_why(kind: RepoType) {
    let (fixture, comparison) = change(kind);
    let cite = |location| comparison.cited_lines(&location, fixture.root());

    assert_eq!(
        cite(location("policy.rs", SourceSide::New, None)),
        Err(Uncitable::WholeFile)
    );
    assert!(matches!(
        cite(location("binary", SourceSide::New, Some((1, 1)))),
        Err(Uncitable::Unreadable(_))
    ));
    assert_eq!(
        cite(location("policy.rs", SourceSide::New, Some((4, 5)))),
        Err(Uncitable::Range)
    );
    assert_eq!(
        cite(location("../policy.rs", SourceSide::New, Some((1, 1)))),
        Err(Uncitable::Path)
    );
}
