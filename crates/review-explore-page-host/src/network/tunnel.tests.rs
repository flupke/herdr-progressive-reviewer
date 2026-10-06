use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use review_explore_page::RoundStage;

use super::*;
use crate::test_page::{Shared, no_round, page_host, path, published, request, status, working};

const HOST: &str = "quiet-river-stone-lamp.trycloudflare.com";

/// A stand-in for `cloudflared` in a directory of its own: it writes its process ID and its
/// arguments there, prints the box with the tunnel's address on its standard error, then runs
/// until it is stopped.
struct StandIn {
    directory: tempfile::TempDir,
    program: TunnelProgram,
}

impl StandIn {
    fn new() -> Self {
        Self::printing(&format!(
            "echo 'INF |  https://{HOST}  |' >&2\nexec sleep 600"
        ))
    }

    fn printing(script: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("cloudflared");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\necho $$ > '{dir}/pid'\necho \"$@\" > '{dir}/arguments'\n{script}\n",
                dir = directory.path().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut tunnel = TunnelProgram::cloudflared(agent_fork::Wrapper {
            program: "sh".into(),
            arguments: vec!["-c".into(), "shift; exec \"$@\"".into(), "sh".into()],
        });
        tunnel.program = program.into_os_string();
        Self {
            directory,
            program: tunnel,
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    /// The local address the tunnel forwards to: its `--url`.
    fn target(&self) -> SocketAddr {
        let arguments = std::fs::read_to_string(self.file("arguments")).unwrap();
        let url = arguments.trim().rsplit(' ').next().unwrap();
        url.strip_prefix("http://").unwrap().parse().unwrap()
    }

    /// Whether the stand-in's process runs.
    fn runs(&self) -> bool {
        let pid = std::fs::read_to_string(self.file("pid")).unwrap();
        Path::new(&format!("/proc/{}", pid.trim())).exists()
    }
}

/// Waits until the tunnels of `network` closed so far stopped: their listeners closed, and
/// `cloudflared` ended.
fn stopped(network: &PageNetwork) {
    let stopping = std::mem::take(&mut network.lock().tunnels.stopping);
    for stop in stopping {
        stop.join().unwrap();
    }
}

/// Opens the tunnel of `network` with `stand_in`, and returns the states it reports.
fn open(network: &PageNetwork, stand_in: &StandIn) -> mpsc::Receiver<TunnelState> {
    let (sent, reported) = mpsc::channel();
    let report: TunnelReport = Arc::new(move |state| {
        let _ = sent.send(state);
    });
    network.open_tunnel(&stand_in.program, &report);
    reported
}

fn next(reported: &mpsc::Receiver<TunnelState>) -> TunnelState {
    reported
        .recv_timeout(Duration::from_secs(10))
        .expect("a state of the tunnel")
}

/// The public address the tunnel reported once open, after it said it was opening.
fn opened(reported: &mpsc::Receiver<TunnelState>) -> String {
    assert_eq!(next(reported), TunnelState::Opening);
    let TunnelState::Open { url } = next(reported) else {
        panic!("the tunnel's address");
    };
    url
}

fn token(url: &str) -> &str {
    url.rsplit('=').next().unwrap()
}

/// The status of a request that the tunnel forwards to `target`: for its public host name,
/// with `headers`.
fn forwarded(target: SocketAddr, method: &str, url: &str, headers: &[(&str, &str)]) -> u16 {
    let mut all = vec![("Host", HOST)];
    all.extend_from_slice(headers);
    status(target, method, path(url), &all)
}

/// The cookie that keeps the token of `url` in a browser of the tunnel's page.
fn cookie(url: &str) -> String {
    format!("explore_token_80={}", token(url))
}

/// Whether nothing listens at `address`.
fn closed(address: SocketAddr) -> bool {
    TcpStream::connect(address).is_err()
}

#[test]
fn the_tunnel_shares_the_running_round_behind_the_token_of_the_phones_qr_code() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();

    let reported = open(&shared.host.network(), &stand_in);

    let url = opened(&reported);
    assert_eq!(
        url,
        format!("https://{HOST}/?token={}", token(&shared.first))
    );
    let target = stand_in.target();
    assert!(target.ip().is_loopback(), "{target}");
    assert_eq!(forwarded(target, "GET", &url, &[]), 303);
    let response = request(
        target,
        "GET",
        "/",
        &[("Host", HOST), ("Cookie", &cookie(&url))],
        "",
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(
        response.contains(&format!("connect-src 'self' wss://{HOST}")),
        "the page's socket goes through the tunnel: {response}"
    );
}

#[test]
fn the_tunnels_page_answers_only_its_public_host_over_https() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    let url = opened(&open(&shared.host.network(), &stand_in));
    let target = stand_in.target();

    let origin = format!("https://{HOST}");
    assert_eq!(
        forwarded(
            target,
            "POST",
            "https://x/csp-report",
            &[("Origin", &origin)]
        ),
        204
    );
    for origin in [format!("http://{HOST}"), "https://foreign.example".into()] {
        assert_eq!(
            forwarded(
                target,
                "POST",
                "https://x/csp-report",
                &[("Origin", &origin)]
            ),
            403,
            "{origin}"
        );
    }
    for host in [
        "rebound.example".to_owned(),
        target.to_string(),
        shared.address.to_string(),
    ] {
        assert_eq!(
            status(target, "GET", path(&url), &[("Host", &host)]),
            403,
            "{host}"
        );
    }
    let lan = shared.address.to_string();
    assert_eq!(
        status(
            shared.address,
            "GET",
            path(&shared.first),
            &[("Host", &lan), ("Origin", &origin)]
        ),
        403,
        "the network listener does not answer the tunnel's origin"
    );
}

#[test]
fn the_tunnel_never_opens_the_page_of_this_machine() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    opened(&open(&shared.host.network(), &stand_in));

