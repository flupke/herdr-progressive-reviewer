use review_mcp::Endpoint;
use review_mcp_config::{Client, ProjectConfig};
use review_repository::repository::Repository;

pub(super) fn print() -> eyre::Result<()> {
    let arguments = std::env::args().skip(2).collect::<Vec<_>>();
    let [client] = arguments.as_slice() else {
        eyre::bail!("usage: reviewer-control mcp-config <codex|claude>");
    };
    let client = client.parse::<Client>().map_err(eyre::Report::msg)?;
    let configuration = project()?;
    eprintln!(
        "Merge this into {} at the repository root:",
        client.config_path()
    );
    print!(
        "{}",
        configuration.snippet(client).map_err(eyre::Report::msg)?
    );
    eprintln!(
        "Reload MCP configuration or restart/resume the agent once to load its tools. Check the connection with /mcp. This command only prints configuration."
    );
    Ok(())
}

pub(super) fn install() -> eyre::Result<()> {
    if std::env::args().len() != 2 {
        eyre::bail!("usage: reviewer-control mcp-install");
    }
    let configuration = project()?;
    let mut failed = false;
    for client in Client::ALL {
        match configuration.install(client) {
            Ok(added) => eprintln!(
                "{}: {}",
                client.config_path(),
                if added {
                    "registered"
                } else {
                    "already configured"
                },
            ),
            Err(error) => {
                failed = true;
                eprintln!("{error}");
            }
        }
    }
    if failed {
        eyre::bail!("Some MCP registrations could not be added; see the errors above");
    }
    eprintln!(
        "Reload MCP configuration or restart/resume the agent once, then check /mcp. The reviewer need not be open while the agent starts."
    );
    Ok(())
}

fn project() -> eyre::Result<ProjectConfig> {
    let repository = Repository::discover(&std::env::current_dir()?)?;
    let endpoint = Endpoint::from_env(repository.root()).map_err(eyre::Report::msg)?;
    Ok(ProjectConfig::new(
        repository.root(),
        endpoint,
        std::env::current_exe()?.with_file_name("reviewer-mcp"),
    ))
}
