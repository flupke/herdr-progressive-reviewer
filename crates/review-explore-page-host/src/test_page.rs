//! What the tests of the page beyond this machine share: rounds to publish, and plain HTTP
//! requests.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use herdr_client::protocol::WorkspaceId;
use review_explore::{RoundOverview, TabTitle};
use review_explore_page::{
    CommandRefusal, CommandSender, PageCommand, PageRound, PublishedRound, Recovery,
    RoundPublisher, RoundStage,
};

use crate::{NetworkAccess, NetworkListener, PageDirectory, PageHost};

/// The round `id`, with nothing more to publish.
pub(crate) fn published(id: &str) -> PublishedRound<'_> {
    PublishedRound {
        id,
        review_unit: &REVIEW,
        design: None,
        changed_files: 0,
        cancellable: None,
        earlier: false,
        overview: &OVERVIEW,
        earlier_citations: &[],
    }
}

/// The review of every round, which these tests do not look at.
static REVIEW: std::sync::LazyLock<review_types::ReviewUnit> =
    std::sync::LazyLock::new(|| "review".into());

/// A round's overview, which these tests do not look at.
static OVERVIEW: RoundOverview = RoundOverview {
    rail: Vec::new(),
    decisions: Vec::new(),
    earlier: Vec::new(),
    title: TabTitle::AgentWorking,
};

pub(crate) fn no_round() -> RoundStage {
    RoundStage::NoRound {
        start: "start".into(),
    }
}

pub(crate) fn working() -> RoundStage {
    RoundStage::AgentWorking {
        request: "turn".into(),
        sent_at_ms: None,
        answer: None,
    }
}

pub(crate) fn loopback(first_port: u16) -> NetworkAccess {
    NetworkAccess {
        enabled: true,
        interface: Some(LOOPBACK.into()),
        first_port,
    }
}

#[cfg(target_os = "linux")]
pub(crate) const LOOPBACK: &str = "lo";
#[cfg(not(target_os = "linux"))]
pub(crate) const LOOPBACK: &str = "lo0";

/// The path and query of `url`.
pub(crate) fn path(url: &str) -> &str {
    let rest = url.split_once("://").unwrap().1;
    &rest[rest.find('/').unwrap()..]
}

pub(crate) fn status(
    address: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
) -> u16 {
    let response = request(address, method, target, headers, "");
    response
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status in {response:?}"))
}

/// The response, head and body, to a request for `target` with `body`.
pub(crate) fn request(
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

/// A page host of the round that `RoundPublisher` publishes, whose owner takes a Reset and
/// refuses every other command, with its records under `state`.
pub(crate) fn page_host(state: &std::path::Path) -> (Arc<RoundPublisher>, PageHost) {
    let round = Arc::new(RoundPublisher::default());
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
        &PageDirectory::new(state),
        &WorkspaceId("w1".into()),
        std::path::Path::new("/repositories/drafts"),
    )
    .unwrap();
    (round, host)
}

/// A page host with its page shared on the loopback interface, which stands in for a network
/// interface, and the addresses it announces.
pub(crate) struct Shared {
    pub(crate) round: Arc<RoundPublisher>,
    pub(crate) host: PageHost,
    pub(crate) address: SocketAddr,
    pub(crate) announced: mpsc::Receiver<String>,
    /// The address the host announced first, for the start screen, before any round started.
    pub(crate) first: String,
    _state: tempfile::TempDir,
}

impl Shared {
    pub(crate) fn start() -> Self {
        let state = tempfile::tempdir().unwrap();
        let (round, host) = page_host(state.path());
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
    pub(crate) fn replace_round(&self, id: &str) -> String {
        self.round.publish(Some(published(id)), working());
        self.next_announcement()
    }

    /// Starts the round `id` while no round runs, and waits for the host to give it the token
    /// of the address it announced for the start screen.
    pub(crate) fn start_round(&self, id: &str) {
        self.round.publish(Some(published(id)), working());
        assert_eq!(
            self.next_announcement(),
            self.first,
            "the round keeps the address"
        );
    }

    pub(crate) fn next_announcement(&self) -> String {
        self.announced
            .recv_timeout(Duration::from_secs(5))
            .expect("an announcement")
    }

    /// The status of a request for `target` with the network address as its host.
    pub(crate) fn status(&self, method: &str, target: &str, headers: &[(&str, &str)]) -> u16 {
        let host = self.address.to_string();
        let mut all = vec![("Host", host.as_str())];
        all.extend_from_slice(headers);
        status(self.address, method, target, &all)
    }

    /// The status of a request for the path and query of `url`.
    pub(crate) fn open(&self, url: &str) -> u16 {
        self.status("GET", path(url), &[])
    }

    /// The status of a load of the page by a browser whose cookie holds the token of `url`,
    /// under the name the page gives it on its port.
    pub(crate) fn load(&self, url: &str) -> u16 {
        let token = url.rsplit('=').next().unwrap();
        let cookie = format!("explore_token_{}={token}", self.address.port());
        self.status("GET", "/", &[("Cookie", &cookie)])
    }
}

/// Shares the page of `host` on a free port of the loopback interface, and returns the address
/// it is served at and the addresses the host announces for it.
pub(crate) fn share_on_loopback(host: &PageHost) -> (SocketAddr, mpsc::Receiver<String>) {
    let listener = NetworkListener::bind(&loopback(0)).unwrap().unwrap();
    let address = listener.address();
    let (announce, announced) = mpsc::channel();
    host.network().share(listener, move |url| {
        let _ = announce.send(url.to_owned());
    });
    (address, announced)
}
