use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use review_lsp::{Event, Operation, Query, Worker};

struct Fixture {
    directory: tempfile::TempDir,
    definition: PathBuf,
    usage: PathBuf,
    line: u32,
    text: &'static str,
}

impl Fixture {
    fn typescript() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("reviewer-lsp-")
            .tempdir()
            .unwrap();
        fs::write(directory.path().join("tsconfig.json"), "{}").unwrap();
        let definition = directory.path().join("answer.ts");
        fs::write(
            &definition,
            "/** Returns forty-two. */\nexport function value(): number { return 42; }\n",
        )
        .unwrap();
        let usage = directory.path().join("usage.ts");
        let text = "export const result = value();";
        fs::write(
            &usage,
            format!("import {{ value }} from './answer';\n{text}\n"),
        )
        .unwrap();
        Self {
            directory,
            definition,
            usage,
            line: 1,
            text,
        }
    }

    fn elixir() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("reviewer-lsp-")
            .tempdir()
            .unwrap();
        fs::write(directory.path().join("mix.exs"), "defmodule Smoke.MixProject do\n  use Mix.Project\n  def project, do: [app: :smoke, version: \"0.1.0\", elixir: \">= 1.15.0\"]\n  def application, do: []\nend\n").unwrap();
        fs::create_dir(directory.path().join("lib")).unwrap();
        let definition = directory.path().join("lib/answer.ex");
        fs::write(
            &definition,
            "defmodule Answer do\n  @doc \"Returns forty-two.\"\n  def value, do: 42\nend\n",
        )
        .unwrap();
        let usage = directory.path().join("lib/usage.ex");
        let text = "  def run, do: Answer.value()";
        fs::write(&usage, format!("defmodule Usage do\n{text}\nend\n")).unwrap();
        Self {
            directory,
            definition,
            usage,
            line: 1,
            text,
        }
    }

    fn verify(&self) {
        let worker = Worker::start(self.directory.path().to_owned());
        let events = worker.event_receiver();
        let timeout = Duration::from_secs(60);
        worker.open_document(self.usage.clone()).unwrap();
        assert!(matches!(
            events.recv_timeout(timeout).unwrap(),
            Event::Initializing(_)
        ));
        let ready = events.recv_timeout(timeout).unwrap();
        assert!(matches!(ready, Event::Ready(_)), "{ready:?}");

        for operation in [
            Operation::Definition,
            Operation::Hover,
            Operation::References,
        ] {
            let event = self.query_until_indexed(&worker, operation);
            match (operation, &event) {
                (Operation::Definition, Event::Locations { locations, .. }) => assert!(
                    locations
                        .iter()
                        .any(|location| location.path == self.definition),
                    "{event:?}",
                ),
                (Operation::References, Event::Locations { locations, .. }) => assert!(
                    locations.iter().any(|location| location.path == self.usage),
                    "{event:?}",
                ),
                (Operation::Hover, Event::Hover { markdown, .. }) => {
                    assert!(
                        markdown.as_ref().is_some_and(|text| !text.is_empty()),
                        "{event:?}"
                    );
                }
                _ => panic!("{event:?}"),
            }
        }
    }
    fn query_until_indexed(&self, worker: &Worker, operation: Operation) -> Event {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            worker
                .request(
                    operation,
                    Query {
                        toast_id: toasts::ToastId::generate(),
                        path: self.usage.clone(),
                        line: self.line,
                        byte_column: self.text.find("value").unwrap(),
                        expected_line: self.text.to_owned(),
                        snapshot_id: "smoke".to_owned(),
                    },
                )
                .unwrap();
            let event = worker
                .event_receiver()
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
            let empty = match &event {
                Event::Locations { locations, .. } => locations.is_empty(),
                Event::Hover { markdown, .. } => markdown.is_none(),
                _ => false,
            };
            if !empty || Instant::now() >= deadline {
                return event;
            }
            // Expert indexes the project after replying to initialize.
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
#[ignore = "requires typescript-language-server and TypeScript on PATH"]
fn typescript_navigation() {
    Fixture::typescript().verify();
}

#[test]
#[ignore = "requires expert and a supported Elixir/OTP installation"]
fn expert_navigation() {
    Fixture::elixir().verify();
}
