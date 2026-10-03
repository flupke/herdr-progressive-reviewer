//! The page on the network, for a phone or a tablet: served on the address of one network
//! interface, behind a new token for each round.
//!
//! The reviewer's settings come from its environment:
//!
//! - `HERDR_REVIEWER_EXPLORE_NETWORK=off` keeps the page on this machine.
//! - `HERDR_REVIEWER_EXPLORE_INTERFACE` names the interface, such as `wlan0`; by default, the
//!   interface of the route to the internet.
//! - `HERDR_REVIEWER_EXPLORE_PORT` is the first port tried (8790 by default); when another
//!   reviewer holds it, the page takes the next free one of the [`PORTS_TRIED`].

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::sync::{Arc, Mutex, PoisonError};

use review_explore_page::{PageRound, Rounds, Token};

/// The first port tried when the settings name none.
const DEFAULT_PORT: u16 = 8790;

/// How many ports, from the first one, the page tries: a firewall rule can open them all.
const PORTS_TRIED: u16 = 10;

/// Whether, and where, the reviewer serves its Explore page to the network.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkAccess {
    /// The page stays on this machine.
    Off,
    On {
        /// The interface whose address the page listens on; `None` for the interface of the
        /// route to the internet.
        interface: Option<String>,
        /// The first port tried; 0 takes any free port.
        port: u16,
    },
}

impl NetworkAccess {
    /// The settings of the reviewer's environment.
    pub fn from_env() -> Result<Self, String> {
        let variable = |name| std::env::var(name).ok();
        Self::from_settings(
            variable("HERDR_REVIEWER_EXPLORE_NETWORK").as_deref(),
            variable("HERDR_REVIEWER_EXPLORE_INTERFACE").as_deref(),
            variable("HERDR_REVIEWER_EXPLORE_PORT").as_deref(),
        )
    }

    fn from_settings(
        network: Option<&str>,
        interface: Option<&str>,
        port: Option<&str>,
    ) -> Result<Self, String> {
        match network {
            None | Some("on") => {}
            Some("off") => return Ok(Self::Off),
            Some(other) => {
                return Err(format!(
                    "HERDR_REVIEWER_EXPLORE_NETWORK must be on or off, not {other:?}"
                ));
            }
        }
        let port = match port {
            None => DEFAULT_PORT,
            Some(port) => port
                .parse()
                .map_err(|_| format!("HERDR_REVIEWER_EXPLORE_PORT must be a port, not {port:?}"))?,
        };
        Ok(Self::On {
            interface: interface.filter(|name| !name.is_empty()).map(str::to_owned),
            port,
        })
    }

    /// Listens on the address of the chosen interface, at the first free port of the
    /// [`PORTS_TRIED`]; `None` when network access is off.
    pub fn listen(&self) -> io::Result<Option<NetworkListener>> {
        let Self::On { interface, port } = self else {
            return Ok(None);
        };
        let ip = match interface {
            Some(name) => interface_address(name)?,
            None => default_address()?,
        };
        let ports = if *port == 0 {
            0..=0
        } else {
            *port..=port.saturating_add(PORTS_TRIED - 1)
        };
        let mut last = None;
        for port in ports {
            match TcpListener::bind((ip, port)) {
                Ok(listener) => {
                    listener.set_nonblocking(true)?;
                    let address = listener.local_addr()?;
                    return Ok(Some(NetworkListener { listener, address }));
                }
                Err(error) if error.kind() == io::ErrorKind::AddrInUse => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("no port to try")))
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

/// A listener on a network interface, for the page on the network.
pub struct NetworkListener {
    listener: TcpListener,
    address: SocketAddr,
}

impl NetworkListener {
    /// The address the page is served at.
    pub(crate) fn address(&self) -> SocketAddr {
        self.address
    }

    pub(crate) fn into_std(self) -> TcpListener {
        self.listener
    }
}

/// The round the page on the network shows, behind a token that changes with each round. The
/// token of a round that is no longer running opens nothing, even before the round's next
/// token is made. Clones share the token.
#[derive(Clone)]
pub(crate) struct RoundTokens {
    round: PageRound,
    current: Arc<Mutex<Option<RoundToken>>>,
}

/// The token of one round.
struct RoundToken {
    round: String,
    token: Token,
}

impl RoundTokens {
    pub(crate) fn new(round: PageRound) -> Self {
        Self {
            round,
            current: Arc::default(),
        }
    }

    /// Gives the round now running a token of its own: the same one while the same round runs,
    /// a new one for a new round, none when no round runs. Returns the address of the page
    /// served at `address` when it changed.
    pub(crate) fn renew(&self, address: SocketAddr) -> Option<Renewed> {
        let running = self.round.stages().round();
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if current.as_ref().map(|token| &token.round) == running.as_ref() {
            return None;
        }
        *current = running.map(|round| RoundToken {
            round,
            token: Token::random(),
        });
        Some(Renewed(
            current.as_ref().map(|current| current.token.url(address)),
        ))
    }
}

/// The address of the running round's page once its token changed; `None` once no round runs.
pub(crate) struct Renewed(pub(crate) Option<String>);

impl Rounds for RoundTokens {
    fn find(&self, token: &str) -> Option<PageRound> {
        let running = self.round.stages().round()?;
        let current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        current
            .as_ref()
            .filter(|current| current.round == running && current.token.matches(token))
            .map(|_| self.round.clone())
    }
}

#[cfg(test)]
#[path = "network.tests.rs"]
mod tests;
