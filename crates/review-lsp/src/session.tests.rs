use std::io::{BufReader, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ChildStdout;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;
use lsp_server::{Message, Notification, RequestId, Response, ResponseError};
use lsp_types::PositionEncodingKind;

use crate::api::{Event, Operation, Query};
use crate::source::ServerLocation;

use super::{Inbound, SERVER_STATUS_METHOD, STARTUP_TIMEOUT, ServerProcess, Session, State};

fn startup() -> crate::api::ServerStartup {
    crate::api::ServerStartup {
        id: toasts::ToastId::generate(),
        name: "rust-analyzer",
    }
}

fn session(state: State) -> Session {
    let mut process = ProcessCommand::new("sh")
        .args(["-c", "exec cat >/dev/null"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let input = std::io::BufWriter::new(process.stdin.take().unwrap());
    let (_sender, inbound) = unbounded();
    Session {
        process: ServerProcess {
            child: process,
            uses_direnv: false,
        },
        server: crate::language::LanguageServer::RustAnalyzer,
        startup: startup(),
        input,
        inbound,
        next_id: 1,
        encoding: PositionEncodingKind::UTF16,
        documents: std::collections::HashMap::new(),
        state,
    }
}

pub(crate) fn ready_session() -> Session {
    session(State::Ready)
}

#[test]
fn startup_reports_setup_diagnostics_when_the_child_exits_before_initialize() {
    let mut session = session(State::Stopped);
    session.process.child.kill().unwrap();
    session.process.child.wait().unwrap();
    let (sender, inbound) = unbounded();
    session.inbound = inbound;
    sender
        .send(Inbound::Failed(
            ".envrc is blocked; run direnv allow".to_owned(),
        ))
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let write_error = session.initialize(root.path(), Instant::now()).unwrap_err();
    assert_eq!(
        session.startup_failure(write_error),
        ".envrc is blocked; run direnv allow"
    );
}

fn session_with_output(state: State) -> (Session, BufReader<ChildStdout>) {
    let mut process = ProcessCommand::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = std::io::BufWriter::new(process.stdin.take().unwrap());
    let output = BufReader::new(process.stdout.take().unwrap());
    let (_sender, inbound) = unbounded();
    (
        Session {
            process: ServerProcess {
                child: process,
                uses_direnv: false,
            },
            server: crate::language::LanguageServer::RustAnalyzer,
            startup: startup(),
            input,
            inbound,
            next_id: 1,
            encoding: PositionEncodingKind::UTF16,
            documents: std::collections::HashMap::new(),
            state,
        },
        output,
    )
}

fn read_message(session: &mut Session, output: &mut BufReader<ChildStdout>) -> Message {
    session.input.flush().unwrap();
    Message::read(output).unwrap().unwrap()
}

fn query(path: std::path::PathBuf) -> Query {
    Query {
        toast_id: toasts::ToastId::generate(),
        path,
        line: 0,
        byte_column: 3,
        expected_line: "fn main() {}".to_owned(),
        snapshot_id: "snapshot".to_owned(),
    }
}

#[test]
fn panic_kills_and_reaps_the_child_process() {
    let mut pid = 0;
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let process = ServerProcess {
            child: ProcessCommand::new("sleep").arg("30").spawn().unwrap(),
            uses_direnv: false,
        };
        pid = process.child.id();
        panic!("test panic");
    }));

    assert!(panic.is_err());
    let output = ProcessCommand::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .unwrap();
    assert!(output.stdout.is_empty(), "child process {pid} survived");
}

#[test]
fn state_accessors_report_only_the_active_state() {
    let now = Instant::now();
    let directory = tempfile::tempdir().unwrap();
    let query = query(directory.path().join("source.rs"));
    let mut session = session(State::Ready);
    assert!(session.is_ready());
    assert!(!session.is_stopped());
    assert!(session.active_query().is_none());
    assert_eq!(session.next_deadline(), None);

    session.state = State::Querying {
        id: RequestId::from(4),
        operation: Operation::Hover,
        query: query.clone(),
        attempt: 2,
        deadline: now,
    };
    assert!(!session.is_ready());
    assert_eq!(session.active_query(), Some(&query));
    assert_eq!(session.next_deadline(), Some(now));

    session.state = State::Retrying {
        operation: Operation::Hover,
        query: query.clone(),
        attempt: 2,
        retry_at: now,
    };
    assert_eq!(session.active_query(), Some(&query));
    assert_eq!(session.next_deadline(), Some(now));
    session.state = State::Stopped;
    assert!(session.is_stopped());
}

