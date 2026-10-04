use review_hunks::LineSelection;
use review_repository::repository::{ChangedFile, RepoPath};
use review_source::SourceLineRange;

use super::*;

fn at(path: &str, side: SourceSide, lines: Option<(u32, u32)>) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(path.as_bytes()),
        side,
        lines: lines.map(|(first_line, last_line)| SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

#[test]
fn locations_name_zero_based_lines_on_their_side() {
    let locations = [
        at("src/lib.rs", SourceSide::Old, Some((2, 3))),
        at("src/lib.rs", SourceSide::New, Some((5, 5))),
    ];

    let selection = NamedLines::of(&locations).or_whole(|| unreachable!());

    assert_eq!(selection.removed, [1, 2].into());
    assert_eq!(selection.added, [4].into());
}

#[test]
fn a_whole_file_location_names_every_line() {
    let locations = [at("src/lib.rs", SourceSide::New, None)];
    let every = LineSelection {
        removed: [0].into(),
        added: [0, 1].into(),
    };

    assert_eq!(NamedLines::of(&locations).or_whole(|| every.clone()), every);
}

#[test]
fn only_the_locations_in_a_file_name_its_lines() {
    let file = ChangedFile::modified("src/lib.rs");
    let locations = [
        at("src/other.rs", SourceSide::New, Some((1, 1))),
        at("src/lib.rs", SourceSide::New, Some((3, 4))),
    ];

    let named = NamedLines::in_file(&file, &locations).expect("one location is in the file");

    assert!(named.contains(SourceSide::New, 2) && named.contains(SourceSide::New, 3));
    assert!(!named.contains(SourceSide::New, 0));
    assert!(!named.contains(SourceSide::Old, 2));
    assert_eq!(NamedLines::in_file(&file, &locations[..1]), None);
    assert!(
        NamedLines::in_file(&file, &[at("src/lib.rs", SourceSide::Old, None)])
            .unwrap()
            .contains(SourceSide::New, 40)
    );
}
