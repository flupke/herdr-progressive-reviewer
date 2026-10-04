use super::*;

#[test]
fn the_start_time_is_read_after_a_command_name_with_spaces_and_parentheses() {
    let stat = "4242 (a (strange) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";

    assert_eq!(state_and_start_time(stat), Some(("S", 987_654)));
}

#[test]
fn this_process_runs_and_its_stamp_names_it() {
    let stamp = ProcessStamp::of(std::process::id()).unwrap();

    assert!(stamp.is_running());
    assert!(
        !ProcessStamp {
            started: stamp.started + 1,
            ..stamp
        }
        .is_running()
    );
}

#[test]
fn a_process_runs_with_an_argument_only_when_its_command_line_holds_it_whole() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let stamp = ProcessStamp::of(child.id()).unwrap();
    // The command line shows once the program is loaded, a moment after the start.
    wait_for(|| stamp.runs_with("30"));

    assert!(stamp.runs_with("30"));
    assert!(!stamp.runs_with("3"));
    child.kill().unwrap();
    // Killed but not reaped: a zombie no longer runs.
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(!stamp.is_running());
    child.wait().unwrap();
}

/// Waits up to two seconds for `ready`.
pub(crate) fn wait_for(ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !ready() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
