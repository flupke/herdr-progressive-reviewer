use std::fs;
use std::sync::{Condvar, Mutex, PoisonError};

use tempfile::tempdir;

use super::*;

/// What notified a watcher, in order, and the wake of a test that waits for it.
#[derive(Default)]
pub(super) struct Notifications {
    recorded: Mutex<Vec<Vec<PathBuf>>>,
    changed: Condvar,
}

impl Notifications {
    /// Records `paths` and runs `notify`, which raises the flag, under one lock: a
    /// notification a test sees after [`Observer::skip_to_now`] raised the flag after it.
    pub(super) fn record(&self, paths: &[PathBuf], notify: impl FnOnce()) {
        let mut recorded = self.recorded.lock().unwrap_or_else(PoisonError::into_inner);
        notify();
        recorded.push(paths.to_vec());
        self.changed.notify_all();
    }
}

/// Follows the notifications of a watcher's state.
struct Observer {
    state: Arc<WatchState>,
    /// How many notifications it has seen.
    seen: usize,
}

impl Observer {
    fn of(watcher: &RepositoryWatcher) -> Self {
        Self {
            state: Arc::clone(&watcher.state),
            seen: 0,
        }
    }

    /// Leaves out the notifications so far, the late ones of an earlier change among them:
    /// called before a change, the next wait is about what follows it.
    fn skip_to_now(&mut self) {
        self.seen = self
            .state
            .notifications
            .recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
    }

    /// Waits for the next notification that `matches`, and returns it and every one before it
    /// that this had not seen.
    fn wait_for(&mut self, matches: impl Fn(&[PathBuf]) -> bool) -> Vec<Vec<PathBuf>> {
        self.wait_for_each(&[&matches])
    }