#[test]
fn quiescent_status_makes_an_initializing_server_ready() {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut session = session(State::Quiescing { deadline });
    let busy = Notification::new(
        "experimental/serverStatus".to_owned(),
        serde_json::json!({ "quiescent": false }),
    );
    assert_eq!(
        session
            .handle(
                Inbound::Message(Message::Notification(busy)),
                Instant::now()
            )
            .unwrap(),
        None
    );
    assert!(!session.is_ready());

    let ready = Notification::new(
        "experimental/serverStatus".to_owned(),
        serde_json::json!({ "quiescent": true }),
    );
    assert_eq!(
        session
            .handle(
                Inbound::Message(Message::Notification(ready)),
                Instant::now()
            )
            .unwrap(),
        Some(Event::Ready(session.startup))
    );
    assert!(session.is_ready());
}

#[test]
fn server_status_is_handled_only_while_the_session_is_quiescing() {
    let deadline = Instant::now() + Duration::from_secs(1);
    let unrelated = Notification::new("unrelated/status".to_owned(), serde_json::Value::Null);
    let mut quiescing = session(State::Quiescing { deadline });
    assert_eq!(
        quiescing
            .handle(
                Inbound::Message(Message::Notification(unrelated)),
                Instant::now(),
            )
            .unwrap(),
        None
    );
    assert!(matches!(quiescing.state, State::Quiescing { .. }));

    let status = Notification::new(SERVER_STATUS_METHOD.to_owned(), serde_json::Value::Null);
    let mut ready = session(State::Ready);
    assert_eq!(
        ready
            .handle(
                Inbound::Message(Message::Notification(status)),
                Instant::now(),
            )
            .unwrap(),
        None
    );
    assert!(ready.is_ready());
}

#[test]
fn location_normalization_keeps_only_valid_source_locations() {
    let directory = tempfile::tempdir().unwrap();
    let valid_path = directory.path().join("valid.rs");
    std::fs::write(&valid_path, "zero\néx\n").unwrap();
    let missing_path = directory.path().join("missing.rs");
    let session = session(State::Ready);

    let locations = session.normalize_locations(vec![
        ServerLocation {
            path: valid_path.clone(),
            line: 1,
            character: 1,
            end_line: 1,
            end_character: 2,
        },
        ServerLocation {
            path: missing_path,
            line: 0,
            character: 0,
            end_line: 0,
            end_character: 0,
        },
    ]);

    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].path, valid_path);
    assert_eq!(locations[0].byte_column, 2);
    assert_eq!(locations[0].end_byte_column, 3);
}

#[test]
fn initialization_accepts_only_its_response_id() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1);
    let mut session = session(State::Initializing {
        id: RequestId::from(7),
        deadline,
    });
    let unrelated = Response::new_ok(
        RequestId::from(8),
        serde_json::json!({ "capabilities": {} }),
    );
    assert_eq!(session.handle_response(unrelated, now).unwrap(), None);
    assert!(matches!(session.state, State::Initializing { .. }));

    let matching = Response::new_ok(
        RequestId::from(7),
        serde_json::json!({
            "capabilities": { "positionEncoding": "utf-8" }
        }),
    );
    assert_eq!(session.handle_response(matching, now).unwrap(), None);
    assert_eq!(session.encoding, PositionEncodingKind::UTF8);
    assert!(matches!(
        session.state,
        State::Quiescing { deadline } if deadline == now + STARTUP_TIMEOUT
    ));
}

#[test]
fn shutdown_accepts_only_its_response_id() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1);
    let (mut session, mut output) = session_with_output(State::ShuttingDown {
        id: RequestId::from(7),
        deadline,
    });

    let unrelated = Response::new_ok(RequestId::from(8), serde_json::Value::Null);
    assert_eq!(session.handle_response(unrelated, now).unwrap(), None);
    assert!(matches!(session.state, State::ShuttingDown { .. }));

    let matching = Response::new_ok(RequestId::from(7), serde_json::Value::Null);
    assert_eq!(session.handle_response(matching, now).unwrap(), None);
    assert!(session.is_stopped());
    let Message::Notification(exit) = read_message(&mut session, &mut output) else {
        panic!("shutdown response did not send exit");
    };
    assert_eq!(exit.method, "exit");
}