    assert_eq!(
        forwarded(stand_in.target(), "GET", shared.host.url(), &[]),
        403
    );
    assert_eq!(
        forwarded(stand_in.target(), "POST", "https://x/mcp", &[]),
        404
    );
}

#[test]
fn the_tunnels_page_holds_its_socket_with_the_tunnels_origin() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::{Message, client};

    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    let url = opened(&open(&shared.host.network(), &stand_in));
    let mut upgrade = format!("ws://{HOST}/ws").into_client_request().unwrap();
    let headers = upgrade.headers_mut();
    headers.insert("cookie", cookie(&url).parse().unwrap());
    headers.insert("origin", format!("https://{HOST}").parse().unwrap());
    let stream = TcpStream::connect(stand_in.target()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let (mut socket, _) = client(upgrade, stream).unwrap();

    let Message::Text(view) = socket.read().unwrap() else {
        panic!("the round's view");
    };
    let view: serde_json::Value = serde_json::from_str(&view).unwrap();
    assert_eq!(view["params"]["view"]["reset"], "r1", "{view}");
}

#[test]
fn turning_the_tunnel_off_closes_its_page_and_ends_cloudflared() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    let network = shared.host.network();
    let reported = open(&network, &stand_in);
    opened(&reported);
    let target = stand_in.target();

    network.close_tunnel();

    assert_eq!(next(&reported), TunnelState::Off);
    stopped(&network);
    assert!(!stand_in.runs());
    assert!(closed(target));
    assert_eq!(
        shared.load(&shared.first),
        200,
        "the phone's page stays open"
    );
}

#[test]
fn a_new_round_or_a_reset_stops_the_tunnel() {
    for next_round in [Some("r2"), None] {
        let shared = Shared::start();
        shared.start_round("r1");
        let stand_in = StandIn::new();
        let network = shared.host.network();
        let reported = open(&network, &stand_in);
        let url = opened(&reported);
        let target = stand_in.target();

        match next_round {
            Some(id) => shared.round.publish(Some(published(id)), working()),
            None => shared.round.publish(None, no_round()),
        }

        assert_eq!(next(&reported), TunnelState::Off, "{next_round:?}");
        stopped(&network);
        assert!(!stand_in.runs(), "{next_round:?}");
        assert!(closed(target), "{next_round:?}");
        assert_eq!(shared.load(&url), 403, "the round's token ended");
    }
}

#[test]
fn the_tunnel_ends_with_the_page_host() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    opened(&open(&shared.host.network(), &stand_in));

    drop(shared);

    assert!(!stand_in.runs());
}

