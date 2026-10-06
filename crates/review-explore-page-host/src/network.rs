//! The page on the network, for a phone or a tablet: served on the address of one network
//! interface, behind a new token for each round and for the start screen before it. The
//! reviewer's [`NetworkAccess`] settings say whether, and on which interface and port. The
//! tunnel that shares the running round (`crate::tunnel`) takes the same tokens.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use review_explore_page::{ExplorePage, Hosts, PageFiles, PageRound, Rounds, Token};
pub use review_explore_page_settings::NetworkAccess;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::tunnel::TunnelShare;

/// How many ports, from the first one, the page tries: a firewall rule can open them all.
const PORTS_TRIED: u16 = 10;

/// How long taking the page off the network waits for its listener to close.
pub(crate) const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// The page on the network of one [`PageHost`](crate::PageHost), shared on one listener at a
/// time, and over one [tunnel](PageNetwork::open_tunnel) at a time, until the host drops. Clones
/// share it.
#[derive(Clone)]
pub struct PageNetwork {
    pub(crate) round: PageRound,
    /// The runtime of the page's thread.
    pub(crate) runtime: Handle,
    pub(crate) shares: Arc<Mutex<Shares>>,
}

/// What serves the page beyond this machine: the listener on a network interface, the tunnel,
/// and the round tokens both take.
#[derive(Default)]
pub(crate) struct Shares {
    /// The tokens, while the listener or the tunnel serves the page.
    tokens: Option<TokenRenewal>,
    listener: Option<NetworkShare>,
    pub(crate) tunnel: Option<TunnelShare>,
    /// How many tunnels were opened: each tunnel's number, so that what a tunnel closed since
    /// reports changes nothing.
    pub(crate) tunnels: u64,
    /// The tunnels being stopped, away from the page's thread and from the reviewer's.
    pub(crate) stopping: Vec<thread::JoinHandle<()>>,
}

impl Shares {
    /// The tokens of the page beyond this machine, made with the task that renews them when
    /// neither the listener nor the tunnel serves the page yet.
    pub(crate) fn tokens(&mut self, round: &PageRound, runtime: &Handle) -> RoundTokens {
        self.tokens
            .get_or_insert_with(|| TokenRenewal::start(round, runtime))
            .tokens
            .clone()
    }

    /// Closes the tokens once neither the listener nor the tunnel serves the page: they open
    /// nothing any more, and the next share makes new ones.
    pub(crate) fn release_tokens(&mut self) {
        if self.listener.is_none() && self.tunnel.is_none() {
            self.tokens = None;
        }
    }
}

impl PageNetwork {
    pub(crate) fn new(round: PageRound, runtime: Handle) -> Self {
        Self {
            round,
            runtime,
            shares: Arc::default(),
        }
    }