    /// Waits until each of `sentinels` matched a notification, and returns every notification
    /// up to the last of these that this had not seen.
    fn wait_for_each(&mut self, sentinels: &[Sentinel<'_>]) -> Vec<Vec<PathBuf>> {
        let notifications = &self.state.notifications;
        let recorded = notifications
            .recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let seen = self.seen;
        let first_match = |recorded: &[Vec<PathBuf>], matches: &dyn Fn(&[PathBuf]) -> bool| {
            recorded[seen..].iter().position(|paths| matches(paths))
        };
        let (recorded, timeout) = notifications
            .changed
            .wait_timeout_while(recorded, review_test_support::GUARD, |recorded| {
                !sentinels
                    .iter()
                    .all(|matches| first_match(recorded, *matches).is_some())
            })
            .unwrap_or_else(PoisonError::into_inner);
        assert!(
            !timeout.timed_out(),
            "no notification came after the first {seen}: {recorded:#?}"
        );
        let last = seen
            + sentinels
                .iter()
                .filter_map(|matches| first_match(&recorded, *matches))
                .max()
                .unwrap();
        self.seen = last + 1;
        recorded[seen..=last].to_vec()
    }
}

/// What a test looks for in a notification: the paths it is about.
type Sentinel<'a> = &'a dyn Fn(&[PathBuf]) -> bool;

/// Whether a notification is about `path`: a change of it or of a directory above it.
fn about(path: &Path) -> impl Fn(&[PathBuf]) -> bool {
    let path = path.to_owned();
    move |paths| paths.iter().any(|changed| path.starts_with(changed))
}

/// Whether a notification is the one of watches that start.
fn started(paths: &[PathBuf]) -> bool {
    paths.is_empty()
}

/// Waits for the notification `matches` and returns the notifications up to it, then checks
/// that it calls for a refresh once the debounce passed.
fn wait_for_refresh(
    watcher: &mut RepositoryWatcher,
    observer: &mut Observer,
    matches: impl Fn(&[PathBuf]) -> bool,
) -> Vec<Vec<PathBuf>> {
    let notified = observer.wait_for(matches);
    assert!(!watcher.take_failure(), "the watcher failed");
    assert!(refresh_comes_due(watcher), "the change calls for a refresh");
    notified
}

/// Whether a refresh comes due, on a clock this advances by hand: a notification that
/// arrives meanwhile starts the debounce again, and the clock follows it.
fn refresh_comes_due(watcher: &mut RepositoryWatcher) -> bool {
    let mut now = Instant::now();
    for _ in 0..100 {
        if watcher.refresh_due(now) {
            return true;
        }
        now += DEBOUNCE;
    }
    false
}

/// A jj-style plan: only working-tree `.gitignore` files hide paths.
fn working_tree_plan(root: &Path) -> WatchPlan {
    WatchPlan {
        root: root.to_owned(),
        metadata: Vec::new(),
        git_excludes: None,
    }
}

/// A Git-style plan: the repository exclude file also hides paths.
fn git_plan(root: &Path) -> WatchPlan {
    WatchPlan {
        git_excludes: Some(vec![root.join(".git/info/exclude")]),
        ..working_tree_plan(root)
    }
}

#[test]
fn displayed_ignored_tracked_source_refreshes_on_edit_delete_and_recreation() {
    use review_repository::repository::Repository;
    use review_test_support::{GitFixture, ReviewRepositoryFixture};

    let files = GitFixture::new();
    files.write("ignored/nested/source.rs", b"original\n");
    files.commit_all("track the source before ignoring its directory");
    files.write(".gitignore", b"ignored/\n");
    let path = files.root().join("ignored/nested/source.rs");
    let mut watcher =
        RepositoryWatcher::new(Repository::discover(files.root()).unwrap().watch_plan());
    let mut observer = Observer::of(&watcher);
    // The watches of the repository start, then those of the source.
    observer.wait_for(started);
    watcher.source_requests().watch(Some(&path));
    wait_for_refresh(&mut watcher, &mut observer, started);

    let noise = files.root().join("ignored/nested/noise.log");
    // A change that each watch reports, after the change it must not report: the source's
    // watch reports the source, the repository's watch this file at its root. Each watch
    // reports in order, so what it reported of the earlier change came before.
    let sentinel = files.root().join("sentinel.txt");
    observer.skip_to_now();
    fs::write(&noise, "ignored output").unwrap();
    fs::write(&path, "edited\n").unwrap();
    fs::write(&sentinel, "after the noise\n").unwrap();
    let notified = observer.wait_for_each(&[&about(&path), &about(&sentinel)]);
    assert!(
        refresh_comes_due(&mut watcher),
        "the edit calls for a refresh"
    );
    assert!(
        notified.iter().flatten().all(|changed| *changed != noise),
        "unrelated ignored output caused a refresh: {notified:?}"
    );
    let changes: [&dyn Fn(); 6] = [
        &|| {
            fs::write(path.with_extension("tmp"), "atomic replacement\n").unwrap();
            fs::rename(path.with_extension("tmp"), &path).unwrap();
        },
        &|| fs::remove_file(&path).unwrap(),
        &|| fs::write(&path, "recreated\n").unwrap(),
        &|| fs::remove_dir_all(files.root().join("ignored")).unwrap(),
        &|| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "new parent directories\n").unwrap();
        },
        &|| fs::write(&path, "still watched\n").unwrap(),
    ];
    for change in changes {
        observer.skip_to_now();
        change();
        wait_for_refresh(&mut watcher, &mut observer, about(&path));
    }

    let definition = files.root().join("ignored/definition.rs");
    fs::write(&definition, "definition\n").unwrap();
    observer.skip_to_now();
    watcher.source_requests().watch(Some(&definition));
    wait_for_refresh(&mut watcher, &mut observer, started);
    observer.skip_to_now();
    fs::write(&path, "old source is no longer displayed\n").unwrap();
    // The watch of the old source is gone, and what it reported before is no longer about the
    // displayed source: the definition's watch and the repository's are the ones that could
    // report the old source.
    fs::write(&definition, "edited definition\n").unwrap();
    fs::write(&sentinel, "after the old source\n").unwrap();
    let notified = observer.wait_for_each(&[&about(&definition), &about(&sentinel)]);
    assert!(
        refresh_comes_due(&mut watcher),
        "the edit calls for a refresh"
    );
    assert!(
        notified.iter().flatten().all(|changed| *changed != path),
        "previously displayed ignored source is still watched: {notified:?}"
    );
}

