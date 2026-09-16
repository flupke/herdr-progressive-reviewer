use review_mcp_config::{Client, UserConfig};

pub(super) fn install() -> eyre::Result<()> {
    let arguments = std::env::args().skip(2).collect::<Vec<_>>();
    let clients = match arguments.as_slice() {
        [] => Client::ALL.to_vec(),
        [client] => vec![client.parse::<Client>().map_err(eyre::Report::msg)?],
        _ => eyre::bail!("usage: reviewer-control mcp-install [codex|claude]"),
    };
    let bridge = std::env::current_exe()?.with_file_name("reviewer-mcp");
    let mut failed = false;
    for client in clients {
        let configuration =
            UserConfig::from_env(client, bridge.clone()).map_err(eyre::Report::msg)?;
        match configuration.install() {
            Ok(added) => eprintln!(
                "{}: {}",
                configuration.path().display(),
                if added {
                    "registered"
                } else {
                    "already configured"
                },
            ),
            Err(error) => {
                failed = true;
                eprintln!("MCP setup for {}: {error}", configuration.path().display());
            }
        }
    }
    if failed {
        eyre::bail!("Some MCP registrations could not be added; see the errors above");
    }
    Ok(())
}
