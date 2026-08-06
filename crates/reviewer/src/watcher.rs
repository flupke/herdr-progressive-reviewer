//! Filesystem-triggered repository refreshes.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder, gitconfig_excludes_path};
use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use review_repository::repository::RepoType;

const DEBOUNCE: Duration = Duration::from_millis(100);
const FALLBACK_POLL_INTERVAL: Duration = Duration::from_secs(30);
const FAILED_WATCHER_POLL_INTERVAL: Duration = Duration::from_secs(2);

enum WatchCommand {
    Event(Event),
    Failed,
    Stop,
}

struct IgnoreRules {
    root: PathBuf,
    repo_type: RepoType,
    directories: Vec<PathBuf>,
    gitignores: Vec<(PathBuf, Gitignore)>,
    external_files: Vec<PathBuf>,
}

impl IgnoreRules {
    fn discover(root: &Path, repo_type: RepoType) -> Self {
        let external_files = Self::external_files(root, repo_type);
        Self::discover_subtree(root, root, repo_type, &external_files)
    }

    fn discover_subtree(
        root: &Path,
        subtree: &Path,
        repo_type: RepoType,
        external_files: &[PathBuf],
    ) -> Self {
        let walk = Self::walk_builder(root, subtree, repo_type);
        let mut directories = Vec::new();
        let mut gitignores = Vec::new();
        for path in external_files {
            Self::add_gitignore_file(&mut gitignores, root, path);
        }
        for entry in walk.build().filter_map(Result::ok) {
            if !entry.path().starts_with(subtree) {
                continue;
            }
            if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                directories.push(entry.path().to_owned());
            } else if entry.file_name() == ".gitignore"
                && let Some(parent) = entry.path().parent()
            {
                Self::add_gitignore(&mut gitignores, parent);
            }
        }
        gitignores.sort_by_key(|(path, _)| path.components().count());
        Self {
            root: root.to_owned(),
            repo_type,
            directories,
            gitignores,
            external_files: external_files.to_owned(),
        }
    }

    fn external_files(root: &Path, repo_type: RepoType) -> Vec<PathBuf> {
        match repo_type {
            RepoType::Jj => Vec::new(),
            RepoType::Git => {
                let mut paths = gitconfig_excludes_path().into_iter().collect::<Vec<_>>();
                if let Some(git) = MetadataWatches::git_directory(root) {
                    let common =
                        MetadataWatches::resolve_directory_or_link_file(&git.join("commondir"))
                            .unwrap_or(git);
                    paths.push(common.join("info/exclude"));
                }
                paths
            }
        }
    }

    fn walk_builder(root: &Path, subtree: &Path, repo_type: RepoType) -> WalkBuilder {
        let walk_subtree = subtree.to_owned();
        let mut walk = WalkBuilder::new(root);
        walk.hidden(false)
            .ignore(false)
            .parents(false)
            .require_git(false);
        match repo_type {
            RepoType::Git => walk.git_global(true).git_exclude(true),
            RepoType::Jj => walk.git_global(false).git_exclude(false),
        };
        walk.filter_entry(move |entry| {
            !is_repository_metadata(entry.path())
                && (entry.path().starts_with(&walk_subtree)
                    || walk_subtree.starts_with(entry.path()))
        });
        walk
    }

    fn add_gitignore(gitignores: &mut Vec<(PathBuf, Gitignore)>, directory: &Path) {
        let path = directory.join(".gitignore");
        Self::add_gitignore_file(gitignores, directory, &path);
    }

    fn add_gitignore_file(
        gitignores: &mut Vec<(PathBuf, Gitignore)>,
        directory: &Path,
        path: &Path,
    ) {
        if !path.is_file() {
            return;
        }
        let mut builder = GitignoreBuilder::new(directory);
        builder.add(path);
        if let Ok(rules) = builder.build() {
            gitignores.push((directory.to_owned(), rules));
        }
    }

    fn includes(&self, path: &Path, is_dir: bool) -> bool {
        if !path.starts_with(&self.root) || is_repository_metadata(path) {
            return false;
        }
        self.gitignores
            .iter()
            .rev()
            .filter(|(root, _)| path.starts_with(root))
            .find_map(|(_, rules)| {
                let matched = rules.matched_path_or_any_parents(path, is_dir);
                (!matched.is_none()).then(|| !matched.is_ignore())
            })
            .unwrap_or(true)
    }

    fn includes_event(&self, event: &Event) -> bool {
        event.paths.iter().any(|path| {
            let is_dir = path.is_dir()
                || self.directories.contains(path)
                || matches!(
                    event.kind,
                    EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
                );
            self.includes(path, is_dir)
        })
    }

    fn external_watch_directories(&self) -> Vec<PathBuf> {
        let mut directories = Vec::new();
        for directory in self
            .external_files
            .iter()
            .filter_map(|path| path.ancestors().skip(1).find(|parent| parent.is_dir()))
        {
            if !directories
                .iter()
                .any(|watched: &PathBuf| watched == directory)
            {
                directories.push(directory.to_owned());
            }
        }
        directories
    }
}

