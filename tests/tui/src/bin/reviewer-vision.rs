//! The vision MCP server, on standard input and output. Register it with an agent (the
//! repository's `.mcp.json` does) and call its `start` tool.

use anyhow::Result;
use reviewer_tui_tests::vision::{Setup, serve};

fn main() -> Result<()> {
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "reviewer-vision: the vision MCP server, on standard input and output.\n\nThe repository's .mcp.json registers it through scripts/vision-mcp: an agent that runs in\nthe repository, inside Herdr (HERDR_SOCKET_PATH), gets its tools. Their start tool runs the\nreviewer of this checkout in a new \"reviewer vision\" workspace of that Herdr; switch to it\nto watch. See .agents/wiki/tui-vision.md."
        );
        return Ok(());
    }
    serve(Setup::from_env()?)
}
