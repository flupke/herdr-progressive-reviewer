use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use lsp_server::{ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    GotoDefinitionParams, GotoDefinitionResponse, HoverClientCapabilities, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, Location, MarkupKind,
    PartialResultParams, Position, PositionEncodingKind, ReferenceContext, ReferenceParams,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier,
    WorkDoneProgressParams, WorkspaceFolder,
};

use crate::api::{Event, Operation, Query, ServerStartup, SourceLocation};
use crate::language::{Language, LanguageServer, Project};
use crate::process::{ServerProcess, StderrOutput};
use crate::reader::MessageReader;
use crate::source::{ServerLocation, encoded_column, hover_markdown, path_uri};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(300);
const SERVER_STATUS_METHOD: &str = "experimental/serverStatus";

#[derive(serde::Deserialize)]
struct ServerStatus {
    quiescent: bool,
}

pub(super) enum Inbound {
    Message(Message),
    Failed(String),
}

enum State {
    Initializing {
        id: RequestId,
        deadline: Instant,
    },
    Quiescing {
        deadline: Instant,
    },
    Ready,
    Querying {
        id: RequestId,
        operation: Operation,
        query: Query,
        attempt: u8,
        deadline: Instant,
    },
    Retrying {
        operation: Operation,
        query: Query,
        attempt: u8,
        retry_at: Instant,
    },
    ShuttingDown {
        id: RequestId,
        deadline: Instant,
    },
    Stopped,
}

pub(super) struct Session {
    process: ServerProcess,
    server: LanguageServer,
    startup: ServerStartup,
    input: BufWriter<ChildStdin>,
    inbound: Receiver<Inbound>,
    next_id: i32,
    encoding: PositionEncodingKind,
    documents: HashMap<PathBuf, (String, i32)>,
    state: State,
}

