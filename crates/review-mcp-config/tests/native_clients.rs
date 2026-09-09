//! Installed-client compatibility checks with private configuration and a local model fixture.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use review_mcp::{Endpoint, Response, Server};
use review_mcp_config::{Client, UserConfig};
use test_case::test_case;

#[derive(Clone, Copy)]
enum Port {
    Repository,
    Override,
}

struct NativeFixture {
    directory: tempfile::TempDir,
    endpoint: Endpoint,
    _port: review_test_support::TestPort,
}

impl NativeFixture {
    fn new(port: Port) -> Self {
        for _ in 0..100 {
            let directory = tempfile::tempdir().unwrap();
            let workspace = directory.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let lease = match port {
                Port::Repository => review_test_support::TestPort::reserve(
                    Endpoint::for_repository(&workspace, None)
                        .unwrap()
                        .address()
                        .port(),
                ),
                Port::Override => Some(review_test_support::TestPort::new()),
            };
            if let Some(lease) = lease {
                let endpoint = Endpoint::for_repository(&workspace, Some(lease.number())).unwrap();
                return Self {
                    directory,
                    endpoint,
                    _port: lease,
                };
            }
        }
        panic!("No private repository endpoint available");
    }
}

#[test_case(Port::Repository; "preinstalled_clients")]
#[test_case(Port::Override; "preinstalled_custom_port")]
fn native_clients_recover_after_the_reviewer_reopens(port: Port) {
    let fixture = NativeFixture::new(port);
    let directory = &fixture.directory;
    let endpoint = fixture.endpoint;
    let workspace = directory.path().join("workspace");
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(&workspace)
            .status()
            .unwrap()
            .success()
    );
    for (client, config_directory) in [(Client::Codex, "codex"), (Client::Claude, "claude")] {
        assert!(
            UserConfig::new(
                client,
                &directory.path().join(config_directory),
                env!("CARGO_BIN_EXE_reviewer-mcp").into()
            )
            .install()
            .unwrap()
        );
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let mut child = Command::new("python3")
        .arg("-B")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/native_clients.py"
        ))
        .arg(directory.path())
        .arg(match port {
            Port::Repository => "repository-port",
            Port::Override => "custom-port",
        })
        .arg(endpoint.address().port().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(directory.path().join("fixture-stderr.txt")).unwrap())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut server = None;
    for phase in ["closed", "available", "closed-again", "reopened"] {
        let mut line = String::new();
        if output.read_line(&mut line).unwrap() == 0 {
            break;
        }
        assert_eq!(line.trim(), phase);
        if matches!(phase, "available" | "reopened") {
            let calls = Arc::clone(&calls);
            server = Some(
                Server::start(endpoint, move |request| {
                    assert_eq!(request.access, "native-probe");
                    calls.fetch_add(1, Ordering::Relaxed);
                    request.respond(Ok(Response::Threads(Vec::new())));
                    Ok(())
                })
                .unwrap(),
            );
        } else {
            drop(server.take());
        }
        writeln!(child.stdin.as_mut().unwrap(), "continue").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        std::fs::read_to_string(directory.path().join("fixture-stderr.txt")).unwrap_or_default(),
        std::fs::read_to_string(directory.path().join("codex-stderr.txt")).unwrap_or_default()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2, "One call per open phase");
    assert!(!workspace.join(".codex").exists());
    assert!(!workspace.join(".mcp.json").exists());
}
