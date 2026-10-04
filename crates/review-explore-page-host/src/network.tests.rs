use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use herdr_client::protocol::WorkspaceId;
use review_explore::{RoundOverview, TabTitle};
use review_explore_page::{
    CommandRefusal, CommandSender, PageCommand, PageRound, PublishedRound, Recovery,
    RoundPublisher, RoundStage,
};

use super::*;
use crate::{PageDirectory, PageHost};

/// A page host with its page shared on the loopback interface, which stands in for a network
/// interface, and the addresses it announces.
struct Shared {
    round: Arc<RoundPublisher>,
    host: PageHost,
    address: SocketAddr,
    announced: mpsc::Receiver<String>,
    /// The address the host announced first, for the start screen, before any round started.
    first: String,
    _state: tempfile::TempDir,
}

impl Shared {
    fn start() -> Self {
        let state = tempfile::tempdir().unwrap();
        let round = Arc::new(RoundPublisher::default());
        // The round's owner: it takes a Reset, and refuses every other command.
        let owner = Arc::downgrade(&round);
        let commands = CommandSender::new(move |command, reply| match command {
            PageCommand::Recover(Recovery::Reset { .. }) => {
                if let Some(round) = owner.upgrade() {
                    round.publish(None, no_round());
                }
                reply.send(Ok(()));
            }
            _ => reply.send(Err(CommandRefusal::Stale)),
        });
        let host = PageHost::start(
            PageRound::new(round.subscribe(), commands),
            &PageDirectory::new(state.path()),
            &WorkspaceId("w1".into()),
            std::path::Path::new("/repositories/drafts"),
        )
        .unwrap();
        let (address, announced) = share_on_loopback(&host);
        let first = announced
            .recv_timeout(Duration::from_secs(5))
            .expect("the address of the page before any round");
        Self {
            round,
            host,
            address,
            announced,
            first,
            _state: state,
        }
    }

    /// Starts the round `id`, which replaces a running round, and returns the address of its
    /// page that the host announced.
    fn replace_round(&self, id: &str) -> String {
        self.round.publish(Some(published(id)), working());
        self.next_announcement()
    }

    /// Starts the round `id` while no round runs, and waits for the host to give it the token
    /// of the address it announced for the start screen.
    fn start_round(&self, id: &str) {
        self.round.publish(Some(published(id)), working());
        assert_eq!(
            self.next_announcement(),
            self.first,
            "the round keeps the address"
        );
    }

    fn next_announcement(&self) -> String {
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

    /// The status of a load of the page by a browser whose cookie holds the token of `url`,
    /// under the name the page gives it on its port.
    fn load(&self, url: &str) -> u16 {
        let token = url.rsplit('=').next().unwrap();
        let cookie = format!("explore_token_{}={token}", self.address.port());
        self.status("GET", "/", &[("Cookie", &cookie)])
    }
}

/// Shares the page of `host` on a free port of the loopback interface, and returns the address
/// it is served at and the addresses the host announces for it.
fn share_on_loopback(host: &PageHost) -> (SocketAddr, mpsc::Receiver<String>) {
    let listener = NetworkListener::bind(&loopback(0)).unwrap().unwrap();
    let address = listener.address();
    let (announce, announced) = mpsc::channel();
    host.network().share(listener, move |url| {
        let _ = announce.send(url.to_owned());
    });
    (address, announced)
}

/// The round `id`, with nothing more to publish.
fn published(id: &str) -> PublishedRound<'_> {
    PublishedRound {
        id,
        design: None,
        cancellable: None,
        earlier: false,
        overview: &OVERVIEW,
    }
}

/// A round's overview, which these tests do not look at.
static OVERVIEW: RoundOverview = RoundOverview {
    rail: Vec::new(),
    decisions: Vec::new(),
    earlier: Vec::new(),
    title: TabTitle::AgentWorking,
};

fn no_round() -> RoundStage {
    RoundStage::NoRound {
        start: "start".into(),
    }
}

fn working() -> RoundStage {
    RoundStage::AgentWorking {
        request: "turn".into(),
    }
}

