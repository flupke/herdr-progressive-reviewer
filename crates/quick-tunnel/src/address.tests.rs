use super::quick_tunnel_host;

#[test]
fn the_address_is_read_from_the_box_cloudflared_prints() {
    let line = "2026-10-06T09:12:44Z INF |  https://quiet-river-stone-lamp.trycloudflare.com                                 |";

    assert_eq!(
        quick_tunnel_host(line),
        Some("quiet-river-stone-lamp.trycloudflare.com")
    );
}

#[test]
fn other_addresses_in_the_log_are_no_tunnels() {
    for line in [
        "2026-10-06T09:12:40Z INF Requesting new quick Tunnel on trycloudflare.com...",
        "2026-10-06T09:12:41Z INF |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |",
        "2026-10-06T09:12:41Z INF Thank you for trying Cloudflare Tunnel. Read https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/trycloudflare/",
        "2026-10-06T09:12:41Z ERR failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": dial tcp: lookup api.trycloudflare.com: no such host",
        "https://.trycloudflare.com",
        "https://a.b.trycloudflare.com",
        "https://example.com/trycloudflare.com",
    ] {
        assert_eq!(quick_tunnel_host(line), None, "{line}");
    }
}