#[test]
fn notification_debounces_before_poll() {
    let now = Instant::now();
    let state = Arc::new(WatchState::default());
    let (commands, command_receiver) = mpsc::channel();
    let mut watcher = RepositoryWatcher {
        commands,
        state: Arc::clone(&state),
        next_refresh: None,
    };

    state.notified.store(true, Ordering::Relaxed);
    state.watching.store(true, Ordering::Relaxed);

    assert!(!watcher.refresh_due(now));
    assert!(command_receiver.try_recv().is_err());
    assert!(watcher.refresh_due(now + DEBOUNCE));
    assert!(!watcher.refresh_due(now + DEBOUNCE));
}

#[test]
fn notification_during_watcher_start_keeps_its_debounce_deadline() {
    let now = Instant::now();
    let state = Arc::new(WatchState::default());
    let (commands, _command_receiver) = mpsc::channel();
    let mut watcher = RepositoryWatcher {
        commands,
        state: Arc::clone(&state),
        next_refresh: None,
    };
    state.notified.store(true, Ordering::Relaxed);
    state.watching.store(true, Ordering::Relaxed);

    assert!(!watcher.refresh_due(now));
    assert!(!watcher.refresh_due(now + DEBOUNCE / 2));
    assert!(watcher.refresh_due(now + DEBOUNCE));
}

#[test]
fn disk_content_change_schedules_a_repository_poll() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let path = root.join("source.rs");
    fs::write(&path, "fn before() {}\n").unwrap();
    let mut watcher = RepositoryWatcher::new(working_tree_plan(root));
    let mut observer = Observer::of(&watcher);
    observer.wait_for(started);

    fs::write(&path, "fn after() {}\n").unwrap();

    observer.wait_for(about(&path));
    assert!(
        refresh_comes_due(&mut watcher),
        "content change did not schedule a poll"
    );
}

#[test]
fn access_events_do_not_refresh_the_repository() {
    assert!(!should_process(&Event::new(EventKind::Access(
        notify::event::AccessKind::Any,
    ))));
    assert!(!should_process(&Event::new(EventKind::Other)));
    assert!(should_process(&Event::new(EventKind::Modify(
        ModifyKind::Any,
    ))));
}

#[test]
fn ignored_and_repository_metadata_paths_are_not_watched() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join(".gitignore"), "ignored/\n*.log\n").unwrap();
    for path in ["kept", "ignored", ".git", ".jj"] {
        fs::create_dir(root.join(path)).unwrap();
    }
    let rules = IgnoreRules::discover(&working_tree_plan(root));

    assert!(rules.directories.contains(&root.to_owned()));
    assert!(rules.directories.contains(&root.join("kept")));
    assert!(!rules.directories.contains(&root.join("ignored")));
    assert!(!rules.directories.contains(&root.join(".git")));
    assert!(!rules.directories.contains(&root.join(".jj")));
    assert!(!rules.includes(&root.join("debug.log"), false));
    assert!(rules.includes(&root.join("source.rs"), false));
}

#[test]
fn standard_git_excludes_are_not_watched() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".git/info/exclude"), "ignored/\n").unwrap();
    fs::create_dir(root.join("ignored")).unwrap();
    let rules = IgnoreRules::discover(&git_plan(root));

    assert!(!rules.directories.contains(&root.join("ignored")));
    assert!(!rules.includes(&root.join("ignored"), true));
}

