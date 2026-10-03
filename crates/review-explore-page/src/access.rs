//! Who may use the page: requests for the page's own host names, from a browser that has the
//! token of the page's address.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};

use axum::http::{HeaderMap, HeaderValue, header};

use crate::PageEvent;

/// The secret of the address that opens a round's page.
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
pub struct Hosts(Vec<String>);

impl Hosts {
    /// The names of this machine's loopback address, for a page on `port`.
    pub fn loopback(port: u16) -> Self {
        Self(vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
        ])
    }

    /// The one name of a page served at `address` of a network interface: the address the
    /// reviewer's QR code gives out.
    pub fn network(address: SocketAddr) -> Self {
        Self(vec![address.to_string()])
    }

    pub(crate) fn admit(&self, headers: &HeaderMap) -> Result<(), PageEvent> {
        let host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .filter(|host| self.0.iter().any(|allowed| allowed == host))
            .ok_or(PageEvent::UnknownHost)?;
        match headers.get(header::ORIGIN) {
            Some(origin) if origin.as_bytes() != format!("http://{host}").as_bytes() => {
                Err(PageEvent::ForeignOrigin)
            }
            _ => Ok(()),
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
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .find_map(|pair| pair.trim().strip_prefix(Self::NAME)?.strip_prefix('='))
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
