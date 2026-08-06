use super::{DiffParser, DiffRow, MAX_LINE_BYTES, NoticeKind};

#[test]
fn rejects_a_line_above_the_parse_limit() {
    let rows = DiffParser::parse(&vec![b'x'; MAX_LINE_BYTES + 1]);

    assert!(matches!(
        rows.as_slice(),
        [DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            ..
        }]
    ));
}