#[test]
fn repository_root_stops_parent_gitignore_rules() {
    let directory = tempdir().unwrap();
    let root = directory.path().join("repository");
    fs::write(directory.path().join(".gitignore"), "repository/ignored/\n").unwrap();
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::create_dir(root.join("ignored")).unwrap();
    let rules = IgnoreRules::discover(&git_plan(&root));

    assert!(rules.directories.contains(&root.join("ignored")));
    assert!(rules.includes(&root.join("ignored"), true));
}

#[test]
fn failed_watcher_reports_once_without_periodic_scans() {
    let now = Instant::now();
    let state = Arc::new(WatchState::default());
    let (commands, command_receiver) = mpsc::channel();
    let mut watcher = RepositoryWatcher {
        commands,
        state: Arc::clone(&state),
        next_refresh: None,
    };
    state.failed.store(true, Ordering::Relaxed);

    assert!(watcher.take_failure());
    assert!(!watcher.take_failure());
    assert!(!watcher.refresh_due(now));
    assert!(!watcher.refresh_due(now + Duration::from_secs(3600)));
    assert!(command_receiver.try_recv().is_err());
}

#[test]
fn metadata_events_follow_the_planned_recursion() {
    let directory = tempdir().unwrap();
    let root = directory.path().join("work");
    let git = directory.path().join("git");
    let operations = directory.path().join("op_heads");
    let plan = WatchPlan {
        metadata: vec![
            MetadataWatch {
                directory: git.clone(),
                scope: MetadataScope::Entries,
            },
            MetadataWatch {
                directory: operations.clone(),
                scope: MetadataScope::Subtree,
            },
        ],
        ..working_tree_plan(&root)
    };
    let metadata = MetadataWatches::new(&plan);

    let event_at = |path| {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
        event.paths.push(path);
        event
    };
    assert!(metadata.includes(&event_at(operations.join("heads/operation"))));
    assert!(metadata.includes(&event_at(git.join("HEAD"))));
    assert!(!metadata.includes(&event_at(git.join("objects/ab/object"))));
    assert!(!metadata.includes(&event_at(root.join("source.rs"))));
}

#[test]
fn jj_uses_only_gitignores_under_the_repository_root() {
    let directory = tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(
        directory.path().join(".gitignore"),
        "root/parent-ignored/\n",
    )
    .unwrap();
    fs::create_dir(root.join(".jj")).unwrap();
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".git/info/exclude"), "git-ignored/\n").unwrap();
    fs::write(root.join(".gitignore"), "local-ignored/\n").unwrap();
    for path in ["parent-ignored", "git-ignored", "local-ignored"] {
        fs::create_dir(root.join(path)).unwrap();
    }
    let rules = IgnoreRules::discover(&working_tree_plan(&root));

    assert!(rules.directories.contains(&root.join("parent-ignored")));
    assert!(rules.directories.contains(&root.join("git-ignored")));
    assert!(!rules.directories.contains(&root.join("local-ignored")));
    assert_eq!(rules.git_excludes, None);

    fs::remove_dir(root.join(".jj")).unwrap();
    let refreshed = IgnoreRules::discover_subtree(&root, &root, rules.git_excludes.as_deref());
    assert!(refreshed.directories.contains(&root.join("git-ignored")));
    assert_eq!(refreshed.git_excludes, None);
}

#[test]
fn refreshes_only_the_changed_subtree() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let left = root.join("left");
    let right = root.join("right");
    fs::create_dir_all(root.join(".git/info")).unwrap();
    fs::write(root.join(".gitignore"), "gitignored/\n").unwrap();
    fs::write(root.join(".git/info/exclude"), "excluded/\n").unwrap();
    fs::create_dir(&left).unwrap();
    fs::create_dir(&right).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(&git_plan(root), &state, &commands).unwrap();
    let new_directory = left.join("new");
    fs::create_dir(&new_directory).unwrap();

    watcher.refresh_subtree(&left).unwrap();

    assert!(watcher.rules.directories.contains(&right));
    assert!(watcher.rules.directories.contains(&new_directory));
    assert!(!watcher.rules.includes(&root.join("gitignored"), true));
    assert!(!watcher.rules.includes(&root.join("excluded"), true));
}

