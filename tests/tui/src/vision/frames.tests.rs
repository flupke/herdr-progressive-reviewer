use super::*;

/// The guard of a wait that an event ends: it fires only if the test fails.
const GUARD: Duration = Duration::from_secs(30);

fn marker(frame: u64, acknowledged: u64) -> FrameMarker {
    FrameMarker {
        frame,
        acknowledged,
        columns: 100,
        rows: 30,
    }
}

fn updated(frames: &Frames, pane: &str, title: &str) {
    frames.observe(
        "pane_updated",
        &json!({"pane": {
            "pane_id": pane, "workspace_id": pane.split(':').next().unwrap(),
            "terminal_title": title,
        }}),
    );
}

#[test]
fn a_pane_update_with_a_marker_is_the_panes_latest_frame() {
    let frames = Frames::new("w1".into());
    updated(&frames, "w1:p2", &marker(3, 1).title());
    updated(&frames, "w1:p2", "zsh");
    assert_eq!(
        frames.pane("w1:p2"),
        PaneFrames {
            latest: Some(marker(3, 1)),
            exited: false,
            agent: false,
        }
    );
}

#[test]
fn panes_of_other_workspaces_are_ignored() {
    let frames = Frames::new("w1".into());
    updated(&frames, "w2:p2", &marker(3, 1).title());
    frames.observe("pane_exited", &json!({"pane_id": "w2:p2"}));
    assert_eq!(frames.pane("w2:p2"), PaneFrames::default());
}

#[test]
fn a_wait_returns_the_first_frame_that_satisfies_it() {
    let frames = Arc::new(Frames::new("w1".into()));
    updated(&frames, "w1:p2", &marker(1, 0).title());
    let painter = Arc::clone(&frames);
    let painting = std::thread::spawn(move || {
        for frame in 2..=4 {
            updated(&painter, "w1:p2", &marker(frame, frame / 3).title());
        }
    });
    let acknowledged = frames.wait("w1:p2", GUARD, |marker| marker.acknowledged >= 1);
    painting.join().unwrap();
    let acknowledged = acknowledged.unwrap();
    assert!(acknowledged.frame >= 3, "{acknowledged:?}");
}

#[test]
fn a_wait_ends_when_the_reviewer_exits() {
    let frames = Frames::new("w1".into());
    updated(&frames, "w1:p2", &marker(1, 0).title());
    frames.observe(
        "pane_exited",
        &json!({"pane_id": "w1:p2", "workspace_id": "w1"}),
    );
    assert_eq!(
        frames.wait("w1:p2", GUARD, |marker| marker.frame > 1),
        Err(Stalled::Exited)
    );
}

#[test]
fn a_wait_ends_when_the_event_stream_ends() {
    let frames = Frames::new("w1".into());
    frames.end("closed".into());
    assert_eq!(
        frames.wait("w1:p2", GUARD, |_| true),
        Err(Stalled::Ended("closed".into()))
    );
}

#[test]
fn a_spent_guard_reports_the_latest_frame() {
    let frames = Frames::new("w1".into());
    updated(&frames, "w1:p2", &marker(1, 0).title());
    assert_eq!(
        frames.wait("w1:p2", Duration::ZERO, |marker| marker.frame > 1),
        Err(Stalled::Guard(Some(marker(1, 0))))
    );
}

#[test]
fn a_wait_for_an_agent_ends_when_herdr_detects_it() {
    let frames = Arc::new(Frames::new("w1".into()));
    let detector = Arc::clone(&frames);
    let detecting = std::thread::spawn(move || {
        detector.observe(
            "pane_agent_detected",
            &json!({
                "pane_id": "w1:p3", "workspace_id": "w1", "agent": "claude",
            }),
        );
    });
    let detected = frames.wait_agent("w1:p3", GUARD);
    detecting.join().unwrap();
    assert_eq!(detected, Ok(()));
}

#[test]
fn a_released_agent_is_no_longer_detected() {
    let frames = Frames::new("w1".into());
    for released in [false, true] {
        frames.observe(
            "pane_agent_detected",
            &json!({
                "pane_id": "w1:p3", "workspace_id": "w1", "agent": "claude", "released": released,
            }),
        );
    }
    assert!(!frames.pane("w1:p3").agent);
}

#[test]
fn a_wait_for_an_exit_ends_when_the_process_exits() {
    let frames = Arc::new(Frames::new("w1".into()));
    updated(&frames, "w1:p2", &marker(1, 0).title());
    let exiting = Arc::clone(&frames);
    let exit = std::thread::spawn(move || {
        exiting.observe(
            "pane_exited",
            &json!({"pane_id": "w1:p2", "workspace_id": "w1"}),
        );
    });
    let exited = frames.wait_exit("w1:p2", GUARD);
    exit.join().unwrap();
    assert_eq!(exited, Ok(()));
}
