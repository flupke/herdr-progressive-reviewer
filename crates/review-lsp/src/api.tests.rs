use std::path::{Path, PathBuf};

use super::{Operation, SourceLocation};

fn location(path: &str) -> SourceLocation {
    SourceLocation {
        path: PathBuf::from(path),
        line: 2,
        byte_column: 3,
        end_line: 4,
        end_byte_column: 5,
    }
}

#[test]
fn operations_have_distinct_titles_and_progress_text() {
    assert_eq!(Operation::Hover.title(), "Documentation");
    assert_eq!(Operation::Definition.title(), "Definitions");
    assert_eq!(Operation::References.title(), "References");
    assert_eq!(Operation::Hover.progress_text(), "Loading documentation…");
    assert_eq!(Operation::Definition.progress_text(), "Finding definition…");
    assert_eq!(Operation::References.progress_text(), "Finding references…");
}

#[test]
fn references_keep_only_repository_locations() {
    let root = Path::new("/repository");
    let inside = location("/repository/src/lib.rs");
    let outside = location("/dependency/src/lib.rs");
    assert_eq!(
        Operation::References.filter_locations(root, vec![inside.clone(), outside.clone()]),
        vec![inside.clone()]
    );
    assert_eq!(
        Operation::Definition.filter_locations(root, vec![inside, outside.clone()]),
        vec![location("/repository/src/lib.rs"), outside]
    );
}

#[test]
fn source_location_paths_and_line_ranges_are_exact() {
    let root = Path::new("/repository");
    let selected = location("/repository/src/lib.rs");
    assert_eq!(selected.review_path(root).as_deref(), Some("src/lib.rs"));
    assert_eq!(selected.display_path(root), "src/lib.rs");
    assert_eq!(
        location("/outside/lib.rs").display_path(root),
        "/outside/lib.rs"
    );
    assert_eq!(
        location("relative.rs").review_path(root).as_deref(),
        Some("relative.rs")
    );

    assert_eq!(selected.range_in_line(1, 10), None);
    assert_eq!(selected.range_in_line(2, 10), Some(3..10));
    assert_eq!(selected.range_in_line(3, 10), Some(0..10));
    assert_eq!(selected.range_in_line(4, 10), Some(0..5));
    assert_eq!(selected.range_in_line(5, 10), None);

    let single = SourceLocation {
        line: 7,
        end_line: 7,
        byte_column: 3,
        end_byte_column: 20,
        ..location("single.rs")
    };
    assert_eq!(single.range_in_line(7, 8), Some(3..8));
}