    /// Serves the page on `listener`, a network interface's, in place of any earlier listener,
    /// on the page's thread. Each round gets a new token there, and the token of a round that
    /// is no longer running opens nothing. While no round runs, the page has a token for its
    /// start screen, which the round started next keeps. `announce` receives the address of
    /// the page each time its token or the token's round changes. The page that resets a
    /// round receives the start screen's token, so that it can start the next round. The
    /// tunnel, while it runs, takes the same tokens.
    pub fn share(
        &self,
        listener: NetworkListener,
        announce: impl Fn(&str) + Send + Sync + 'static,
    ) {
        self.unshare();
        let address = listener.address();
        let mut shares = self.lock();
        let tokens = shares.tokens(&self.round, &self.runtime);
        let page = ExplorePage::new(
            tokens.clone(),
            Hosts::network(address),
            PageFiles::embedded(),
            |_| {},
        );
        let app = page.into_router(axum::Router::new());
        let listener = listener.into_std();
        let server = self.runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            let _ = axum::serve(listener, app).await;
        });
        tokens.announce_to(Some(Announcer {
            address,
            announce: Box::new(announce),
        }));
        shares.listener = Some(NetworkShare { tokens, server });
    }

    /// Stops serving the page on the network interface: no address is announced after this
    /// returns, and the listener is closed by then (it waits for the page's thread up to a
    /// second). Its tokens open nothing any more, unless the tunnel still serves the page with
    /// them.
    pub fn unshare(&self) {
        let mut shares = self.lock();
        drop(shares.listener.take());
        shares.release_tokens();
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Shares> {
        self.shares.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The tokens of the page beyond this machine, and the task that renews them as rounds change,
/// until it drops.
struct TokenRenewal {
    tokens: RoundTokens,
    task: JoinHandle<()>,
}

impl TokenRenewal {
    fn start(round: &PageRound, runtime: &Handle) -> Self {
        let tokens = RoundTokens::new(round.clone());
        let mut stages = round.stages().clone();
        let renewing = tokens.clone();
        let task = runtime.spawn(async move {
            loop {
                renewing.renew();
                if !stages.changed().await {
                    return;
                }
            }
        });
        Self { tokens, task }
    }
}

impl Drop for TokenRenewal {
    /// Closes the tokens, which open nothing any more, and stops renewing them.
    fn drop(&mut self) {
        self.tokens.close();
        self.task.abort();
    }
}

/// The page served on one listener of a network interface, until it drops.
struct NetworkShare {
    tokens: RoundTokens,
    /// The server, which owns the listener.
    server: JoinHandle<()>,
}

impl Drop for NetworkShare {
    /// Stops announcing the page's address, then stops the server and waits, briefly, for the
    /// page's thread to drop it: the listener is closed once this returns, so that the next
    /// listener can take its port.
    fn drop(&mut self) {
        self.tokens.announce_to(None);
        self.server.abort();
        wait_for_end(&self.server);
    }
}

/// Waits, at most [`STOP_TIMEOUT`], for the page's thread to drop the aborted `task`.
pub(crate) fn wait_for_end(task: &JoinHandle<()>) {
    let deadline = Instant::now() + STOP_TIMEOUT;
    while !task.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
}

/// A listener on a network interface, for the page on the network.
pub struct NetworkListener {
    listener: TcpListener,
    address: SocketAddr,
}

impl NetworkListener {
    /// Listens on the address of the interface `access` names, at the first free port of the
    /// [`PORTS_TRIED`] from its first port; `None` when network access is off.
    pub fn bind(access: &NetworkAccess) -> io::Result<Option<Self>> {
        if !access.enabled {
            return Ok(None);
        }
        let ip = match &access.interface {
            Some(name) => interface_address(name)?,
            None => default_address()?,
        };
        let first = access.first_port;
        let ports = if first == 0 {
            0..=0
        } else {
            first..=first.saturating_add(PORTS_TRIED - 1)
        };
        let mut last = None;
        for port in ports {
            match TcpListener::bind((ip, port)) {
                Ok(listener) => {
                    listener.set_nonblocking(true)?;
                    let address = listener.local_addr()?;
                    return Ok(Some(Self { listener, address }));
                }
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("no port to try")))
    }

    /// The address the page is served at.
    pub(crate) fn address(&self) -> SocketAddr {
        self.address
    }

    fn into_std(self) -> TcpListener {
        self.listener
    }
}

/// The first IPv4 address of the interface `name`.
fn interface_address(name: &str) -> io::Result<Ipv4Addr> {
    nix::ifaddrs::getifaddrs()?
        .filter(|interface| interface.interface_name == name)
        .find_map(|interface| Some(interface.address?.as_sockaddr_in()?.ip()))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("the interface {name} has no IPv4 address"),
            )
        })
}

/// The address of the interface of the route to the internet. Connecting a UDP socket sends
/// nothing: it only picks the interface.
fn default_address() -> io::Result<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9))?;
    match socket.local_addr()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Ok(ip),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "this machine has no network address",
        )),
    }
}

/// The round the page beyond this machine shows, behind a token that changes with each round.
/// While no round runs, the page has a token too, for the start screen: the round that starts
/// next keeps it, so the page that started the round stays connected to it. The token of a
/// round that is no longer running opens nothing, even before the next token is made. Each new
/// token or round of the token is announced with the address of the page on the network
/// interface, while it is served there. Clones share the token.
#[derive(Clone)]
pub(crate) struct RoundTokens {
    round: PageRound,
    current: Arc<Mutex<Current>>,
}