#[test]
fn a_tunnel_opens_only_for_a_running_round() {
    let shared = Shared::start();
    let stand_in = StandIn::new();

    let reported = open(&shared.host.network(), &stand_in);

    let TunnelState::Failed(reason) = next(&reported) else {
        panic!("no tunnel without a round");
    };
    assert!(!reason.is_empty());
    assert!(!stand_in.file("pid").exists(), "cloudflared did not start");
    assert_eq!(shared.load(&shared.first), 200);
}

#[test]
fn a_missing_cloudflared_is_reported_in_one_line_with_how_to_install_it() {
    let shared = Shared::start();
    shared.start_round("r1");
    let mut stand_in = StandIn::new();
    stand_in.program.program = "no-such-cloudflared".into();

    let reported = open(&shared.host.network(), &stand_in);

    let TunnelState::Failed(reason) = next(&reported) else {
        panic!("a failure");
    };
    assert!(reason.contains("no-such-cloudflared"), "{reason}");
    assert!(reason.contains("https://"), "{reason}");
    assert!(!reason.contains('\n'));
    let installed = StandIn::new();
    let again = open(&shared.host.network(), &installed);
    opened(&again);
}

#[test]
fn a_cloudflared_that_fails_closes_the_tunnels_page_and_says_why() {
    let shared = Shared::start();
    shared.start_round("r1");
    // The stand-in fails once the test writes to its FIFO.
    let fifo = tempfile::tempdir().unwrap();
    let fail = fifo.path().join("fail");
    nix::unistd::mkfifo(&fail, nix::sys::stat::Mode::S_IRWXU).unwrap();
    let stand_in = StandIn::printing(&format!(
        "echo 'INF |  https://{HOST}  |' >&2\nread line < '{}'\necho 'ERR lost the edge' >&2\nexit 1",
        fail.display()
    ));
    let network = shared.host.network();
    let reported = open(&network, &stand_in);
    let url = opened(&reported);
    let target = stand_in.target();
    assert_eq!(forwarded(target, "GET", &url, &[]), 303, "the page is open");

    std::fs::write(&fail, "\n").unwrap();

    let TunnelState::Failed(reason) = next(&reported) else {
        panic!("the tunnel went down");
    };
    assert!(reason.contains("ERR lost the edge"), "{reason}");
    stopped(&network);
    assert!(closed(target));
}

#[test]
fn without_the_network_listener_the_tunnel_takes_a_round_token_of_its_own() {
    let state = tempfile::tempdir().unwrap();
    let (round, host) = page_host(state.path());
    round.publish(Some(published("r1")), working());
    let stand_in = StandIn::new();
    let network = host.network();

    let reported = open(&network, &stand_in);

    let url = opened(&reported);
    assert!(!host.url().contains(token(&url)));
    assert_eq!(forwarded(stand_in.target(), "GET", &url, &[]), 303);
    network.close_tunnel();
    assert_eq!(next(&reported), TunnelState::Off);
    let again = StandIn::new();
    let second = opened(&open(&network, &again));
    assert_ne!(
        token(&second),
        token(&url),
        "the tokens closed with the tunnel"
    );
}

#[test]
fn a_second_open_while_the_tunnel_runs_says_where_it_stands_and_starts_nothing() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    let network = shared.host.network();
    let reported = open(&network, &stand_in);
    let url = opened(&reported);

    let second = StandIn::new();
    let again = open(&network, &second);

    assert_eq!(next(&again), TunnelState::Open { url });
    assert!(!second.file("pid").exists());
    assert!(stand_in.runs());
}

#[test]
fn a_stage_of_the_same_round_keeps_the_tunnel() {
    let shared = Shared::start();
    shared.start_round("r1");
    let stand_in = StandIn::new();
    let network = shared.host.network();
    let (kept, watched) = mpsc::channel();
    network.lock().tunnels.watcher = Watcher(Some(kept));
    let reported = open(&network, &stand_in);
    let url = opened(&reported);
    let interrupted = RoundStage::Interrupted {
        request: None,
        attempt: None,
        interruption: review_explore_page::Interruption::Stopped,
        answer: None,
    };

    shared
        .round
        .publish(Some(published("r1")), interrupted.clone());

    // The task that ends the tunnel with its round saw the new stage and kept the tunnel.
    while watched.recv_timeout(Duration::from_secs(10)).unwrap() != interrupted {}
    assert!(reported.try_recv().is_err(), "nothing more reported");
    assert_eq!(forwarded(stand_in.target(), "GET", &url, &[]), 303);
}
