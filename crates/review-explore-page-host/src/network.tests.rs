use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use herdr_client::protocol::WorkspaceId;
use review_explore_page::{RoundPublisher, RoundStage};

use super::*;
use crate::{PageDirectory, PageHost};

/// A page host with its page shared on the loopback interface, which stands in for a network
/// interface, and the addresses it announces.
struct Shared {
    round: RoundPublisher,
    host: PageHost,
    address: SocketAddr,
    announced: mpsc::Receiver<Option<String>>,
    _state: tempfile::TempDir,
}

impl Shared {
    fn start() -> Self {
        let state = tempfile::tempdir().unwrap();
        let round = RoundPublisher::default();
        let host = PageHost::start(
            crate::tests::page_round(&round),
            &PageDirectory::new(state.path()),
            &WorkspaceId("w1".into()),
        )
        .unwrap();
        let listener = loopback(0).listen().unwrap().unwrap();
        let address = listener.address();
        let (announce, announced) = mpsc::channel();
        host.share(listener, move |url| {
            let _ = announce.send(url.map(str::to_owned));
        });
        Self {
            round,
            host,
            address,
            announced,
            _state: state,
        }
    }

    /// Starts the round `id`, and returns the address of its page that the host announced.
    fn start_round(&self, id: &str) -> String {
        self.round.publish(Some(id), RoundStage::AgentWorking);
        self.next_announcement()
            .expect("the address of the round's page")
    }

    fn next_announcement(&self) -> Option<String> {
        self.announced
            .recv_timeout(Duration::from_secs(5))
            .expect("an announcement")
    }

    /// The status of a request for `target` with the network address as its host.
    fn status(&self, method: &str, target: &str, headers: &[(&str, &str)]) -> u16 {
        let host = self.address.to_string();
        let mut all = vec![("Host", host.as_str())];
        all.extend_from_slice(headers);
        status(self.address, method, target, &all)
    }

    /// The status of a request for the path and query of `url`.
    fn open(&self, url: &str) -> u16 {
        self.status("GET", path(url), &[])
    }
}

fn loopback(port: u16) -> NetworkAccess {
    NetworkAccess::On {
        interface: Some(LOOPBACK.into()),
        port,
    }
}

#[cfg(target_os = "linux")]
const LOOPBACK: &str = "lo";
#[cfg(not(target_os = "linux"))]
const LOOPBACK: &str = "lo0";

fn path(url: &str) -> &str {
    let rest = url.strip_prefix("http://").unwrap();
    &rest[rest.find('/').unwrap()..]
}

fn status(address: SocketAddr, method: &str, target: &str, headers: &[(&str, &str)]) -> u16 {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = format!("{method} {target} HTTP/1.1\r\nConnection: close\r\n");
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    request.push_str("Content-Length: 0\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {response:?}"))
}

#[test]
fn each_round_opens_on_the_network_behind_a_token_of_its_own() {
    let shared = Shared::start();

    let first = shared.start_round("r1");
    assert!(
        first.starts_with(&format!("http://{}/?token=", shared.address)),
        "{first}"
    );
    assert_eq!(shared.open(&first), 303);
    let cookie = format!("explore_token={}", first.rsplit('=').next().unwrap());
    assert_eq!(shared.status("GET", "/", &[("Cookie", &cookie)]), 200);

    shared
        .round
        .publish(Some("r1"), RoundStage::Interrupted { failure: None });
    assert_eq!(shared.open(&first), 303, "the round keeps its token");

    let second = shared.start_round("r2");
    assert_ne!(second, first);
    assert_eq!(shared.open(&first), 403, "the token of a closed round");
    assert_eq!(shared.status("GET", "/", &[("Cookie", &cookie)]), 403);
    assert_eq!(shared.open(&second), 303);
}

#[test]
fn a_reset_closes_the_rounds_page_on_the_network() {
    let shared = Shared::start();
    let url = shared.start_round("r1");

    shared.round.publish(None, RoundStage::NoRound);

    assert_eq!(shared.next_announcement(), None);
    assert_eq!(shared.open(&url), 403);
}

#[test]
fn the_network_page_answers_only_the_address_it_gave_out() {
    let shared = Shared::start();
    let url = shared.start_round("r1");
    let target = path(&url);

    for host in [
        "rebound.example",
        &format!("localhost:{}", shared.address.port()),
    ] {
        assert_eq!(
            status(shared.address, "GET", target, &[("Host", host)]),
            403,
            "{host}"
        );
    }
    let origin = format!("http://{}", shared.address);
    assert_eq!(
        shared.status("POST", "/csp-report", &[("Origin", &origin)]),
        204
    );
    assert_eq!(
        shared.status(
            "POST",
            "/csp-report",
            &[("Origin", "http://foreign.example")]
        ),
        403
    );
}

#[test]
fn the_network_listener_serves_neither_mcp_nor_the_page_of_this_machine() {
    let shared = Shared::start();
    shared.start_round("r1");

    assert_eq!(shared.status("POST", "/mcp", &[]), 404);
    assert_eq!(shared.open(shared.host.url()), 403);
}

#[test]
fn a_taken_port_moves_the_page_to_the_next_free_one() {
    let taken = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port();

    let listener = loopback(port).listen().unwrap().unwrap();

    let chosen = listener.address().port();
    assert!(chosen > port && chosen < port + PORTS_TRIED, "{chosen}");
}

#[test]
fn the_settings_turn_the_network_off_and_pick_the_interface_and_port() {
    let parse = |network: Option<&str>, interface: Option<&str>, port: Option<&str>| {
        NetworkAccess::from_settings(network, interface, port)
    };
    assert_eq!(
        parse(None, None, None),
        Ok(NetworkAccess::On {
            interface: None,
            port: DEFAULT_PORT
        })
    );
    assert_eq!(
        parse(Some("off"), Some("wlan0"), None),
        Ok(NetworkAccess::Off)
    );
    assert_eq!(
        parse(Some("on"), Some("wlan0"), Some("9000")),
        Ok(NetworkAccess::On {
            interface: Some("wlan0".into()),
            port: 9000
        })
    );
    assert_eq!(
        parse(None, Some(""), None),
        Ok(NetworkAccess::On {
            interface: None,
            port: DEFAULT_PORT
        })
    );
    assert!(parse(Some("maybe"), None, None).is_err());
    assert!(parse(None, None, Some("port")).is_err());
}

#[test]
fn an_unknown_interface_is_an_error() {
    let access = NetworkAccess::On {
        interface: Some("no-such-interface0".into()),
        port: 0,
    };

    assert!(access.listen().is_err());
}

#[test]
fn network_access_turned_off_listens_nowhere() {
    assert!(NetworkAccess::Off.listen().unwrap().is_none());
}
