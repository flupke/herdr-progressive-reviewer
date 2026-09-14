//! Agent-owned MCP catalogue, forwarding calls to the project reviewer when open.

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [port] = args.as_slice() else {
        return Err("usage: reviewer-mcp <port>".into());
    };
    let port = port.parse::<u16>().map_err(|error| error.to_string())?;
    let endpoint = review_mcp::Endpoint::for_repository(
        &std::env::current_dir().map_err(|error| error.to_string())?,
        Some(port),
    )?;
    review_mcp::serve_stdio(endpoint)
}
