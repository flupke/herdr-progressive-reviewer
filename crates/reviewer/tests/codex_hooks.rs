//! A Codex TUI in a Herdr test server, on an app server started outside that Herdr, with a
//! scratch Codex home where `reviewer-control codex-plugin install` installed and trusted the
//! reviewer's Codex plugin, and a local model fixture: the plugin's hook confirms a prompt typed
//! alone, and blocks text left in the input box that joined a typed prompt or `/resume`, so the
//! agent stays on its session. It calls no model API.

use std::ffi::OsStr;
use std::fs;
use std::process::Command;
use std::sync::mpsc;

use agent_hooks::{AgentHooks, Heard, HookDirectory, HookedAgent};
use herdr_client::protocol::{AgentPort as _, PaneId};
use review_mcp::{Endpoint, Response, Server};
use review_test_support::codex::{AppServer, CodexHome, Provider, prompt, wait_output};
use review_test_support::{GUARD, HerdrTestServer, TestPort};

/// Why the reviewer blocks a prompt, as Codex shows it.
const BLOCKED: &str = "The test reviewer stopped this prompt";

#[test]
fn the_codex_plugin_s_hook_hears_a_typed_prompt_and_blocks_text_left_in_the_input_box() {
    let codex = CodexHome::new();
    let port = TestPort::new();
    let endpoint = Endpoint::for_repository(&codex.workspace(), Some(port.number())).unwrap();
    let (named, sessions) = mpsc::channel();
    let _mcp = Server::start(endpoint, move |request| {
        let _ = named.send(request.agent_session.clone());
        request.respond(Ok(Response::Threads(Vec::new())));
        Ok(())
    })
    .unwrap();
    let provider = Provider::start(
        &codex,
        &format!(
            "[mcp_servers.herdr_reviewer]\nurl = \"{}\"\n",
            endpoint.url()
        ),
    );
    plugin(&codex, "install");
    let runtime = tempfile::tempdir().unwrap();
    // The daemon was started from a pane of another Herdr server.
    let _daemon = AppServer::start(
        &codex,
        &provider,
        &[
            ("XDG_RUNTIME_DIR", runtime.path().as_os_str()),
            ("HERDR_PANE_ID", OsStr::new("w:elsewhere")),
            ("HERDR_SOCKET_PATH", OsStr::new("/run/other-herdr.sock")),
            ("SHELL", OsStr::new("/bin/sh")),
        ],
    );
    let herdr = HerdrTestServer::start(&codex.workspace());
    let pane = codex.start_tui(&herdr, &["--remote", "unix://"]);
    let hooks = AgentHooks::listen(&HookDirectory::for_server(
        runtime.path(),
        std::path::Path::new(&herdr.environment()[OsStr::new("HERDR_SOCKET_PATH")]),
    ))
    .unwrap();
    let session = || {
        sessions
            .recv_timeout(GUARD)
            .expect("the agent called no reviewer tool")
            .expect("the call named no agent session")
    };

    prompt(&herdr, &pane, "call the reviewer");
    let earlier = session();
    prompt(&herdr, &pane, "/new");
    prompt(&herdr, &pane, "call the reviewer");
    let current = session();
    assert_ne!(current, earlier);
    hooks.name_session(&pane, &current);
    let agent = herdr.client().get_agent(&pane).unwrap().unwrap();
    let codex_agent = hooks
        .hooked(&herdr.client(), &agent)
        .expect("the plugin's hook ran in the agent's session");
    assert_eq!(
        codex_agent,
        HookedAgent::Codex {
            session: current.clone()
        }
    );

    let alone = hooks.expect_prompt(&codex_agent, "typed alone", BLOCKED);
    prompt(&herdr, &pane, "typed alone");
    assert_eq!(alone.wait(GUARD), Some(Heard::Submitted));
    drop(alone);
    wait_output(&herdr, &pane, "Answered: typed alone");

    let reply = hooks.expect_prompt(&codex_agent, "a reply", BLOCKED);
    leave_in_input_box(&herdr, &pane, "my words");
    prompt(&herdr, &pane, "a reply");
    assert_eq!(
        reply.wait(GUARD),
        Some(Heard::Joined {
            left: "my words".into()
        })
    );
    drop(reply);
    wait_output(&herdr, &pane, "Blocked by hook");
    wait_output(&herdr, &pane, BLOCKED);

    let resume = hooks.expect_resume(&codex_agent, &earlier, BLOCKED);
    leave_in_input_box(&herdr, &pane, "more words");
    prompt(&herdr, &pane, &format!("/resume {earlier}"));
    assert_eq!(
        resume.wait(GUARD),
        Some(Heard::Blocked {
            prompt: format!("more words/resume {earlier}")
        })
    );
    drop(resume);

    // The agent stayed on its session: its next prompt comes from there.
    let after = hooks.expect_prompt(&codex_agent, "still here", BLOCKED);
    prompt(&herdr, &pane, "still here");
    assert_eq!(after.wait(GUARD), Some(Heard::Submitted));
    wait_output(&herdr, &pane, "Answered: still here");

    plugin(&codex, "uninstall");
    let config = fs::read_to_string(codex.home().join("config.toml")).unwrap();
    assert!(
        !config.contains("progressive-reviewer") && !config.contains("herdr-progressive-reviewer"),
        "the plugin, its marketplace and its trust are gone:\n{config}"
    );
}

/// Types `text` in the agent's input box, without submitting it, and waits until it shows.
fn leave_in_input_box(herdr: &HerdrTestServer, pane: &PaneId, text: &str) {
    herdr.run_cli(&["pane", "send-text", &pane.0, text]);
    wait_output(herdr, pane, text);
}

/// Runs `reviewer-control codex-plugin <action>` on `codex`'s home.
fn plugin(codex: &CodexHome, action: &str) {
    let status = Command::new(env!("CARGO_BIN_EXE_reviewer-control"))
        .args(["codex-plugin", action])
        .envs(codex.environment())
        .status()
        .unwrap();
    assert!(status.success(), "codex-plugin {action}: {status}");
}
