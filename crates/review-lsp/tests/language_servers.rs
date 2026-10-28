use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use review_lsp::{Event, FileChange, FileChangeKind, Operation, Query, Worker};

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
            // Expert indexes the project after replying to initialize: ask again once it
            // answered, until it finds something. The deadline guards against a hang.
            if !empty || Instant::now() >= deadline {
                return event;
            }
        }
    }
}

#[test]
#[ignore = "requires tsgo, TypeScript 7, or typescript-language-server with TypeScript"]
fn typescript_navigation() {
    Fixture::typescript().verify();
}

#[test]
#[ignore = "requires expert and a supported Elixir/OTP installation"]
fn expert_navigation() {
    Fixture::elixir().verify();
}

/// An umbrella project whose function `Orchestrator.Service.start_container/2` is called through
/// an alias, from its own app, from its tests and from another app, while other modules define
/// functions of the same name.
struct Umbrella {
    directory: tempfile::TempDir,
}

const SERVICE_LINE: &str = "  def start_container(name, options) do";

impl Umbrella {
    fn new() -> Self {
        let umbrella = Self {
            directory: tempfile::Builder::new()
                .prefix("reviewer-lsp-")
                .tempdir()
                .unwrap(),
        };
        umbrella.write("mix.exs", "defmodule Umbrella.MixProject do\n  use Mix.Project\n  def project, do: [apps_path: \"apps\", version: \"0.1.0\", deps: []]\nend\n");
        for (app, module, deps) in [
            ("orchestrator", "Orchestrator", "[]"),
            ("web", "Web", "[{:orchestrator, in_umbrella: true}]"),
        ] {
            umbrella.write(&format!("apps/{app}/mix.exs"), &format!("defmodule {module}.MixProject do\n  use Mix.Project\n  def project, do: [app: :{app}, version: \"0.1.0\", build_path: \"../../_build\", deps_path: \"../../deps\", lockfile: \"../../mix.lock\", elixir: \">= 1.15.0\", deps: {deps}]\n  def application, do: []\nend\n"));
        }
        umbrella.write("apps/orchestrator/lib/service.ex", &format!("defmodule Orchestrator.Service do\n{SERVICE_LINE}\n    {{name, options}}\n  end\nend\n"));
        umbrella.write("apps/orchestrator/lib/other.ex", "defmodule Orchestrator.Other do\n  def start_container(name), do: name\n  def start_container(name, a, b), do: {name, a, b}\nend\n");
        umbrella.write("apps/orchestrator/lib/controller.ex", "defmodule Orchestrator.Controller do\n  alias Orchestrator.Service\n\n  def create(name) do\n    Service.start_container(name, [])\n  end\nend\n");
        umbrella.write("apps/orchestrator/test/service_test.exs", "defmodule Orchestrator.ServiceTest do\n  use ExUnit.Case\n  test \"starts\" do\n    assert Orchestrator.Service.start_container(:a, []) == {:a, []}\n  end\nend\n");
        umbrella.write("apps/web/lib/page.ex", "defmodule Web.Page do\n  alias Orchestrator.Service\n  def show(name), do: Service.start_container(name, [])\nend\n");
        umbrella
    }