#[test]
fn external_ignore_change_refreshes_watches() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let exclude = root.join(".git/info/exclude");
    let ignored = root.join("ignored");
    fs::create_dir(root.join(".git")).unwrap();
    fs::create_dir(&ignored).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(&git_plan(root), &state, &commands).unwrap();
    assert!(watcher.rules.directories.contains(&ignored));

    fs::create_dir(root.join(".git/info")).unwrap();
    fs::write(&exclude, "ignored/\n").unwrap();
    let mut event = Event::new(EventKind::Create(CreateKind::Folder));
    event.paths.push(root.join(".git/info"));
    watcher.update(&event, &state).unwrap();

    assert!(!watcher.rules.directories.contains(&ignored));
    assert!(
        watcher
            .external_directories
            .contains(&root.join(".git/info"))
    );
}

#[test]
fn unrelated_paths_do_not_change_external_ignore_rules() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join(".git/info")).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let watcher = ActiveWatcher::start(&git_plan(root), &state, &commands).unwrap();
    let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
    event.paths.push(root.join("source.rs"));

    assert!(!watcher.changes_external_rules(&event));
}

#[test]
fn directory_create_events_refresh_the_changed_subtree_only() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let parent = root.join("parent");
    fs::create_dir(&parent).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(&working_tree_plan(root), &state, &commands).unwrap();
    let child = parent.join("child");
    fs::create_dir(&child).unwrap();
    let mut event = Event::new(EventKind::Create(CreateKind::Folder));
    event.paths.push(child.clone());

    watcher.refresh_changed_directories(&event).unwrap();

    assert!(watcher.rules.directories.contains(&child));
}

#[test]
fn content_modify_events_do_not_refresh_directory_rules() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(&working_tree_plan(root), &state, &commands).unwrap();
    let absent = root.join("absent");
    watcher.rules.directories.push(absent.clone());
    let mut event = Event::new(EventKind::Modify(ModifyKind::Data(
        notify::event::DataChange::Any,
    )));
    event.paths.push(absent.clone());

    watcher.refresh_changed_directories(&event).unwrap();

    assert!(watcher.rules.directories.contains(&absent));
}

#[test]
fn ignored_gitignore_changes_refresh_its_subtree() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let ignored = root.join("ignored");
    let gitignore = root.join(".gitignore");
    fs::write(&gitignore, ".gitignore\nignored/\n").unwrap();
    fs::create_dir(&ignored).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(&working_tree_plan(root), &state, &commands).unwrap();
    assert!(!watcher.rules.directories.contains(&ignored));

    fs::write(&gitignore, "").unwrap();
    let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
    event.paths.push(gitignore);
    watcher.update(&event, &state).unwrap();

    assert!(watcher.rules.directories.contains(&ignored));
}

#[test]
fn a_directory_gone_before_its_watch_starts_is_skipped_and_other_errors_are_kept() {
    let directory = tempdir().unwrap();
    let gone = directory.path().join("gone");
    let (commands, _events) = mpsc::channel::<WatchCommand>();
    let mut watcher = notify::recommended_watcher(move |_: notify::Result<Event>| {
        let _ = commands.send(WatchCommand::Failed);
    })
    .unwrap();

    let missing = watcher
        .watch(&gone, RecursiveMode::NonRecursive)
        .unwrap_err();

    assert!(is_missing(&missing), "{missing:?}");
    watch_present(&mut watcher, &gone, RecursiveMode::NonRecursive).unwrap();
    assert!(!is_missing(&notify::Error::generic("the watcher failed")));
}