#[derive(Default)]
struct WatchState {
    notified: AtomicBool,
    failed: AtomicBool,
    watching: AtomicBool,
}

struct ActiveWatcher {
    watcher: RecommendedWatcher,
    rules: IgnoreRules,
    metadata: MetadataWatches,
    external_directories: Vec<PathBuf>,
}

struct MetadataWatches {
    watches: Vec<MetadataWatch>,
}

struct MetadataWatch {
    directory: PathBuf,
    mode: RecursiveMode,
}

pub(super) struct RepositoryWatcher {
    commands: Sender<WatchCommand>,
    state: Arc<WatchState>,
    next_poll: Instant,
    poll_interval: Duration,
}

impl RepositoryWatcher {
    pub(super) fn new(root: &Path, repo_type: RepoType) -> Self {
        let state = Arc::new(WatchState::default());
        let (commands, command_receiver) = mpsc::channel();
        let root = root.to_owned();
        let thread_state = Arc::clone(&state);
        let event_commands = commands.clone();
        thread::spawn(move || {
            let mut watcher =
                ActiveWatcher::start(&root, repo_type, &thread_state, &event_commands);
            while let Ok(command) = command_receiver.recv() {
                match command {
                    WatchCommand::Event(event) => {
                        if let Some(active) = watcher.as_mut()
                            && active.update(&event, &thread_state).is_err()
                        {
                            watcher = None;
                            thread_state.watching.store(false, Ordering::Relaxed);
                            thread_state.failed.store(true, Ordering::Relaxed);
                        }
                    }
                    WatchCommand::Failed => {
                        watcher = None;
                        thread_state.watching.store(false, Ordering::Relaxed);
                        thread_state.failed.store(true, Ordering::Relaxed);
                    }
                    WatchCommand::Stop => break,
                }
            }
        });

        Self {
            commands,
            state,
            next_poll: Instant::now() + FAILED_WATCHER_POLL_INTERVAL,
            poll_interval: FAILED_WATCHER_POLL_INTERVAL,
        }
    }

    pub(super) fn poll_due(&mut self, now: Instant) -> bool {
        let notified = self.state.notified.swap(false, Ordering::Relaxed);
        let failed = self.state.failed.swap(false, Ordering::Relaxed);
        let watching = self.state.watching.load(Ordering::Relaxed);

        if failed {
            // ponytail: Fall back to polling; restart only if transient failures matter.
            self.poll_interval = FAILED_WATCHER_POLL_INTERVAL;
            self.next_poll = now;
        } else if notified {
            self.next_poll = now + DEBOUNCE;
        } else if watching && self.poll_interval == FAILED_WATCHER_POLL_INTERVAL {
            self.poll_interval = FALLBACK_POLL_INTERVAL;
            self.next_poll = now + FALLBACK_POLL_INTERVAL;
        }

        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + self.poll_interval;
        true
    }
}

impl Drop for RepositoryWatcher {
    fn drop(&mut self) {
        let _ = self.commands.send(WatchCommand::Stop);
    }
}