#[test]
fn content_modified_retries_only_the_matching_active_query() {
    let now = Instant::now();
    let directory = tempfile::tempdir().unwrap();
    let query = query(directory.path().join("source.rs"));
    let mut session = session(State::Querying {
        id: RequestId::from(3),
        operation: Operation::Definition,
        query: query.clone(),
        attempt: 4,
        deadline: now,
    });
    let modified = Response {
        id: RequestId::from(3),
        response_result: Err(ResponseError {
            code: -32801,
            message: "content modified".to_owned(),
            data: None,
        }),
    };
    assert_eq!(session.handle_response(modified, now).unwrap(), None);
    let State::Retrying {
        operation,
        query: retried,
        attempt,
        retry_at,
    } = &session.state
    else {
        panic!("query was not retried");
    };
    assert_eq!(*operation, Operation::Definition);
    assert_eq!(retried, &query);
    assert_eq!(*attempt, 5);
    assert_eq!(*retry_at, now + Duration::from_millis(100));
}

#[test]
fn deadlines_fire_at_the_boundary_and_preserve_early_states() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1);
    let mut initializing = session(State::Initializing {
        id: RequestId::from(0),
        deadline,
    });
    assert_eq!(initializing.handle_deadline(now).unwrap(), None);
    assert!(matches!(initializing.state, State::Initializing { .. }));
    assert_eq!(
        initializing.handle_deadline(deadline).unwrap_err(),
        "rust-analyzer did not respond"
    );

    let directory = tempfile::tempdir().unwrap();
    let query = query(directory.path().join("source.rs"));
    let toast_id = query.toast_id;
    let mut querying = session(State::Querying {
        id: RequestId::from(1),
        operation: Operation::Hover,
        query,
        attempt: 0,
        deadline,
    });
    let event = querying.handle_deadline(deadline).unwrap().unwrap();
    assert!(matches!(
        event,
        Event::Failed {
            toast_id: Some(id),
            snapshot_id: Some(ref snapshot),
            ..
        } if id == toast_id && snapshot == "snapshot"
    ));
    assert!(querying.is_ready());
}

#[test]
fn query_rejects_changed_source_and_tracks_a_valid_request() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let now = Instant::now();
    let mut session = session(State::Ready);
    let mut changed = query(path.clone());
    changed.expected_line = "different".to_owned();
    assert!(
        session
            .request(Operation::Hover, changed, now)
            .unwrap_err()
            .contains("source changed")
    );

    session
        .request(Operation::References, query(path), now)
        .unwrap();
    assert_eq!(session.next_id, 2);
    assert!(matches!(
        session.state,
        State::Querying {
            operation: Operation::References,
            attempt: 0,
            ..
        }
    ));
}

#[test]
fn initialize_sends_the_required_client_capabilities() {
    let directory = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let (mut session, mut output) = session_with_output(State::Stopped);

    session.initialize(directory.path(), now).unwrap();

    let Message::Request(request) = read_message(&mut session, &mut output) else {
        panic!("initialize did not send a request");
    };
    assert_eq!(request.id, RequestId::from(0));
    assert_eq!(request.method, "initialize");
    let params: lsp_types::InitializeParams = serde_json::from_value(request.params).unwrap();
    assert_eq!(params.process_id, Some(std::process::id()));
    assert_eq!(params.workspace_folders.as_ref().unwrap().len(), 1);
    assert_eq!(
        params.client_info.as_ref().map(|info| info.name.as_str()),
        Some("progressive-reviewer")
    );
    assert_eq!(
        params
            .capabilities
            .experimental
            .as_ref()
            .and_then(|value| value.get("serverStatusNotification")),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        params
            .capabilities
            .general
            .unwrap()
            .position_encodings
            .unwrap(),
        vec![PositionEncodingKind::UTF8, PositionEncodingKind::UTF16]
    );
    assert_eq!(
        params
            .capabilities
            .text_document
            .unwrap()
            .hover
            .unwrap()
            .content_format
            .unwrap(),
        vec![lsp_types::MarkupKind::Markdown]
    );
    assert!(matches!(
        session.state,
        State::Initializing { deadline, .. }
            if deadline == now + super::INITIALIZE_TIMEOUT
    ));
}

#[test]
fn direnv_startup_allows_time_for_nix_environment_preparation() {
    let directory = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let (mut session, mut output) = session_with_output(State::Stopped);
    session.process.uses_direnv = true;
    session.initialize(directory.path(), now).unwrap();
    let _ = read_message(&mut session, &mut output);
    assert_eq!(session.next_deadline(), Some(now + STARTUP_TIMEOUT));
}

