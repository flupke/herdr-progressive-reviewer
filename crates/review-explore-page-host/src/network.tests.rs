use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use review_explore_page::RoundStage;

use super::*;
use crate::test_page::{
    PAST, Shared, loopback, no_round, path, published, share_on_loopback, status, working,
};

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
            answer: None,
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

/// A socket open to `path` of the page at `address`, behind the token of `url`.
fn socket(
    address: std::net::SocketAddr,
    url: &str,
    path: &str,
) -> tokio_tungstenite::tungstenite::WebSocket<TcpStream> {
    use tokio_tungstenite::tungstenite::client;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let token = url.rsplit('=').next().unwrap();
    let mut upgrade = format!("ws://{address}{path}")
        .into_client_request()
        .unwrap();
    let headers = upgrade.headers_mut();
    let cookie = format!("explore_token_{}={token}", address.port());
    headers.insert("cookie", cookie.parse().unwrap());
    headers.insert("origin", format!("http://{address}").parse().unwrap());
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client(upgrade, stream).unwrap().0
}

/// The view of the first message `socket` reads.
fn first_view(
    socket: &mut tokio_tungstenite::tungstenite::WebSocket<TcpStream>,
) -> serde_json::Value {
    use tokio_tungstenite::tungstenite::Message;

    loop {
        let Message::Text(text) = socket.read().unwrap() else {
            continue;
        };
        let message: serde_json::Value = serde_json::from_str(&text).unwrap();
        if message["method"] == "state" {
            return message["params"]["view"].clone();
        }
    }
}

#[test]
fn only_the_page_on_this_machine_lists_and_opens_past_rounds() {
    let shared = Shared::start();
    let path = format!("/ws?past={PAST}");
    let loopback = shared.host.url().to_owned();
    let port = loopback
        .trim_start_matches("http://127.0.0.1:")
        .split('/')
        .next()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let here = std::net::SocketAddr::from(([127, 0, 0, 1], port));

    let mut on_this_machine = socket(here, &loopback, &path);
    let mut on_the_network = socket(shared.address, &shared.first, &path);

    let view = first_view(&mut on_this_machine);
    assert_eq!(view["past"]["round"], PAST);
    assert_eq!(view["past_rounds"][0]["rounds"][0]["id"], PAST);
    let view = first_view(&mut on_the_network);
    assert!(view["past"].is_null(), "{}", view["past"]);
    assert!(view["past_rounds"].is_null(), "{}", view["past_rounds"]);
}

#[test]
fn a_reset_on_the_network_page_hands_that_page_the_start_screens_token() {
    use tokio_tungstenite::tungstenite::Message;

    let shared = Shared::start();
    shared.start_round("r1");
    let mut socket = socket(shared.address, &shared.first, "/ws");

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

    assert!(TcpStream::connect(shared.address).is_err());
    shared.round.publish(Some(published("r2")), working());
    assert_eq!(
        shared.announced.try_recv(),
        Err(mpsc::TryRecvError::Disconnected),
        "the page left the network with what announced its address"
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
    assert!(TcpStream::connect(shared.address).is_err());
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
