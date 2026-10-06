use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use super::*;

const HOST: &str = "quiet-river-stone-lamp.trycloudflare.com";

/// A wrapper that runs the program without a parent-death signal: it drops the reviewer's
/// process ID the launcher passes and runs the rest.
fn plain_wrapper() -> agent_fork::Wrapper {
    agent_fork::Wrapper {
        program: "sh".into(),
        arguments: vec!["-c".into(), "shift; exec \"$@\"".into(), "sh".into()],
    }
}

/// A stand-in for `cloudflared` in `directory`, which runs `script` after writing its process
/// ID and its arguments to `pid` and `arguments` there. The process ID file appears whole.
fn stand_in(directory: &Path, script: &str) -> TunnelProgram {
    let program = directory.join("cloudflared");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\necho $$ > '{dir}/pid.new'\nmv '{dir}/pid.new' '{dir}/pid'\n\
             echo \"$@\" > '{dir}/arguments'\n{script}\n",
            dir = directory.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    TunnelProgram {
        program: "cloudflared".into(),
        wrapper: plain_wrapper(),
        environment: vec![(
            OsString::from("PATH"),
            std::env::join_paths(
                [directory.to_owned()]
                    .into_iter()
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap(),
        )],
        directory: directory.to_owned(),
        address_wait: Duration::from_secs(10),
    }
}

/// Prints the box with the address on the standard error, as `cloudflared` does.
fn prints_address() -> String {
    format!(
        "echo 'INF Requesting new quick Tunnel on trycloudflare.com...' >&2\n\
         echo 'INF |  https://{HOST}  |' >&2"
    )
}

fn target() -> SocketAddr {
    "127.0.0.1:8790".parse().unwrap()
}

fn start(program: &TunnelProgram) -> (QuickTunnel, mpsc::Receiver<TunnelEvent>) {
    let (sent, received) = mpsc::channel();
    let tunnel = QuickTunnel::start(program, target(), move |event| {
        let _ = sent.send(event);
    })
    .unwrap();
    (tunnel, received)
}

fn next(events: &mpsc::Receiver<TunnelEvent>) -> TunnelEvent {
    events.recv_timeout(Duration::from_secs(10)).unwrap()
}

/// Whether the stand-in in `directory`, which wrote its process ID, still runs.
fn runs(directory: &Path) -> bool {
    let pid = std::fs::read_to_string(directory.join("pid")).unwrap();
    Path::new(&format!("/proc/{}", pid.trim())).exists()
}

#[test]
fn the_tunnel_to_the_local_address_reports_its_public_host_and_stops_with_its_process() {
    let directory = tempfile::tempdir().unwrap();
    let program = stand_in(
        directory.path(),
        &format!("{}\nexec sleep 30", prints_address()),
    );

    let (tunnel, events) = start(&program);

    assert_eq!(
        next(&events),
        TunnelEvent::Opened {
            host: HOST.to_owned()
        }
    );
    let arguments = std::fs::read_to_string(directory.path().join("arguments")).unwrap();
    assert_eq!(
        arguments.trim(),
        "tunnel --no-autoupdate --url http://127.0.0.1:8790"
    );
    assert!(runs(directory.path()));
    tunnel.stop();
    assert!(!runs(directory.path()));
    // The tunnel lets go of the events once it saw the end of its process.
    assert_eq!(
        events.recv(),
        Err(mpsc::RecvError),
        "nothing is reported once stopped"
    );
}

#[test]
fn dropping_the_tunnel_ends_its_process() {
    let directory = tempfile::tempdir().unwrap();
    let program = stand_in(
        directory.path(),
        &format!("{}\nexec sleep 30", prints_address()),
    );
    let (tunnel, events) = start(&program);
    next(&events);

    drop(tunnel);

    assert!(!runs(directory.path()));
}

#[test]
fn a_missing_cloudflared_fails_at_once_and_says_how_to_install_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut program = stand_in(directory.path(), "exit 0");
    program.program = "no-such-cloudflared".into();

    let failure = QuickTunnel::start(&program, target(), |_| {})
        .err()
        .unwrap();

    assert_eq!(
        failure,
        TunnelFailure::Missing {
            program: "no-such-cloudflared".into()
        }
    );
    let line = failure.to_string();
    assert!(line.contains("no-such-cloudflared"), "{line}");
    assert!(line.contains("https://"), "how to install it: {line}");
    assert!(!line.contains('\n'));
}

#[test]
fn a_cloudflared_that_ends_before_its_address_says_why_in_one_line() {
    let directory = tempfile::tempdir().unwrap();
    let program = stand_in(
        directory.path(),
        "echo 'INF Requesting new quick Tunnel on trycloudflare.com...' >&2\n\
         echo 'ERR failed to request quick Tunnel: no such host' >&2\nexit 1",
    );

    let (_tunnel, events) = start(&program);

    let TunnelEvent::Failed(failure) = next(&events) else {
        panic!("a failure");
    };
    assert!(matches!(failure, TunnelFailure::Exited(_)), "{failure:?}");
    let line = failure.to_string();
    assert!(
        line.contains("ERR failed to request quick Tunnel: no such host"),
        "{line}"
    );
}

#[test]
fn a_cloudflared_that_prints_no_address_in_time_is_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let mut program = stand_in(directory.path(), "exec sleep 30");
    program.address_wait = Duration::ZERO;

    let (tunnel, events) = start(&program);
    // The tunnel's process, whether it is still the wrapper or already the stand-in, which
    // takes the wrapper's place: with no wait, the tunnel may stop it before the stand-in runs.
    let process = tunnel.fork.stamp();
    assert_ne!(process.started, 0, "the process was read while it ran");

    assert_eq!(
        next(&events),
        TunnelEvent::Failed(TunnelFailure::NoAddress(Duration::ZERO))
    );
    assert!(!process.is_running());
}

#[test]
fn a_tunnel_that_goes_down_after_its_address_reports_its_end() {
    let directory = tempfile::tempdir().unwrap();
    let program = stand_in(
        directory.path(),
        &format!(
            "{}\necho 'ERR connection to the edge lost' >&2\nexit 1",
            prints_address()
        ),
    );

    let (_tunnel, events) = start(&program);

    assert!(matches!(next(&events), TunnelEvent::Opened { .. }));
    let TunnelEvent::Failed(failure) = next(&events) else {
        panic!("the end");
    };
    assert!(matches!(failure, TunnelFailure::Ended(_)), "{failure:?}");
    assert!(
        failure
            .to_string()
            .contains("ERR connection to the edge lost")
    );
}
