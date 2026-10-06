//! Who may use the page: requests for the page's own host names, from a browser that has the
//! token of the page's address.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};

use axum::http::{HeaderMap, HeaderValue, header};

use crate::PageEvent;

/// The secret of the address that opens a round's page.
#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    /// A chosen token, such as a fixed one in development: letters, digits, `-` and `_` only,
    /// so that it fits in an address and a cookie as it is.
    pub fn chosen(token: String) -> Result<Self, String> {
        let valid = !token.is_empty()
            && token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if valid {
            Ok(Self(token))
        } else {
            Err(format!(
                "invalid token {token:?}: use letters, digits, - and _"
            ))
        }
    }

    /// The address that opens the page served at `address`, with this token.
    pub fn url(&self, address: SocketAddr) -> String {
        format!("http://{address}/?token={}", self.0)
    }

    /// The address that opens the page through the HTTPS tunnel whose public host name is
    /// `host`, with this token.
    pub fn tunnel_url(&self, host: &str) -> String {
        format!("https://{host}/?token={}", self.0)
    }

    /// The address that opens the page served on this machine's loopback `port`, with this
    /// token.
    pub fn loopback_url(&self, port: u16) -> String {
        self.url(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
    }

    /// Compares in constant time, so the time of a refusal tells nothing about the token.
    pub fn matches(&self, candidate: &str) -> bool {
        let expected = self.0.as_bytes();
        let candidate = candidate.as_bytes();
        expected.len() == candidate.len()
            && expected
                .iter()
                .zip(candidate)
                .fold(0, |difference, (a, b)| difference | (a ^ b))
                == 0
    }
}

impl fmt::Display for Token {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}

/// The `Host` values the page answers, with their port. A request for any other name is
/// refused: a foreign site that points its own name at this machine (DNS rebinding) cannot
/// read the page. A request whose `Origin` is another site is refused, whatever its method:
/// browsers name the origin of a script's request to another site and of every post, so a
/// page of another site, a page on another local port included, can neither read this one,
/// its polled status included, nor post to it. A request with no `Origin` passes this check:
/// it is a navigation, or does not come from a browser page, and the browser's reports of
/// policy violations need not carry one. A form post of the page must also require the
/// token's cookie.
pub struct Hosts {
    names: Vec<String>,
    /// How the browser reaches the page under these names: its origin's scheme.
    scheme: Scheme,
}

/// The scheme of the page's origin in the browser.
#[derive(Clone, Copy)]
enum Scheme {
    /// Plain HTTP, straight to the reviewer's listener.
    Http,
    /// HTTPS to a tunnel, which forwards the requests to the reviewer's listener in plain HTTP.
    Https,
}

impl Hosts {
    /// The names of this machine's loopback address, for a page on `port`.
    pub fn loopback(port: u16) -> Self {
        Self {
            names: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
            scheme: Scheme::Http,
        }
    }

    /// The one name of a page served at `address` of a network interface: the address the
    /// reviewer's QR code gives out.
    pub fn network(address: SocketAddr) -> Self {
        Self {
            names: vec![address.to_string()],
            scheme: Scheme::Http,
        }
    }

    /// The one name of a page that a tunnel serves over HTTPS at the public host name `host`,
    /// which the tunnel keeps as the `Host` of the requests it forwards.
    pub fn tunnel(host: &str) -> Self {
        Self {
            names: vec![host.to_owned()],
            scheme: Scheme::Https,
        }
    }

    /// The scope of the page's cookies for a request this page answers.
    pub(crate) fn admit(&self, headers: &HeaderMap) -> Result<CookieScope, PageEvent> {
        let host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .filter(|host| self.names.iter().any(|allowed| allowed == host))
            .ok_or(PageEvent::UnknownHost)?;
        match headers.get(header::ORIGIN) {
            Some(origin) if origin.as_bytes() != self.origin(host).as_bytes() => {
                Err(PageEvent::ForeignOrigin)
            }
            _ => Ok(CookieScope::of(host)),
        }
    }

    /// The origin of the page under the name `host`.
    fn origin(&self, host: &str) -> String {
        match self.scheme {
            Scheme::Http => format!("http://{host}"),
            Scheme::Https => format!("https://{host}"),
        }
    }

    /// The address of the page's socket under the name `host`, as the page's client opens it.
    pub(crate) fn socket(&self, host: &str) -> String {
        match self.scheme {
            Scheme::Http => format!("ws://{host}"),
            Scheme::Https => format!("wss://{host}"),
        }
    }
}

/// Browsers keep cookies by host name and ignore the port, so pages on one address (two
/// reviewers, or the reviewer and the standalone server) would overwrite each other's token.
/// The page names its cookies after its own port, so each keeps its own. This prevents
/// collisions only: the browser still sends this page's cookies to every port of its address,
/// so another server there that the browser visits receives this page's token, and can set a
/// cookie under this page's names (ADR 0003). The page's handlers use the plain names; the
/// scope renames the cookies of each request and response at the edge of the page.
pub(crate) struct CookieScope {
    suffix: String,
}

impl CookieScope {
    /// The scope of the page that answers the host name `host`, which ends with its port.
    fn of(host: &str) -> Self {
        let port = host.rsplit_once(':').map_or("80", |(_, port)| port);
        Self {
            suffix: format!("_{port}"),
        }
    }

    /// Keeps only the request's cookies of this scope, under their plain names.
    pub(crate) fn receive(&self, headers: &mut HeaderMap) {
        let kept: Vec<String> = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                Some(format!("{}={value}", name.strip_suffix(&self.suffix)?))
            })
            .collect();
        headers.remove(header::COOKIE);
        if !kept.is_empty()
            && let Ok(value) = HeaderValue::from_str(&kept.join("; "))
        {
            headers.insert(header::COOKIE, value);
        }
    }

    /// Names each cookie the response sets after this scope.
    pub(crate) fn send(&self, headers: &mut HeaderMap) {
        let scoped: Vec<HeaderValue> = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| {
                let (name, rest) = value.split_once('=')?;
                HeaderValue::from_str(&format!("{name}{}={rest}", self.suffix)).ok()
            })
            .collect();
        headers.remove(header::SET_COOKIE);
        for value in scoped {
            headers.append(header::SET_COOKIE, value);
        }
    }
}

/// The cookie that keeps the token once the page has traded the address's token for it: out
/// of reach of the page's scripts, and left out of requests that other sites start.
pub(crate) struct TokenCookie;

impl TokenCookie {
    const NAME: &str = "explore_token";

    /// The token a request's cookie carries.
    pub(crate) fn read(headers: &HeaderMap) -> Option<&str> {
        cookie(headers, Self::NAME)
    }

    /// The `Set-Cookie` value that keeps `token`.
    pub(crate) fn set(token: &str) -> Option<HeaderValue> {
        HeaderValue::from_str(&format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Lax",
            Self::NAME
        ))
        .ok()
    }
}

/// The value of a request's cookie `name`.
pub(crate) fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
}
