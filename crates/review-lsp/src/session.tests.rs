use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{AnalyzerProcess, ProcessCommand};

#[test]
fn panic_kills_and_reaps_the_child_process() {
    let mut pid = 0;
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let process = AnalyzerProcess(ProcessCommand::new("sleep").arg("30").spawn().unwrap());
        pid = process.0.id();
        panic!("test panic");
    }));

    assert!(panic.is_err());
    let output = ProcessCommand::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .unwrap();
    assert!(output.stdout.is_empty(), "child process {pid} survived");
}
