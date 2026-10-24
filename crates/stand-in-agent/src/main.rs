//! The stand-in agent's program: the agent in its pane, or, started with Claude Code's
//! `--fork-session`, a fork of its session, or, run as `observe` by a real Claude Code's hook,
//! the reporter of what that agent does. The library says what runs it agrees with it on.

mod agent;
mod fork;
mod hooks;
mod input;
mod observe;
mod transcript;
mod turns;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) == Some(stand_in_agent::OBSERVE) {
        observe::run();
    } else if arguments
        .iter()
        .any(|argument| argument == "--fork-session")
    {
        fork::run(&arguments);
    } else {
        agent::run();
    }
}
