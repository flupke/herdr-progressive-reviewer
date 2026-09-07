use std::path::Path;

use crate::api::{Operation, Query};

use super::*;

fn request(path: &Path) -> Command {
    Command::Request {
        operation: Operation::Definition,
        query: Query {
            toast_id: toasts::ToastId::generate(),
            path: path.to_owned(),
            line: 0,
            byte_column: 0,
            expected_line: String::new(),
            snapshot_id: "snapshot".to_owned(),
        },
    }
}

#[test]
fn mixed_projects_route_documents_and_requests_to_independent_servers() {
    let repository = tempfile::tempdir().unwrap();
    let (events, _receiver) = unbounded();
    let mut manager = Manager::new(repository.path().to_owned(), events);
    let mut channels = Vec::new();
    for name in ["lib.rs", "lib.ex", "index.ts"] {
        let path = repository.path().join(name);
        std::fs::write(&path, "").unwrap();
        let project = Project::for_document(repository.path(), &path).unwrap();
        let (commands, receiver) = unbounded();
        manager.servers.insert(
            project,
            ServerWorker {
                commands,
                handle: None,
            },
        );
        channels.push((path, receiver));
    }
    for (path, _) in &channels {
        manager.route(Command::OpenDocument(path.clone()));
        manager.route(request(path));
    }
    for (path, receiver) in &channels {
        assert_eq!(
            receiver.recv().unwrap(),
            Command::OpenDocument(path.clone())
        );
        assert!(
            matches!(receiver.recv().unwrap(), Command::Request { query, .. } if query.path == *path)
        );
        assert!(receiver.is_empty());
    }

    let (commands, receiver) = unbounded();
    commands.send(Command::Restart).unwrap();
    commands.send(Command::Shutdown).unwrap();
    manager.run(&receiver);
    drop(manager);
    for (_, receiver) in channels {
        assert_eq!(receiver.recv().unwrap(), Command::Restart);
        assert_eq!(receiver.recv().unwrap(), Command::Shutdown);
    }
}

#[test]
fn unsupported_files_and_deleted_documents_do_not_start_servers() {
    let repository = tempfile::tempdir().unwrap();
    let (events, receiver) = unbounded();
    let mut manager = Manager::new(repository.path().to_owned(), events);
    manager.route(Command::OpenDocument(repository.path().join("README.md")));
    manager.route(Command::OpenDocument(repository.path().join("deleted.ex")));
    assert!(receiver.is_empty());
    assert!(manager.servers.is_empty());

    let command = request(&repository.path().join("README.md"));
    let Command::Request { query, .. } = &command else {
        unreachable!()
    };
    let id = query.toast_id;
    manager.route(command);
    assert!(matches!(receiver.recv().unwrap(), Event::Failed {
        toast_id: Some(toast_id), snapshot_id: Some(snapshot_id), ..
    } if toast_id == id && snapshot_id == "snapshot"));
    assert!(manager.servers.is_empty());
}
