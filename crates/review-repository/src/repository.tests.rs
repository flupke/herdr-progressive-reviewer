use std::ffi::OsStr;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use super::{Cancellation, ChangeKind, ChangedFile, RepoPath, RepositoryProcess, ShortRevision};
use crate::Error;

#[test]
fn a_jj_revision_splits_where_jj_colours_its_prefix_and_its_rest() {
    // `jj log --color=always -T 'change_id.shortest(8)'`
    let display_id = "\u{1b}[1m\u{1b}[38;5;5mm\u{1b}[0m\u{1b}[38;5;8mtsvqnzp\u{1b}[39m";

    assert_eq!(
        ShortRevision::of(display_id),
        ShortRevision {
            prefix: "m".to_owned(),
            rest: "tsvqnzp".to_owned(),
        }
    );
}

#[test]
fn a_revision_without_a_prefix_coloured_apart_is_all_rest() {
    // A Git abbreviation, then a jj one whose prefix and rest share a colour.
    for (display_id, plain) in [
        ("3f2a9c1", "3f2a9c1"),
        ("\u{1b}[38;5;5mmtsvqnzp\u{1b}[39m", "mtsvqnzp"),
    ] {
        assert_eq!(
            ShortRevision::of(display_id),
            ShortRevision {
                prefix: String::new(),
                rest: plain.to_owned(),
            },
            "{display_id:?}"
        );
    }
}

#[test]
fn repository_paths_display_valid_utf8() {
    let path = RepoPath::from_bytes("Каталог/файл.md".as_bytes());

    assert_eq!(path.display(), "Каталог/файл.md");
}

#[test]
fn repository_paths_escape_control_characters() {
    let path = RepoPath::from_bytes("Каталог/\nфайл.md".as_bytes());

    assert_eq!(path.display(), r"Каталог/\nфайл.md");
}

#[test]
fn repository_paths_preserve_ascii_escaping() {
    let path = RepoPath::from_bytes(b"quote-\"-\\-\n.txt");

    assert_eq!(path.display(), r#"quote-\"-\\-\n.txt"#);
}

#[test]
fn repository_paths_preserve_non_utf8_bytes() {
    let path = RepoPath::from_bytes(b"invalid-\xff.txt");

    assert_eq!(path.display(), r"invalid-\xff.txt");
}

#[test]
fn cancellation_stops_a_child_command() {
    let cancellation = Cancellation::default();
    cancellation.cancel();

    let error = RepositoryProcess::new("jj", Path::new("."), "test jj cancellation", &cancellation)
        .output(["version"])
        .unwrap_err();

    assert!(matches!(error, Error::CommandCancelled { .. }));
}

/// How long a test waits for a woken command before it fails; it fires only on a hang.
const HANG_GUARD: Duration = Duration::from_secs(10);

/// Run `work` on another thread: where its result arrives.
fn in_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (finished, result) = mpsc::channel();
    thread::spawn(move || {
        let _ = finished.send(work());
    });
    result
}

/// A command that runs a script in `sh`, as a repository command runs jj or git.
fn shell(cancellation: &Cancellation) -> RepositoryProcess<'_> {
    RepositoryProcess {
        options: &["-c"],
        ..RepositoryProcess::new("sh", Path::new("."), "test shell", cancellation)
    }
}

#[test]
fn a_command_that_prints_more_than_its_limit_is_stopped() {
    let result = in_thread(|| {
        let cancellation = Cancellation::default();
        RepositoryProcess {
            output_limit: 64 * 1024,
            ..shell(&cancellation)
        }
        .output(["exec yes"])
    });

    let error = result
        .recv_timeout(HANG_GUARD)
        .expect("the size limit stops the command")
        .unwrap_err();
    assert!(matches!(error, Error::CommandOutputTooLarge { .. }));
}

#[test]
fn a_cancellation_stops_a_running_child_that_closed_its_output() {
    let directory = tempfile::tempdir().unwrap();
    let started = directory.path().join("started");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&started)
            .status()
            .unwrap()
            .success()
    );
    let cancellation = Cancellation::default();
    let result = in_thread({
        let cancellation = cancellation.clone();
        let started = started.clone();
        move || {
            shell(&cancellation).output([
                OsStr::new(r#"exec >/dev/null 2>&1; echo > "$0"; exec sleep 60"#),
                started.as_os_str(),
            ])
        }
    });
    // The script writes to the FIFO once it runs; reading it waits for that.
    in_thread(move || std::fs::read(started))
        .recv_timeout(HANG_GUARD)
        .expect("the command starts")
        .unwrap();

    cancellation.cancel();

    let error = result
        .recv_timeout(HANG_GUARD)
        .expect("the cancellation stops the command")
        .unwrap_err();
    assert!(matches!(error, Error::CommandCancelled { .. }));
}

/// How a command waits for its child on Linux, where a watcher thread wakes it.
#[cfg(target_os = "linux")]
mod wakes {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    /// A command waiting on `cancellation` on another thread until `settled`: where it reports
    /// whether it was cancelled, once it has checked `settled` the first time.
    fn waiting_command(cancellation: &Cancellation, settled: &Arc<AtomicBool>) -> Receiver<bool> {
        let (started, waiting) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let cancellation = cancellation.clone();
        let settled = Arc::clone(settled);
        thread::spawn(move || {
            let cancelled = cancellation.wait_until(|| {
                let _ = started.send(());
                settled.load(Ordering::SeqCst)
            });
            let _ = finished.send(cancelled);
        });
        waiting
            .recv_timeout(HANG_GUARD)
            .expect("the command waits on the cancellation");
        result
    }

    #[test]
    fn a_cancellation_wakes_a_command_waiting_on_its_child() {
        let cancellation = Cancellation::default();
        let result = waiting_command(&cancellation, &Arc::default());

        cancellation.cancel();

        assert!(
            result
                .recv_timeout(HANG_GUARD)
                .expect("the cancellation wakes the command")
        );
    }

    #[test]
    fn a_command_wakes_once_its_child_is_over() {
        let cancellation = Cancellation::default();
        let settled = Arc::new(AtomicBool::new(false));
        let result = waiting_command(&cancellation, &settled);

        settled.store(true, Ordering::SeqCst);
        cancellation.wake();

        assert!(
            !result
                .recv_timeout(HANG_GUARD)
                .expect("waking the command makes it check again")
        );
    }
}

#[test]
fn rename_diff_paths_include_each_distinct_side_once() {
    let renamed = ChangedFile {
        old_path: Some(RepoPath::from_bytes(b"old.rs")),
        new_path: Some(RepoPath::from_bytes(b"new.rs")),
        change: ChangeKind::Renamed,
        ..ChangedFile::modified("new.rs")
    };
    let unchanged = ChangedFile::modified("same.rs");

    assert_eq!(
        renamed
            .diff_paths()
            .map(RepoPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"old.rs".as_slice(), b"new.rs".as_slice()]
    );
    assert_eq!(
        unchanged
            .diff_paths()
            .map(RepoPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"same.rs".as_slice()]
    );
}
