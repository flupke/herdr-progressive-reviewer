use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn notification_debounces_before_poll() {
    let now = Instant::now();
    let state = Arc::new(WatchState::default());
    let (commands, command_receiver) = mpsc::channel();
    let mut watcher = RepositoryWatcher {
        commands,
        state: Arc::clone(&state),
        next_poll: now + FALLBACK_POLL_INTERVAL,
        poll_interval: FALLBACK_POLL_INTERVAL,
    };

    state.notified.store(true, Ordering::Relaxed);
    state.watching.store(true, Ordering::Relaxed);

    assert!(!watcher.poll_due(now));
    assert!(command_receiver.try_recv().is_err());
    assert!(watcher.poll_due(now + DEBOUNCE));
    assert!(!watcher.poll_due(now + DEBOUNCE));
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
    let rules = IgnoreRules::discover(root, RepoType::Jj);

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
    let rules = IgnoreRules::discover(root, RepoType::Git);

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
    let rules = IgnoreRules::discover(&root, RepoType::Git);

    assert!(rules.directories.contains(&root.join("ignored")));
    assert!(rules.includes(&root.join("ignored"), true));
}

#[test]
fn failed_watcher_stays_in_polling_mode() {
    let now = Instant::now();
    let state = Arc::new(WatchState::default());
    let (commands, command_receiver) = mpsc::channel();
    let mut watcher = RepositoryWatcher {
        commands,
        state: Arc::clone(&state),
        next_poll: now + FALLBACK_POLL_INTERVAL,
        poll_interval: FALLBACK_POLL_INTERVAL,
    };
    state.failed.store(true, Ordering::Relaxed);

    assert!(watcher.poll_due(now));
    assert!(command_receiver.try_recv().is_err());
}

#[test]
fn watches_only_repository_metadata_that_signals_state_changes() {
    let directory = tempdir().unwrap();
    let root = directory.path().join("work");
    let git = root.join(".git");
    let jj = directory.path().join("jj-repo");
    fs::create_dir_all(git.join("refs/heads")).unwrap();
    fs::create_dir_all(git.join("objects/ab")).unwrap();
    fs::create_dir_all(root.join(".jj")).unwrap();
    fs::create_dir_all(jj.join("op_heads/heads")).unwrap();
    fs::write(root.join(".jj/repo"), "../../jj-repo").unwrap();
    let metadata = MetadataWatches::discover(&root);

    let event_at = |path| {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
        event.paths.push(path);
        event
    };
    assert!(metadata.includes(&event_at(jj.join("op_heads/heads/operation"))));
    assert!(!metadata.includes(&event_at(git.join("refs/heads/main"))));
    assert!(!metadata.includes(&event_at(git.join("objects/ab/object"))));
    assert!(!metadata.includes(&event_at(root.join("source.rs"))));

    fs::remove_file(root.join(".jj/repo")).unwrap();
    fs::create_dir(root.join(".jj/repo")).unwrap();
    let metadata = MetadataWatches::discover(&root);
    assert!(metadata.includes(&event_at(git.join("refs/heads/main"))));
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
    let rules = IgnoreRules::discover(&root, RepoType::Jj);

    assert!(rules.directories.contains(&root.join("parent-ignored")));
    assert!(rules.directories.contains(&root.join("git-ignored")));
    assert!(!rules.directories.contains(&root.join("local-ignored")));
    assert!(rules.external_files.is_empty());

    fs::remove_dir(root.join(".jj")).unwrap();
    let refreshed =
        IgnoreRules::discover_subtree(&root, &root, rules.repo_type, &rules.external_files);
    assert!(refreshed.directories.contains(&root.join("git-ignored")));
    assert!(refreshed.external_files.is_empty());
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
    let mut watcher = ActiveWatcher::start(root, RepoType::Git, &state, &commands).unwrap();
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
    let mut watcher = ActiveWatcher::start(root, RepoType::Git, &state, &commands).unwrap();
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
fn ignored_gitignore_changes_refresh_its_subtree() {
    let directory = tempdir().unwrap();
    let root = directory.path();
    let ignored = root.join("ignored");
    let gitignore = root.join(".gitignore");
    fs::write(&gitignore, ".gitignore\nignored/\n").unwrap();
    fs::create_dir(&ignored).unwrap();
    let state = Arc::new(WatchState::default());
    let (commands, _events) = mpsc::channel();
    let mut watcher = ActiveWatcher::start(root, RepoType::Jj, &state, &commands).unwrap();
    assert!(!watcher.rules.directories.contains(&ignored));

    fs::write(&gitignore, "").unwrap();
    let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
    event.paths.push(gitignore);
    watcher.update(&event, &state).unwrap();

    assert!(watcher.rules.directories.contains(&ignored));
}