#[test]
fn expert_and_typescript_initialize_without_waiting_for_rust_status() {
    for server in [
        crate::language::LanguageServer::Expert,
        crate::language::LanguageServer::TypeScript,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let (mut session, mut output) = session_with_output(State::Stopped);
        session.server = server;
        session.initialize(directory.path(), now).unwrap();
        let Message::Request(request) = read_message(&mut session, &mut output) else {
            panic!("expected initialize");
        };
        assert_eq!(
            request.params["rootUri"],
            crate::source::path_uri(directory.path()).unwrap().as_str()
        );
        assert!(request.params["capabilities"]["experimental"].is_null());
        if server == crate::language::LanguageServer::TypeScript {
            assert_eq!(
                request.params["initializationOptions"]["tsserver"]["useSyntaxServer"],
                "never"
            );
        }

        let event = session
            .handle(
                Inbound::Message(Message::Response(Response::new_ok(
                    request.id,
                    serde_json::json!({ "capabilities": {} }),
                ))),
                now,
            )
            .unwrap();
        assert_eq!(event, Some(Event::Ready(session.startup)));
        let Message::Notification(initialized) = read_message(&mut session, &mut output) else {
            panic!("expected initialized");
        };
        assert_eq!(initialized.method, "initialized");
        assert!(session.is_ready());
        assert!(session.next_deadline().is_none());
    }
}

#[test]
fn opened_documents_use_their_language_and_forward_changed_text() {
    for (file, language) in [
        ("lib.ex", "elixir"),
        ("mix.exs", "elixir"),
        ("view.heex", "heex"),
        ("view.eex", "eelixir"),
        ("index.ts", "typescript"),
        ("view.tsx", "typescriptreact"),
        ("index.mjs", "javascript"),
        ("view.jsx", "javascriptreact"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(file);
        let (mut session, mut output) = session_with_output(State::Ready);
        std::fs::write(&path, "before").unwrap();
        session.open_document(&path).unwrap();
        let Message::Notification(open) = read_message(&mut session, &mut output) else {
            panic!("expected didOpen");
        };
        assert_eq!(open.method, "textDocument/didOpen");
        assert_eq!(open.params["textDocument"]["languageId"], language);
        assert_eq!(open.params["textDocument"]["text"], "before");

        std::fs::write(&path, "after").unwrap();
        session.open_document(&path).unwrap();
        let Message::Notification(change) = read_message(&mut session, &mut output) else {
            panic!("expected didChange");
        };
        assert_eq!(change.method, "textDocument/didChange");
        assert_eq!(change.params["textDocument"]["version"], 2);
        assert_eq!(change.params["contentChanges"][0]["text"], "after");
    }
}

#[test]
fn valid_query_opens_the_document_and_sends_the_selected_operation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let now = Instant::now();
    let (mut session, mut output) = session_with_output(State::Ready);

    session
        .request(Operation::References, query(path.clone()), now)
        .unwrap();

    let Message::Notification(open) = read_message(&mut session, &mut output) else {
        panic!("query did not open the document");
    };
    assert_eq!(open.method, "textDocument/didOpen");
    let Message::Request(request) = read_message(&mut session, &mut output) else {
        panic!("query did not send a request");
    };
    assert_eq!(request.id, RequestId::from(1));
    assert_eq!(request.method, "textDocument/references");
    assert_eq!(session.next_id, 2);
    assert!(session.documents.contains_key(&path));
    assert!(matches!(
        session.state,
        State::Querying { deadline, .. }
            if deadline == now + super::INITIALIZE_TIMEOUT
    ));
}

#[test]
fn document_sync_sends_changes_only_when_content_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "first\n").unwrap();
    let (mut session, mut output) = session_with_output(State::Ready);
    session.open_document(&path).unwrap();
    let Message::Notification(open) = read_message(&mut session, &mut output) else {
        panic!("document was not opened");
    };
    assert_eq!(open.method, "textDocument/didOpen");

    session.open_document(&path).unwrap();
    std::fs::write(&path, "second\n").unwrap();
    session.open_document(&path).unwrap();
    let Message::Notification(change) = read_message(&mut session, &mut output) else {
        panic!("document change was not sent");
    };
    assert_eq!(change.method, "textDocument/didChange");
    let params: lsp_types::DidChangeTextDocumentParams =
        serde_json::from_value(change.params).unwrap();
    assert_eq!(params.text_document.version, 2);
    assert_eq!(params.content_changes[0].text, "second\n");
}