/// The token the page beyond this machine opens with, if any.
#[derive(Default)]
struct Current {
    token: Option<RoundToken>,
    /// The page left the network and the tunnel: no token opens it any more, and none is made.
    closed: bool,
    /// Where the address of the page on the network interface is announced, while it is served
    /// there.
    announcer: Option<Announcer>,
}

/// Announces the address of the page on a network interface.
struct Announcer {
    /// The address the page is served at.
    address: SocketAddr,
    announce: Box<dyn Fn(&str) + Send + Sync>,
}

impl Announcer {
    fn announce(&self, token: &Token) {
        (self.announce)(&token.url(self.address));
    }
}

/// The token of one round, or of the round that starts next.
struct RoundToken {
    /// The round the token belongs to; `None` for the round that starts next.
    round: Option<String>,
    token: Token,
}

impl RoundToken {
    /// Whether the token opens the page while `running` runs: the token of that round, or the
    /// token made for the round that starts next.
    fn opens(&self, running: Option<&String>) -> bool {
        self.round.is_none() || self.round.as_ref() == running
    }
}

impl RoundTokens {
    /// The tokens of the page of `round`.
    fn new(round: PageRound) -> Self {
        Self {
            round,
            current: Arc::default(),
        }
    }

    /// Gives the token to the round now running: the same one while the same round runs, the
    /// token made while no round ran to the round that starts, a new one for a round that
    /// replaces another, and a new one for the next round once no round runs. Announces the
    /// address of the page when the token or its round changed. Does nothing once closed.
    fn renew(&self) {
        let mut current = self.lock();
        self.renew_locked(&mut current);
    }

    /// The round that runs now, if any.
    pub(crate) fn running(&self) -> Option<String> {
        self.round.stages().round()
    }

    /// The token of the running round `round`, renewed first; `None` when another round runs, or
    /// none, or the tokens are closed.
    pub(crate) fn token_of(&self, round: &str) -> Option<Token> {
        let mut current = self.lock();
        self.renew_locked(&mut current);
        current
            .token
            .as_ref()
            .filter(|token| token.round.as_deref() == Some(round))
            .map(|token| token.token.clone())
    }

    /// Announces the address of the page on a network interface through `announcer` from now
    /// on, at once with the current token if there is one; `None` announces nothing more, once
    /// this returns, since announcements hold the lock.
    fn announce_to(&self, announcer: Option<Announcer>) {
        let mut current = self.lock();
        if let (Some(announcer), Some(token)) = (&announcer, &current.token) {
            announcer.announce(&token.token);
        }
        current.announcer = announcer;
    }

    /// Opens the page with no token any more, and makes none: the page left the network and the
    /// tunnel. No address is announced after this returns.
    fn close(&self) {
        let mut current = self.lock();
        current.closed = true;
        current.token = None;
        current.announcer = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Current> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Renews the locked token, unless the page left the network, and announces the address
    /// when the token or its round changed.
    fn renew_locked(&self, current: &mut Current) {
        if current.closed {
            return;
        }
        let running = self.round.stages().round();
        let token = &mut current.token;
        match token.as_mut() {
            Some(token) if token.round == running => return,
            Some(token) if token.round.is_none() => token.round = running,
            _ => {
                *token = Some(RoundToken {
                    round: running,
                    token: Token::random(),
                });
            }
        }
        if let (Some(token), Some(announcer)) = (token, &current.announcer) {
            announcer.announce(&token.token);
        }
    }
}

impl Rounds for RoundTokens {
    fn find(&self, token: &str) -> Option<PageRound> {
        let running = self.round.stages().round();
        let current = self.lock();
        current
            .token
            .as_ref()
            .filter(|current| current.opens(running.as_ref()) && current.token.matches(token))
            .map(|_| self.round.clone())
    }

    /// A Reset from the page ended the round of its token: the page receives the token made
    /// for the round that starts next, while no round runs. The reset round's own token opens
    /// nothing any more.
    fn after_reset(&self) -> Option<Token> {
        let mut current = self.lock();
        self.renew_locked(&mut current);
        current
            .token
            .as_ref()
            .filter(|current| current.round.is_none())
            .map(|current| current.token.clone())
    }
}

#[cfg(test)]
#[path = "network.tests.rs"]
mod tests;
