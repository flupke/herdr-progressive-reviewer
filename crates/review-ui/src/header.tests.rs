use super::*;
use crate::ReviewFile;

#[test]
fn totals_file_stats_in_the_header() {
    let mut app = ReviewApp::default();
    let mut reviewed = ReviewFile::new("first.rs", ReviewStatus::Reviewed);
    reviewed.lines_added = 5;
    reviewed.lines_removed = 2;
    let mut unreviewed = ReviewFile::new("second.rs", ReviewStatus::Unreviewed);
    unreviewed.lines_added = 3;
    unreviewed.lines_removed = 4;
    app.files = vec![reviewed, unreviewed];
    let area = Rect::new(0, 0, 80, 1);
    let mut buffer = Buffer::empty(area);

    HeaderView(&app).render(area, &mut buffer);

    let text = (0..area.width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    assert!(text.contains("+8 -6 - 1/2 reviewed"));
    assert_eq!(
        buffer[(u16::try_from(text.find("+8").unwrap()).unwrap(), 0)].fg,
        app.palette.insertion
    );
    assert_eq!(
        buffer[(u16::try_from(text.find("-6").unwrap()).unwrap(), 0)].fg,
        app.palette.deletion
    );
}
