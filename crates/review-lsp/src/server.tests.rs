use std::path::PathBuf;

use crossbeam_channel::unbounded;

use crate::api::{Command, Event, Operation, Query};

use super::{Server, ServerLoopControl};

fn query(snapshot_id: &str) -> Query {
    Query {
        toast_id: toasts::ToastId::generate(),
        path: PathBuf::from("source.rs"),
        line: 0,
        byte_column: 0,
        expected_line: String::new(),
        snapshot_id: snapshot_id.to_owned(),
    }
}

#[test]
fn shutdown_without_a_session_stops_the_command_loop() {
    let (event_sender, _events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );
    assert_eq!(server.command(Command::Shutdown), ServerLoopControl::Stop);
    assert!(server.stopping);

    let (commands, command_receiver) = unbounded();
    commands.send(Command::Shutdown).unwrap();
    server.run(&command_receiver);
}

#[test]
fn open_document_is_queued_until_the_session_is_ready() {
    let (event_sender, _events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );
    server.session = Some(crate::session::tests::ready_session());

    let document = PathBuf::from("source.rs");
    assert_eq!(
        server.command(Command::OpenDocument(document.clone())),
        ServerLoopControl::Continue
    );
    assert_eq!(server.pending, [Command::OpenDocument(document)]);
}

#[test]
fn startup_failure_reports_the_request_identity() {
    let (event_sender, events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/nonexistent-reviewer-lsp-project"),
            server: crate::language::LanguageServer::Expert,
        },
        event_sender,
    );
    let request = query("snapshot");
    server.command(Command::Request {
        operation: Operation::Definition,
        query: request.clone(),
    });
    assert!(matches!(events.recv().unwrap(), Event::Initializing(_)));
    assert!(matches!(
        events.recv().unwrap(),
        Event::Failed {
            snapshot_id: None,
            ..
        }
    ));
    assert!(matches!(events.recv().unwrap(), Event::Failed {
        toast_id: Some(id), snapshot_id: Some(snapshot), ..
    } if id == request.toast_id && snapshot == request.snapshot_id));
}

#[test]
fn restart_without_an_open_document_does_not_start_a_session() {
    let (event_sender, events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );

    server.session = Some(crate::session::tests::ready_session());

    assert_eq!(
        server.command(Command::Restart),
        ServerLoopControl::Continue
    );

    assert!(server.session.is_none());
    assert!(events.is_empty());
}

#[test]
fn pending_requests_are_failed_with_their_identity() {
    let (event_sender, events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );
    let first = query("first");
    let second = query("second");
    server
        .pending
        .push_back(Command::OpenDocument(PathBuf::from("source.rs")));
    server.pending.push_back(Command::Request {
        operation: Operation::Hover,
        query: first.clone(),
    });
    server.pending.push_back(Command::Request {
        operation: Operation::References,
        query: second.clone(),
    });

    assert!(server.fail_requests("failed"));

    for expected in [first, second] {
        assert!(matches!(
            events.recv().unwrap(),
            Event::Failed {
                toast_id: Some(toast_id),
                snapshot_id: Some(snapshot_id),
                message,
            } if toast_id == expected.toast_id
                && snapshot_id == expected.snapshot_id
                && message == "failed"
        ));
    }
    server.pending.clear();
    assert!(!server.fail_requests("failed"));
}

#[test]
fn session_results_are_forwarded_or_reported_as_failures() {
    let (event_sender, events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );
    let startup = crate::api::ServerStartup {
        id: toasts::ToastId::generate(),
        name: "rust-analyzer",
    };
    server.handle_session_result(Ok(Some(Event::Ready(startup))));
    assert_eq!(events.recv().unwrap(), Event::Ready(startup));

    server
        .pending
        .push_back(Command::OpenDocument(PathBuf::from("source.rs")));
    server.handle_session_result(Err("broken".to_owned()));
    assert!(server.pending.is_empty());
    assert!(matches!(
        events.recv().unwrap(),
        Event::Failed {
            toast_id: None,
            snapshot_id: None,
            message,
        } if message == "broken"
    ));
}

#[test]
fn stopping_session_failures_do_not_emit_user_errors() {
    let (event_sender, events) = unbounded();
    let mut server = Server::new(
        crate::language::Project {
            root: PathBuf::from("/repository"),
            server: crate::language::LanguageServer::RustAnalyzer,
        },
        event_sender,
    );
    server.stopping = true;
    server.fail_session("closed");
    assert!(events.is_empty());
    assert!(server.session.is_none());
}
