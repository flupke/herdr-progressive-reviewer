//! The stand-in agent's program: the agent in its pane, or, started with Claude Code's
//! `--fork-session`, a fork of its session. The library says what runs it agrees with it on.

mod agent;
mod fork;
mod hooks;
mod input;
mod transcript;
mod turns;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--fork-session")
    {
        fork::run(&arguments);
    } else {
        agent::run();
    }
}
