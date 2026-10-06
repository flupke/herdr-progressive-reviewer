use super::*;

#[test]
fn a_marker_reads_back_from_its_title() {
    let marker = FrameMarker {
        frame: 12,
        acknowledged: 3,
        columns: 100,
        rows: 30,
    };
    assert_eq!(FrameMarker::parse(&marker.title()), Some(marker));
}

#[test]
fn the_escape_sequence_sets_the_title() {
    let marker = FrameMarker::default();
    assert_eq!(
        marker.escape_sequence(),
        format!("\x1b]2;{}\x07", marker.title())
    );
}

#[test]
fn other_titles_carry_no_marker() {
    for title in [
        "",
        "zsh",
        "reviewer vision",
        "reviewer vision frame=1 ack=0",
        "reviewer vision frame=x ack=0 size=1x1",
        "reviewer vision frame=1 ack=0 size=1x1 extra",
    ] {
        assert_eq!(FrameMarker::parse(title), None, "{title:?}");
    }
}

#[test]
fn a_request_reads_back_its_number() {
    assert_eq!(
        parse_acknowledgement_request(&acknowledgement_request(7)),
        Some(7)
    );
    assert_eq!(parse_acknowledgement_request("hello"), None);
}