    fn path(&self, path: &str) -> PathBuf {
        self.directory.path().join(path)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.path(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// The references of `start_container/2`, as `path:line` from the umbrella root, and the
    /// files the server has not indexed.
    fn references(&self, worker: &Worker) -> (Vec<String>, usize) {
        worker
            .request(
                Operation::References,
                Query {
                    toast_id: toasts::ToastId::generate(),
                    path: self.path("apps/orchestrator/lib/service.ex"),
                    line: 1,
                    byte_column: SERVICE_LINE.find("start_container").unwrap(),
                    expected_line: SERVICE_LINE.to_owned(),
                    snapshot_id: "umbrella".to_owned(),
                },
            )
            .unwrap();
        // A restarted server reports its startup first.
        let event = worker
            .event_receiver()
            .iter()
            .find(|event| !matches!(event, Event::Initializing(_) | Event::Ready(_)))
            .unwrap();
        let Event::Locations {
            locations,
            unindexed_files,
            ..
        } = event
        else {
            panic!("{event:?}");
        };
        let locations = locations
            .iter()
            .map(|location| {
                let path = location.path.strip_prefix(self.directory.path()).unwrap();
                format!("{}:{}", path.display(), location.line)
            })
            .collect();
        (locations, unindexed_files)
    }

    /// Asks for the references until `done` holds, each answer pacing the next request:
    /// Expert reports no event when it has indexed a file. The deadline guards against a
    /// hang.
    fn references_until(
        &self,
        worker: &Worker,
        done: impl Fn(&[String]) -> bool,
    ) -> (Vec<String>, usize) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let references = self.references(worker);
            if done(&references.0) || Instant::now() >= deadline {
                return references;
            }
        }
    }
}

#[test]
#[ignore = "requires expert and a supported Elixir/OTP installation"]
fn expert_finds_aliased_callers_and_reports_the_changes_it_has_not_indexed() {
    let umbrella = Umbrella::new();
    let worker = Worker::start(umbrella.directory.path().to_owned());
    let events = worker.event_receiver();
    let page = umbrella.path("apps/web/lib/page.ex");
    worker.open_document(page.clone()).unwrap();
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(60)).unwrap(),
        Event::Initializing(_)
    ));
    let ready = events.recv_timeout(Duration::from_secs(60)).unwrap();
    assert!(matches!(ready, Event::Ready(_)), "{ready:?}");

    let (references, unindexed) =
        umbrella.references_until(&worker, |references| !references.is_empty());
    assert_eq!(
        references,
        [
            "apps/orchestrator/lib/service.ex:1",
            "apps/orchestrator/lib/controller.ex:4",
            "apps/orchestrator/test/service_test.exs:3",
            "apps/web/lib/page.ex:2",
        ]
    );
    assert_eq!(unindexed, 0);

    // An agent adds a caller in a new file and another in the open document.
    umbrella.write("apps/web/lib/late.ex", "defmodule Web.Late do\n  alias Orchestrator.Service\n  def run(name), do: Service.start_container(name, [])\nend\n");
    umbrella.write("apps/web/lib/page.ex", "defmodule Web.Page do\n  alias Orchestrator.Service\n  def show(name), do: name\n  def again(name), do: Service.start_container(name, [])\nend\n");
    worker.file_change_sender().send(vec![
        FileChange {
            path: umbrella.path("apps/web/lib/late.ex"),
            kind: FileChangeKind::Created,
        },
        FileChange {
            path: page,
            kind: FileChangeKind::Changed,
        },
    ]);
    // Expert reindexes the open document, which changed through the editor's protocol, and
    // misses the new file until it restarts: the list says so.
    let (references, unindexed) = umbrella.references_until(&worker, |references| {
        references
            .iter()
            .any(|reference| reference == "apps/web/lib/page.ex:3")
    });
    assert!(
        references
            .iter()
            .any(|reference| reference == "apps/web/lib/page.ex:3"),
        "{references:?}"
    );
    assert_eq!(unindexed, 1, "{references:?}");
    assert!(
        !references
            .iter()
            .any(|reference| reference.starts_with("apps/web/lib/late.ex")),
        "Expert indexed a file changed on disk: {references:?}"
    );

    worker.restart().unwrap();
    let (references, unindexed) = umbrella.references_until(&worker, |references| {
        references
            .iter()
            .any(|reference| reference.starts_with("apps/web/lib/late.ex"))
    });
    assert!(
        references.contains(&"apps/web/lib/late.ex:2".to_owned()),
        "{references:?}"
    );
    assert_eq!(unindexed, 0);
}