impl ActiveWatcher {
    fn start(
        root: &Path,
        repo_type: RepoType,
        state: &Arc<WatchState>,
        commands: &Sender<WatchCommand>,
    ) -> Option<Self> {
        let rules = IgnoreRules::discover(root, repo_type);
        let metadata = MetadataWatches::discover(root);
        let external_directories = rules.external_watch_directories();
        let callback_commands = commands.clone();
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) if should_process(&event) => {
                    let _ = callback_commands.send(WatchCommand::Event(event));
                }
                Err(_) => {
                    let _ = callback_commands.send(WatchCommand::Failed);
                }
                Ok(_) => {}
            })
            .and_then(|mut watcher| {
                for directory in &rules.directories {
                    watcher.watch(directory, RecursiveMode::NonRecursive)?;
                }
                for directory in &external_directories {
                    watcher.watch(directory, RecursiveMode::NonRecursive)?;
                }
                for target in &metadata.watches {
                    watcher.watch(&target.directory, target.mode)?;
                }
                Ok(watcher)
            });
        if let Ok(watcher) = watcher {
            state.watching.store(true, Ordering::Relaxed);
            Some(Self {
                watcher,
                rules,
                metadata,
                external_directories,
            })
        } else {
            state.watching.store(false, Ordering::Relaxed);
            state.failed.store(true, Ordering::Relaxed);
            None
        }
    }

    fn update(&mut self, event: &Event, state: &WatchState) -> notify::Result<()> {
        if event.need_rescan() {
            return Err(notify::Error::generic("filesystem events were lost"));
        }
        let changes_external_rules = event.paths.iter().any(|path| {
            self.rules
                .external_files
                .iter()
                .any(|external| external == path || external.starts_with(path))
        });
        if self.metadata.includes(event) && !changes_external_rules {
            state.notified.store(true, Ordering::Relaxed);
            return Ok(());
        }
        let changes_ignore_rules = event
            .paths
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == ".gitignore"));
        if !changes_external_rules && !changes_ignore_rules && !self.rules.includes_event(event) {
            return Ok(());
        }
        state.notified.store(true, Ordering::Relaxed);

        if changes_external_rules {
            let root = self.rules.root.clone();
            self.refresh_subtree(&root)?;
            for directory in self.rules.external_watch_directories() {
                if !self.external_directories.contains(&directory) {
                    self.watcher
                        .watch(&directory, RecursiveMode::NonRecursive)?;
                    self.external_directories.push(directory);
                }
            }
            return Ok(());
        }

        for path in &event.paths {
            if path.file_name().is_some_and(|name| name == ".gitignore")
                && let Some(parent) = path.parent()
            {
                self.refresh_subtree(parent)?;
            }
        }

        if matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        ) {
            for path in &event.paths {
                if path.is_dir()
                    || self
                        .rules
                        .directories
                        .iter()
                        .any(|directory| directory.starts_with(path))
                {
                    self.refresh_subtree(path)?;
                }
            }
        }
        Ok(())
    }

    fn refresh_subtree(&mut self, subtree: &Path) -> notify::Result<()> {
        let (removed, retained) = std::mem::take(&mut self.rules.directories)
            .into_iter()
            .partition(|directory| directory.starts_with(subtree));
        self.rules.directories = retained;
        for directory in removed {
            let _ = self.watcher.unwatch(&directory);
        }
        self.rules
            .gitignores
            .retain(|(path, _)| !path.starts_with(subtree));

        if !subtree.is_dir() || !self.rules.includes(subtree, true) {
            return Ok(());
        }
        let discovered = IgnoreRules::discover_subtree(
            &self.rules.root,
            subtree,
            self.rules.repo_type,
            &self.rules.external_files,
        );
        for directory in &discovered.directories {
            self.watcher.watch(directory, RecursiveMode::NonRecursive)?;
        }
        self.rules.directories.extend(discovered.directories);
        self.rules.gitignores.extend(
            discovered
                .gitignores
                .into_iter()
                .filter(|(path, _)| path.starts_with(subtree)),
        );
        if subtree == self.rules.root {
            self.rules.external_files = discovered.external_files;
        }
        self.rules
            .gitignores
            .sort_by_key(|(path, _)| path.components().count());
        Ok(())
    }
}

impl MetadataWatches {
    fn discover(root: &Path) -> Self {
        let mut metadata = Self {
            watches: Vec::new(),
        };

        if let Some(op_heads) = Self::jj_operation_heads(root) {
            metadata.add(op_heads, RecursiveMode::Recursive);
        } else if let Some(git) = Self::git_directory(root) {
            let common = Self::resolve_directory_or_link_file(&git.join("commondir"))
                .unwrap_or_else(|| git.clone());
            metadata.add(git, RecursiveMode::NonRecursive);
            metadata.add(common.clone(), RecursiveMode::NonRecursive);
            metadata.add(common.join("refs"), RecursiveMode::Recursive);
        }

        metadata
    }

    fn git_directory(root: &Path) -> Option<PathBuf> {
        let path = root.join(".git");
        if path.is_dir() {
            return fs::canonicalize(&path).ok();
        }
        let contents = fs::read_to_string(&path).ok()?;
        let value = contents.trim().strip_prefix("gitdir:")?.trim();
        let directory = path.parent()?.join(value);
        fs::canonicalize(directory).ok()
    }

    fn jj_operation_heads(root: &Path) -> Option<PathBuf> {
        let repository = Self::resolve_directory_or_link_file(&root.join(".jj/repo"))?;
        fs::canonicalize(repository.join("op_heads")).ok()
    }

    fn resolve_directory_or_link_file(path: &Path) -> Option<PathBuf> {
        if path.is_dir() {
            return fs::canonicalize(path).ok();
        }
        let value = fs::read_to_string(path).ok()?;
        fs::canonicalize(path.parent()?.join(value.trim())).ok()
    }

    fn add(&mut self, directory: PathBuf, mode: RecursiveMode) {
        let Ok(directory) = fs::canonicalize(directory) else {
            return;
        };
        if !self
            .watches
            .iter()
            .any(|target| target.directory == directory)
        {
            self.watches.push(MetadataWatch { directory, mode });
        }
    }

    fn includes(&self, event: &Event) -> bool {
        event.paths.iter().any(|path| {
            self.watches.iter().any(|target| match target.mode {
                RecursiveMode::Recursive => path.starts_with(&target.directory),
                RecursiveMode::NonRecursive => path.parent() == Some(target.directory.as_path()),
            })
        })
    }
}

fn should_process(event: &Event) -> bool {
    event.need_rescan()
        || matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        )
}

fn is_repository_metadata(path: &Path) -> bool {
    path.components().any(
        |component| matches!(component, Component::Normal(name) if name == ".git" || name == ".jj"),
    )
}

#[cfg(test)]
#[path = "watcher.tests.rs"]
mod tests;
