//! Who may use the page, as a browser or another site would ask: only the page's own host
//! names, only a browser that traded the address's token for the cookie of the page's own port,
//! and never a script or a post of another site. The page's socket follows the same rules
//! (`socket.tests.rs`).

use std::fmt::Write as _;
use std::net::SocketAddr;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::post;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    CommandSender, ExplorePage, Hosts, PageFiles, PageRound, RoundPublisher, RoundStage, Rounds,
    Token,
};

const TOKEN: &str = "test-token";

/// The rounds of the test's page: one, which the token opens.
struct OneRound(RoundPublisher);

impl Rounds for OneRound {
    fn find(&self, token: &str) -> Option<PageRound> {
        if !Token::chosen(TOKEN.into()).unwrap().matches(token) {
            return None;
        }
        let commands = CommandSender::new(|_, reply| reply.send(Ok(())));
        Some(PageRound::new(self.0.subscribe(), commands))
    }
}

/// The page, with a route of its caller's (`/test/sessions`, as the standalone server adds),
/// served on a free loopback port.
async fn serve() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let rounds = RoundPublisher::default();
    rounds.publish(
        None,
        RoundStage::NoRound {
            start: "start-1".into(),
        },
    );
    let page = ExplorePage::new(
        OneRound(rounds),
        Hosts::loopback(address.port()),
        PageFiles::embedded(),
        |_| {},
    );
    let caller = Router::new().route("/test/sessions", post(|| async { StatusCode::OK }));
    let app = page.into_router(caller);
    tokio::spawn(async move { axum::serve(listener, app).await });
    address
}

/// A request to the page at `address`, with the page's own host name unless `headers` names
/// another.
struct Ask<'a> {
    method: &'a str,
    target: &'a str,
    headers: Vec<(&'a str, String)>,
}

impl<'a> Ask<'a> {
    fn get(target: &'a str) -> Self {
        Self {
            method: "GET",
            target,
            headers: Vec::new(),
        }
    }

    fn post(target: &'a str) -> Self {
        Self {
            method: "POST",
            ..Self::get(target)
        }
    }

    fn with(mut self, name: &'a str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// The status of the page's response.
    async fn status(self, address: SocketAddr) -> u16 {
        let mut request = format!(
            "{} {} HTTP/1.1\r\nConnection: close\r\n",
            self.method, self.target
        );
        if !self.headers.iter().any(|(name, _)| *name == "Host") {
            let _ = write!(request, "Host: {address}\r\n");
        }
        for (name, value) in &self.headers {
            let _ = write!(request, "{name}: {value}\r\n");
        }
        request.push_str("Content-Length: 0\r\n\r\n");
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status in {response:?}"))
    }
}

/// The cookie of the page at `address` that keeps `token`.
fn token_cookie(address: SocketAddr, token: &str) -> String {
    format!("explore_token_{}={token}", address.port())
}

#[tokio::test]
async fn only_the_token_of_the_address_opens_the_page() {
    let address = serve().await;

    assert_eq!(Ask::get("/?token=wrong").status(address).await, 403);
    assert_eq!(Ask::get("/").status(address).await, 403);
    assert_eq!(Ask::get("/token").status(address).await, 403);
    let wrong = token_cookie(address, "wrong");
    assert_eq!(
        Ask::get("/").with("Cookie", wrong).status(address).await,
        403
    );
    let opens = format!("/?token={TOKEN}");
    assert_eq!(Ask::get(&opens).status(address).await, 303);
    let cookie = token_cookie(address, TOKEN);
    assert_eq!(
        Ask::get("/")
            .with("Cookie", cookie.clone())
            .status(address)
            .await,
        200
    );
    assert_eq!(
        Ask::get("/token")
            .with("Cookie", cookie)
            .status(address)
            .await,
        204
    );
}

// Browsers keep a host's cookies for all its ports: the token cookie of another page on this
// host, on another port, does not stand in for this page's own.
#[tokio::test]
async fn the_page_reads_the_token_only_from_the_cookie_of_its_own_port() {
    let address = serve().await;

    for cookie in [
        format!("explore_token={TOKEN}"),
        format!("explore_token_1={TOKEN}"),
    ] {
        assert_eq!(
            Ask::get("/")
                .with("Cookie", cookie.clone())
                .status(address)
                .await,
            403,
            "{cookie}"
        );
    }
    let own = token_cookie(address, TOKEN);
    assert_eq!(Ask::get("/").with("Cookie", own).status(address).await, 200);
}

#[tokio::test]
async fn a_request_for_another_host_name_is_refused_the_callers_routes_included() {
    let address = serve().await;
    let host = format!("rebound.example:{}", address.port());

    let opens = format!("/?token={TOKEN}");
    assert_eq!(
        Ask::get(&opens)
            .with("Host", host.clone())
            .status(address)
            .await,
        403
    );
    let cookie = token_cookie(address, TOKEN);
    let load = Ask::get("/")
        .with("Host", host.clone())
        .with("Cookie", cookie);
    assert_eq!(load.status(address).await, 403);
    let route = Ask::post("/test/sessions").with("Host", host);
    assert_eq!(route.status(address).await, 403);
    assert_eq!(Ask::post("/test/sessions").status(address).await, 200);
}

#[tokio::test]
async fn a_post_or_a_script_of_another_site_is_refused() {
    let address = serve().await;
    let cookie = token_cookie(address, TOKEN);

    for origin in ["http://foreign.example", "http://127.0.0.1:1"] {
        let report = Ask::post("/csp-report").with("Origin", origin);
        assert_eq!(report.status(address).await, 403, "{origin}");
        let route = Ask::post("/test/sessions").with("Origin", origin);
        assert_eq!(route.status(address).await, 403, "{origin}");
        let load = Ask::get("/")
            .with("Cookie", cookie.clone())
            .with("Origin", origin);
        assert_eq!(load.status(address).await, 403, "{origin}");
    }
    let own = format!("http://{address}");
    let report = Ask::post("/csp-report").with("Origin", own.clone());
    assert_eq!(report.status(address).await, 204);
    let load = Ask::get("/").with("Cookie", cookie).with("Origin", own);
    assert_eq!(load.status(address).await, 200);
}
