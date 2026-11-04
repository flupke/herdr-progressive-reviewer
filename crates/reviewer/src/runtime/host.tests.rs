use test_case::test_case;

use super::*;

#[test_case(&[("HERDR_SOCKET_PATH", "/run/herdr.sock"), ("HC_TERMINAL_SOCKET", "/run/hc.sock")], Some(HostKind::Herdr); "Herdr run in a terminal of herdr-app")]
#[test_case(&[("HC_TERMINAL_SOCKET", "/run/hc.sock")], Some(HostKind::HerdrCore); "herdr-app")]
#[test_case(&[("HERDR_SOCKET_PATH", ""), ("HC_TERMINAL_SOCKET", "/run/hc.sock")], Some(HostKind::HerdrCore); "an empty Herdr socket")]
#[test_case(&[("HC_TERMINAL_SOCKET", "")], None; "an empty herdr-core socket")]
fn the_host_is_the_one_whose_socket_the_pane_is_given(
    environment: &[(&str, &str)],
    expected: Option<HostKind>,
) {
    let var = |name: &str| {
        environment
            .iter()
            .find(|(set, _)| *set == name)
            .map(|(_, value)| OsString::from(value))
    };
    assert_eq!(host_kind(var), expected);
}