#[test]
fn query_response_id_and_retry_limit_are_enforced() {
    let now = Instant::now();
    let directory = tempfile::tempdir().unwrap();
    let query = query(directory.path().join("source.rs"));
    let mut session = session(State::Querying {
        id: RequestId::from(3),
        operation: Operation::Hover,
        query: query.clone(),
        attempt: 9,
        deadline: now,
    });
    let unrelated = Response::new_ok(RequestId::from(4), serde_json::Value::Null);
    assert_eq!(session.handle_response(unrelated, now).unwrap(), None);
    assert!(matches!(session.state, State::Querying { .. }));

    let modified = Response {
        id: RequestId::from(3),
        response_result: Err(ResponseError {
            code: -32801,
            message: "content modified".to_owned(),
            data: None,
        }),
    };
    let event = session.handle_response(modified, now).unwrap().unwrap();
    assert!(matches!(event, Event::Failed { .. }));
    assert!(session.is_ready());
}

#[test]
fn all_deadline_states_wait_until_the_boundary() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(1);
    let mut quiescing = session(State::Quiescing { deadline });
    assert_eq!(quiescing.handle_deadline(now).unwrap(), None);
    assert_eq!(
        quiescing.handle_deadline(deadline).unwrap_err(),
        "rust-analyzer did not finish startup"
    );

    let directory = tempfile::tempdir().unwrap();
    let retry_query = query(directory.path().join("missing.rs"));
    let mut retrying = session(State::Retrying {
        operation: Operation::Hover,
        query: retry_query,
        attempt: 2,
        retry_at: deadline,
    });
    assert_eq!(retrying.handle_deadline(now).unwrap(), None);
    assert!(matches!(retrying.state, State::Retrying { .. }));
    assert!(retrying.handle_deadline(deadline).is_err());

    let query = query(directory.path().join("query.rs"));
    let mut querying = session(State::Querying {
        id: RequestId::from(3),
        operation: Operation::Hover,
        query: query.clone(),
        attempt: 1,
        deadline,
    });
    assert_eq!(querying.handle_deadline(now).unwrap(), None);
    assert!(matches!(querying.state, State::Querying { .. }));
    assert!(matches!(
        querying.handle_deadline(deadline).unwrap(),
        Some(Event::Failed {
            toast_id: Some(toast_id),
            snapshot_id: Some(snapshot_id),
            ..
        }) if toast_id == query.toast_id && snapshot_id == query.snapshot_id
    ));
    assert!(querying.is_ready());

    let (mut shutting_down, mut output) = session_with_output(State::ShuttingDown {
        id: RequestId::from(2),
        deadline,
    });
    assert_eq!(shutting_down.handle_deadline(now).unwrap(), None);
    assert!(matches!(shutting_down.state, State::ShuttingDown { .. }));
    assert_eq!(shutting_down.handle_deadline(deadline).unwrap(), None);
    let Message::Notification(exit) = read_message(&mut shutting_down, &mut output) else {
        panic!("shutdown deadline did not send exit");
    };
    assert_eq!(exit.method, "exit");
    assert!(shutting_down.is_stopped());
}

#[test]
fn shutdown_and_server_requests_write_protocol_responses() {
    let now = Instant::now();
    let (mut session, mut output) = session_with_output(State::Ready);
    session.next_id = 6;
    session.begin_shutdown(now).unwrap();
    let Message::Request(shutdown) = read_message(&mut session, &mut output) else {
        panic!("shutdown request was not sent");
    };
    assert_eq!(shutdown.id, RequestId::from(6));
    assert_eq!(shutdown.method, "shutdown");
    assert_eq!(session.next_id, 7);
    assert!(matches!(
        session.state,
        State::ShuttingDown { deadline, .. }
            if deadline == now + Duration::from_secs(1)
    ));

    session
        .respond(lsp_server::Request::new(
            RequestId::from(8),
            "workspace/configuration".to_owned(),
            serde_json::json!({ "items": [{}, {}] }),
        ))
        .unwrap();
    let Message::Response(response) = read_message(&mut session, &mut output) else {
        panic!("configuration response was not sent");
    };
    assert_eq!(
        response.response_result.unwrap(),
        serde_json::json!([null, null])
    );

    session
        .respond(lsp_server::Request::new(
            RequestId::from(9),
            "workspace/workspaceFolders".to_owned(),
            (),
        ))
        .unwrap();
    let Message::Response(response) = read_message(&mut session, &mut output) else {
        panic!("workspace response was not sent");
    };
    assert_eq!(response.response_result.unwrap(), serde_json::Value::Null);
}