impl Session {
    pub(super) fn start(
        project: &Project,
        startup: ServerStartup,
        now: Instant,
    ) -> Result<Self, String> {
        let mut process = ServerProcess::start(project)?;
        let input = process
            .child
            .stdin
            .take()
            .ok_or_else(|| "language server stdin is unavailable".to_owned())?;
        let output = process
            .child
            .stdout
            .take()
            .ok_or_else(|| "language server stdout is unavailable".to_owned())?;
        let stderr = process
            .child
            .stderr
            .take()
            .ok_or_else(|| "language server stderr is unavailable".to_owned())?;
        let diagnostics = StderrOutput::capture(stderr);
        let (sender, inbound) = unbounded();
        let server = project.server;
        thread::spawn(move || {
            let mut output = MessageReader::new(BufReader::new(output), server);
            loop {
                match output.read() {
                    Ok(Some(message)) => {
                        if sender.send(Inbound::Message(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(Inbound::Failed(diagnostics.failure(server.command())));
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(Inbound::Failed(error.to_string()));
                        return;
                    }
                }
            }
        });
        let mut session = Self {
            process,
            server: project.server,
            startup,
            input: BufWriter::new(input),
            inbound,
            next_id: 1,
            encoding: PositionEncodingKind::UTF16,
            documents: HashMap::new(),
            state: State::Stopped,
        };
        if let Err(error) = session.initialize(&project.root, now) {
            return Err(session.startup_failure(error));
        }
        Ok(session)
    }

    fn startup_failure(&self, write_error: String) -> String {
        let deadline = Instant::now() + Duration::from_millis(200);
        while let Ok(message) = self.inbound.recv_deadline(deadline) {
            if let Inbound::Failed(error) = message {
                return error;
            }
        }
        write_error
    }

    pub(super) fn inbound(&self) -> &Receiver<Inbound> {
        &self.inbound
    }

    pub(super) fn is_ready(&self) -> bool {
        matches!(self.state, State::Ready)
    }

    pub(super) fn is_stopped(&self) -> bool {
        matches!(self.state, State::Stopped)
    }

    pub(super) fn active_query(&self) -> Option<&Query> {
        match &self.state {
            State::Querying { query, .. } | State::Retrying { query, .. } => Some(query),
            _ => None,
        }
    }

    pub(super) fn next_deadline(&self) -> Option<Instant> {
        match self.state {
            State::Initializing { deadline, .. }
            | State::Quiescing { deadline }
            | State::Querying { deadline, .. }
            | State::ShuttingDown { deadline, .. } => Some(deadline),
            State::Retrying { retry_at, .. } => Some(retry_at),
            State::Ready | State::Stopped => None,
        }
    }

    #[allow(deprecated)] // TypeScript still uses rootUri to find its project dependencies.
    fn initialize(&mut self, root: &Path, now: Instant) -> Result<(), String> {
        let uri = path_uri(root)?;
        let capabilities = ClientCapabilities {
            experimental: (self.server == LanguageServer::RustAnalyzer)
                .then(|| serde_json::json!({ "serverStatusNotification": true })),
            general: Some(lsp_types::GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF8,
                    PositionEncodingKind::UTF16,
                ]),
                ..Default::default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                hover: Some(HoverClientCapabilities {
                    content_format: Some(vec![MarkupKind::Markdown]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(uri.clone()),
            initialization_options: self.server.initialization_options(),
            capabilities,
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: uri.clone(),
                name: root
                    .file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("workspace")
                    .to_owned(),
            }]),
            client_info: Some(lsp_types::ClientInfo {
                name: "progressive-reviewer".to_owned(),
                version: None,
            }),
            ..Default::default()
        };
        let id = RequestId::from(0);
        self.write(&Request::new(id.clone(), "initialize".to_owned(), params).into())?;
        self.state = State::Initializing {
            id,
            deadline: now
                + if self.process.uses_direnv {
                    STARTUP_TIMEOUT
                } else {
                    INITIALIZE_TIMEOUT
                },
        };
        Ok(())
    }

    pub(super) fn request(
        &mut self,
        operation: Operation,
        query: Query,
        now: Instant,
    ) -> Result<(), String> {
        self.start_query(operation, query, 0, now)
    }

    fn start_query(
        &mut self,
        operation: Operation,
        query: Query,
        attempt: u8,
        now: Instant,
    ) -> Result<(), String> {
        let text = fs::read_to_string(&query.path)
            .map_err(|error| format!("could not read {}: {error}", query.path.display()))?;
        let line = text
            .lines()
            .nth(usize::try_from(query.line).map_err(|_| "source line is too large")?)
            .ok_or_else(|| "the source changed; wait for the review to refresh".to_owned())?;
        if line != query.expected_line || !line.is_char_boundary(query.byte_column) {
            return Err("the source changed; wait for the review to refresh".to_owned());
        }
        let uri = path_uri(&query.path)?;
        self.sync_document(&query.path, uri.clone(), &text)?;
        let character = encoded_column(line, query.byte_column, &self.encoding)?;
        let position = TextDocumentPositionParams::new(
            TextDocumentIdentifier::new(uri),
            Position::new(query.line, character),
        );
        let id = RequestId::from(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let message = match operation {
            Operation::Hover => {
                let params = HoverParams {
                    text_document_position_params: position,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                };
                Request::new(
                    id.clone(),
                    lsp_types::request::HoverRequest::METHOD.to_owned(),
                    params,
                )
            }
            Operation::Definition => {
                let params = GotoDefinitionParams {
                    text_document_position_params: position,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                };
                Request::new(
                    id.clone(),
                    lsp_types::request::GotoDefinition::METHOD.to_owned(),
                    params,
                )
            }
            Operation::References => {
                let params = ReferenceParams {
                    text_document_position: position,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                };
                Request::new(
                    id.clone(),
                    lsp_types::request::References::METHOD.to_owned(),
                    params,
                )
            }
        };
        self.write(&message.into())?;
        self.state = State::Querying {
            id,
            operation,
            query,
            attempt,
            deadline: now + INITIALIZE_TIMEOUT,
        };
        Ok(())
    }

    pub(super) fn open_document(&mut self, path: &Path) -> Result<(), String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        self.sync_document(path, path_uri(path)?, &text)
    }

    fn normalize_locations(&self, locations: Vec<ServerLocation>) -> Vec<SourceLocation> {
        locations
            .into_iter()
            .filter_map(|location| location.normalize(&self.encoding))
            .collect()
    }

    fn sync_document(&mut self, path: &Path, uri: Uri, text: &str) -> Result<(), String> {
        let changed_version = if let Some((old, version)) = self.documents.get_mut(path) {
            if old == text {
                None
            } else {
                *version = version.saturating_add(1);
                old.clear();
                old.push_str(text);
                Some(*version)
            }
        } else {
            self.write(
                &Notification::new(
                    lsp_types::notification::DidOpenTextDocument::METHOD.to_owned(),
                    DidOpenTextDocumentParams {
                        text_document: TextDocumentItem::new(
                            uri,
                            Language::for_path(path)
                                .ok_or("No language server supports this file")?
                                .id
                                .to_owned(),
                            1,
                            text.to_owned(),
                        ),
                    },
                )
                .into(),
            )?;
            self.documents.insert(path.to_owned(), (text.to_owned(), 1));
            return Ok(());
        };
        if let Some(version) = changed_version {
            self.write(
                &Notification::new(
                    lsp_types::notification::DidChangeTextDocument::METHOD.to_owned(),
                    DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier::new(uri, version),
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: text.to_owned(),
                        }],
                    },
                )
                .into(),
            )?;
        }
        Ok(())
    }

    pub(super) fn handle(
        &mut self,
        inbound: Inbound,
        now: Instant,
    ) -> Result<Option<Event>, String> {
        match inbound {
            Inbound::Message(Message::Request(request)) => {
                self.respond(request)?;
                Ok(None)
            }
            Inbound::Message(Message::Notification(notification)) => {
                if notification.method != SERVER_STATUS_METHOD
                    || !matches!(self.state, State::Quiescing { .. })
                {
                    return Ok(None);
                }
                let status: ServerStatus = serde_json::from_value(notification.params)
                    .map_err(|error| format!("invalid rust-analyzer status: {error}"))?;
                if status.quiescent {
                    self.state = State::Ready;
                    Ok(Some(Event::Ready(self.startup)))
                } else {
                    Ok(None)
                }
            }
            Inbound::Message(Message::Response(response)) => self.handle_response(response, now),
            Inbound::Failed(error) => Err(error),
        }
    }

    fn handle_response(
        &mut self,
        response: Response,
        now: Instant,
    ) -> Result<Option<Event>, String> {
        let state = std::mem::replace(&mut self.state, State::Stopped);
        match state {
            State::Initializing { id, .. } if response.id == id => {
                let result: InitializeResult = response_value(response)?;
                self.encoding = result
                    .capabilities
                    .position_encoding
                    .unwrap_or(PositionEncodingKind::UTF16);
                self.write(
                    &Notification::new(
                        lsp_types::notification::Initialized::METHOD.to_owned(),
                        InitializedParams {},
                    )
                    .into(),
                )?;
                if self.server == LanguageServer::RustAnalyzer {
                    self.state = State::Quiescing {
                        deadline: now + STARTUP_TIMEOUT,
                    };
                    Ok(None)
                } else {
                    self.state = State::Ready;
                    Ok(Some(Event::Ready(self.startup)))
                }
            }
            State::Querying {
                id,
                operation,
                query,
                attempt,
                ..
            } if response.id == id => {
                if response
                    .response_result
                    .as_ref()
                    .is_err_and(|error| error.message == "content modified")
                    && attempt < 9
                {
                    self.state = State::Retrying {
                        operation,
                        query,
                        attempt: attempt + 1,
                        retry_at: now + Duration::from_millis(100),
                    };
                    return Ok(None);
                }
                self.state = State::Ready;
                Ok(Some(self.query_event(operation, query, response)))
            }
            State::ShuttingDown { id, .. } if response.id == id => {
                self.write(&Notification::new("exit".to_owned(), ()).into())?;
                self.state = State::Stopped;
                Ok(None)
            }
            state => {
                self.state = state;
                Ok(None)
            }
        }
    }

    fn query_event(&self, operation: Operation, query: Query, response: Response) -> Event {
        let failed_snapshot_id = query.snapshot_id.clone();
        let result = match operation {
            Operation::Hover => {
                response_value::<Option<lsp_types::Hover>>(response).map(|hover| Event::Hover {
                    toast_id: query.toast_id,
                    snapshot_id: query.snapshot_id,
                    markdown: hover.map(|hover| hover_markdown(hover.contents)),
                })
            }
            Operation::Definition => response_value::<Option<GotoDefinitionResponse>>(response)
                .map(|locations| Event::Locations {
                    toast_id: query.toast_id,
                    operation,
                    snapshot_id: query.snapshot_id,
                    locations: self.normalize_locations(ServerLocation::from_definition(locations)),
                }),
            Operation::References => {
                response_value::<Option<Vec<Location>>>(response).map(|locations| {
                    Event::Locations {
                        toast_id: query.toast_id,
                        operation,
                        snapshot_id: query.snapshot_id,
                        locations: self.normalize_locations(
                            locations
                                .unwrap_or_default()
                                .into_iter()
                                .filter_map(|location| ServerLocation::from_lsp(&location))
                                .collect(),
                        ),
                    }
                })
            }
        };
        result.unwrap_or_else(|message| Event::Failed {
            toast_id: Some(query.toast_id),
            snapshot_id: Some(failed_snapshot_id),
            message,
        })
    }

    pub(super) fn handle_deadline(&mut self, now: Instant) -> Result<Option<Event>, String> {
        let state = std::mem::replace(&mut self.state, State::Stopped);
        match state {
            State::Initializing { deadline, .. } if now >= deadline => {
                Err(format!("{} did not respond", self.server.command()))
            }
            State::Quiescing { deadline } if now >= deadline => {
                Err(format!("{} did not finish startup", self.server.command()))
            }
            State::Retrying {
                operation,
                query,
                attempt,
                retry_at,
            } if now >= retry_at => {
                self.state = State::Retrying {
                    operation,
                    query: query.clone(),
                    attempt,
                    retry_at,
                };
                self.start_query(operation, query, attempt, now)?;
                Ok(None)
            }
            State::Querying {
                query, deadline, ..
            } if now >= deadline => {
                self.state = State::Ready;
                Ok(Some(Event::Failed {
                    toast_id: Some(query.toast_id),
                    snapshot_id: Some(query.snapshot_id),
                    message: format!("{} did not respond", self.server.command()),
                }))
            }
            State::ShuttingDown { deadline, .. } if now >= deadline => {
                self.write(&Notification::new("exit".to_owned(), ()).into())?;
                self.state = State::Stopped;
                Ok(None)
            }
            state => {
                self.state = state;
                Ok(None)
            }
        }
    }

    pub(super) fn begin_shutdown(&mut self, now: Instant) -> Result<(), String> {
        let id = RequestId::from(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.write(&Request::new(id.clone(), "shutdown".to_owned(), ()).into())?;
        self.state = State::ShuttingDown {
            id,
            deadline: now + Duration::from_secs(1),
        };
        Ok(())
    }

    fn respond(&mut self, request: Request) -> Result<(), String> {
        let result = match request.method.as_str() {
            "workspace/configuration" => request
                .params
                .get("items")
                .and_then(serde_json::Value::as_array)
                .map_or_else(
                    || serde_json::json!([]),
                    |items| serde_json::Value::Array(vec![serde_json::Value::Null; items.len()]),
                ),
            "workspace/workspaceFolders"
            | "client/registerCapability"
            | "client/unregisterCapability"
            | "window/workDoneProgress/create" => serde_json::Value::Null,
            _ => {
                return self.write(
                    &Response::new_err(
                        request.id,
                        ErrorCode::MethodNotFound as i32,
                        "method is not supported".to_owned(),
                    )
                    .into(),
                );
            }
        };
        self.write(&Response::new_ok(request.id, result).into())
    }

    fn write(&mut self, message: &Message) -> Result<(), String> {
        message
            .write(&mut self.input)
            .map_err(|error| format!("could not write to {}: {error}", self.server.command()))
    }
}

fn response_value<R: serde::de::DeserializeOwned>(response: Response) -> Result<R, String> {
    match response.response_result {
        Ok(value) => serde_json::from_value(value).map_err(|error| error.to_string()),
        Err(error) => Err(error.message),
    }
}

#[cfg(all(test, unix))]
#[path = "session.tests.rs"]
pub(super) mod tests;