fn loopback(first_port: u16) -> NetworkAccess {
    NetworkAccess {
        enabled: true,
        interface: Some(LOOPBACK.into()),
        first_port,
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
    let response = request(address, method, target, headers, "");
    response
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {response:?}"))
}

/// The response, head and body, to a request for `target` with `body`.
fn request(
    address: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = format!("{method} {target} HTTP/1.1\r\nConnection: close\r\n");
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    let _ = write!(request, "Content-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn the_page_opens_on_the_network_before_any_round_and_stays_with_the_round_it_starts() {
    let shared = Shared::start();
    assert!(
        shared
            .first
            .starts_with(&format!("http://{}/?token=", shared.address)),
        "{}",
        shared.first
    );
    assert_eq!(shared.open(&shared.first), 303);
    assert_eq!(shared.load(&shared.first), 200);

    shared.start_round("r1");
    assert_eq!(
        shared.load(&shared.first),
        200,
        "the page that started the round"
    );
    shared.round.publish(
        Some(published("r1")),
        RoundStage::Interrupted {
            request: None,
            attempt: None,
            interruption: review_explore_page::Interruption::Stopped,
        },
    );
    assert_eq!(shared.open(&shared.first), 303, "the round keeps its token");
}

#[test]
fn a_round_that_replaces_another_opens_behind_a_token_of_its_own() {
    let shared = Shared::start();
    shared.start_round("r1");

    let second = shared.replace_round("r2");

    assert_ne!(second, shared.first);
    assert_eq!(
        shared.open(&shared.first),
        403,
        "the token of a closed round"
    );
    assert_eq!(shared.load(&shared.first), 403);
    assert_eq!(shared.open(&second), 303);
}

#[test]
fn a_reset_closes_the_rounds_page_and_opens_a_new_one_for_the_next_round() {
    let shared = Shared::start();
    shared.start_round("r1");

    shared.round.publish(None, no_round());

    let next = shared.next_announcement();
    assert_ne!(next, shared.first);
    assert_eq!(shared.open(&shared.first), 403);
    assert_eq!(shared.load(&shared.first), 403);
    assert_eq!(shared.load(&next), 200);
    shared.round.publish(Some(published("r2")), working());
    assert_eq!(shared.next_announcement(), next);
    assert_eq!(
        shared.load(&next),
        200,
        "the page that started the next round"
    );
}

#[test]
fn a_reset_on_the_network_page_hands_that_page_the_start_screens_token() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::{Message, client};

    let shared = Shared::start();
    shared.start_round("r1");
    let token = shared.first.rsplit('=').next().unwrap();
    let host = shared.address.to_string();
    let mut upgrade = format!("ws://{host}/ws").into_client_request().unwrap();
    let headers = upgrade.headers_mut();
    let cookie = format!("explore_token_{}={token}", shared.address.port());
    headers.insert("cookie", cookie.parse().unwrap());
    headers.insert("origin", format!("http://{host}").parse().unwrap());
    let stream = TcpStream::connect(shared.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (mut socket, _) = client(upgrade, stream).unwrap();

    let reset = r#"{"id":1,"method":"reset","params":{"round":"r1"}}"#;
    socket.send(Message::Text(reset.into())).unwrap();
    let reply = loop {
        let Message::Text(text) = socket.read().unwrap() else {
            continue;
        };
        let message: serde_json::Value = serde_json::from_str(&text).unwrap();
        if message["id"] == 1 {
            break message;
        }
    };

    let next = shared.next_announcement();
    assert_ne!(next, shared.first);
    assert_eq!(
        reply["result"]["reopen"].as_str(),
        next.rsplit('=').next(),
        "the page opens with the start screen's token"
    );
    assert_eq!(shared.load(&next), 200, "the page shows the start screen");
    assert_eq!(shared.load(&shared.first), 403, "the round's token ended");
}

#[test]
fn the_network_page_answers_only_the_address_it_gave_out() {
    let shared = Shared::start();
    shared.start_round("r1");
    let target = path(&shared.first);

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

    let listener = NetworkListener::bind(&loopback(port)).unwrap().unwrap();

    let chosen = listener.address().port();
    assert!(chosen > port && chosen < port + PORTS_TRIED, "{chosen}");
}

#[test]
fn an_unknown_interface_is_an_error() {
    let mut access = loopback(0);
    access.set_interface("no-such-interface0");

    assert!(NetworkListener::bind(&access).is_err());
}

#[test]
fn network_access_turned_off_listens_nowhere() {
    let mut access = loopback(0);
    access.enabled = false;

    assert!(NetworkListener::bind(&access).unwrap().is_none());
}

#[test]
fn unsharing_closes_the_page_on_the_network_and_announces_nothing_more() {
    let shared = Shared::start();
    shared.start_round("r1");

    shared.host.network().unshare();

    assert!(review_test_support::refuses_connections(shared.address));
    shared.round.publish(Some(published("r2")), working());
    assert!(
        shared
            .announced
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "no address after the page left the network"
    );
}

#[test]
fn sharing_again_moves_the_page_to_the_new_listener() {
    let shared = Shared::start();
    shared.start_round("r1");

    let (address, announced) = share_on_loopback(&shared.host);

    let url = announced
        .recv_timeout(Duration::from_secs(5))
        .expect("the address on the new listener");
    assert!(
        url.starts_with(&format!("http://{address}/?token=")),
        "{url}"
    );
    assert!(review_test_support::refuses_connections(shared.address));
    let host = address.to_string();
    assert_eq!(status(address, "GET", path(&url), &[("Host", &host)]), 303);
}

#[test]
fn the_port_of_the_page_taken_off_the_network_is_free_at_once() {
    let shared = Shared::start();

    shared.host.network().unshare();

    let again = NetworkListener::bind(&loopback(shared.address.port()))
        .unwrap()
        .unwrap();
    assert_eq!(again.address(), shared.address);
}
