use std::io::Write;

use super::*;

#[test]
fn split_terminal_output_publishes_only_completed_frames_and_preserves_unicode() {
    let output = "\x1b[?2026h\x1b[H日本a\x1b[?2026l".as_bytes();
    for split in 0..output.len() {
        let directory = tempfile::tempdir().unwrap();
        let frames = Frames::new(directory.path().to_owned()).unwrap();
        let mut replay = Replay::new(10, 2).unwrap();
        replay.output(&output[..split], &frames).unwrap();
        assert!(
            frames.latest().is_err(),
            "published an incomplete frame at byte {split}"
        );
        replay.output(&output[split..], &frames).unwrap();
        let frame = frames.latest().unwrap();
        assert_eq!(frame.text, "日本a     \n          ");
        assert_eq!(frame.number, 1);
    }
}

#[test]
fn coalesced_output_preserves_frame_history_and_deduplicates_unchanged_paints() {
    let directory = tempfile::tempdir().unwrap();
    let frames = Frames::new(directory.path().to_owned()).unwrap();
    let mut replay = Replay::new(10, 2).unwrap();
    replay
        .output(
            b"\x1b[Hone\x1b[?2026l\x1b[Htwo\x1b[?2026l\x1b[?2026l",
            &frames,
        )
        .unwrap();
    assert_eq!(frames.latest().unwrap().number, 2);
    assert!(
        std::fs::read_to_string(frames.path(1))
            .unwrap()
            .contains("one")
    );
    assert!(
        std::fs::read_to_string(frames.path(2))
            .unwrap()
            .contains("two")
    );
    for number in 0..70 {
        replay
            .output(format!("\x1b[H{number:03}\x1b[?2026l").as_bytes(), &frames)
            .unwrap();
    }
    assert_eq!(
        std::fs::read_dir(directory.path().join("frames"))
            .unwrap()
            .count(),
        64
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("latest.txt")).unwrap(),
        frames.latest().unwrap().display()
    );
}

#[test]
fn growing_recording_handles_partial_utf8_and_resize_events() {
    let directory = tempfile::tempdir().unwrap();
    let frames = Frames::new(directory.path().to_owned()).unwrap();
    let path = directory.path().join("session.cast");
    let mut writer = File::create(&path).unwrap();
    writeln!(writer, "{{\"version\":2,\"width\":10,\"height\":2}}").unwrap();
    let mut recording = Recording::new(File::open(&path).unwrap());
    let data = serde_json::to_string(&(0.1, "o", "\x1b[H日本\x1b[?2026l")).unwrap() + "\n";
    let split = data.find('日').unwrap() + 1;
    writer.write_all(&data.as_bytes()[..split]).unwrap();
    recording.drain(&frames).unwrap();
    assert!(frames.latest().is_err());
    writer.write_all(&data.as_bytes()[split..]).unwrap();
    recording.drain(&frames).unwrap();
    assert!(frames.latest().unwrap().text.starts_with("日本"));
    writeln!(writer, "[0.2,\"r\",\"6x3\"]").unwrap();
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&(0.3, "o", "\x1b[?2026l")).unwrap()
    )
    .unwrap();
    recording.drain(&frames).unwrap();
    assert_eq!(
        frames.latest().unwrap().size,
        tui_test::Size { cols: 6, rows: 3 }
    );
}
