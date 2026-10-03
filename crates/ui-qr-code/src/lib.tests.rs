use super::*;

const ADDRESS: &str = "http://192.168.1.23:8790/?token=0123456789abcdef0123456789abcdef";

/// Whether the module at `column` and `row`, counted from the quiet zone's corner, is dark.
fn dark(code: &QrCode, column: usize, row: usize) -> bool {
    let line = &code.text().lines[row / 2];
    let span = &line.spans[column];
    let color = if row.is_multiple_of(2) {
        span.style.fg
    } else {
        span.style.bg
    };
    match color {
        Some(color) if color == DARK => true,
        Some(color) if color == LIGHT => false,
        other => panic!("module {column},{row} has the color {other:?}"),
    }
}

#[test]
fn a_page_address_fits_in_a_narrow_pane() {
    let code = QrCode::new(ADDRESS).unwrap();

    // 33 modules of a version 4 code, and two of quiet zone on each side; two modules a row.
    assert_eq!(code.width(), 37);
    assert_eq!(code.height(), 19);
    assert_eq!(code.text().lines.len(), 19);
}

#[test]
fn the_code_is_dark_on_light_with_its_finder_patterns_in_three_corners() {
    let code = QrCode::new(ADDRESS).unwrap();
    let size = usize::from(code.width());

    for offset in 0..QUIET_ZONE {
        for along in 0..size {
            assert!(
                !dark(&code, offset, along),
                "quiet zone at {offset},{along}"
            );
            assert!(
                !dark(&code, along, offset),
                "quiet zone at {along},{offset}"
            );
        }
    }
    // A finder pattern's dark ring, seven modules wide, in the top-left, top-right and
    // bottom-left corners.
    let modules = size - 2 * QUIET_ZONE;
    for (left, top) in [(0, 0), (modules - 7, 0), (0, modules - 7)] {
        for along in 0..7 {
            for (column, row) in [(along, 0), (along, 6), (0, along), (6, along)] {
                assert!(dark(
                    &code,
                    QUIET_ZONE + left + column,
                    QUIET_ZONE + top + row
                ));
            }
        }
        assert!(!dark(&code, QUIET_ZONE + left + 1, QUIET_ZONE + top + 1));
        assert!(dark(&code, QUIET_ZONE + left + 3, QUIET_ZONE + top + 3));
    }
}
