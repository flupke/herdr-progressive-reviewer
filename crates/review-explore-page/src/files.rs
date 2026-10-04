//! The page's shell, client modules, styles and scripts: built into the binary, or read from
//! disk in development.

use std::path::{Path, PathBuf};

use tokio::sync::watch;

/// An asset, by its path under `assets/`, with its content type.
macro_rules! asset {
    ($name:literal, $content_type:literal) => {
        Asset {
            name: $name,
            content_type: $content_type,
            body: include_str!(concat!("../assets/", $name)),
        }
    };
}

/// The page that loads the client; `{dev}` takes the count of file changes in development.
const SHELL: &str = include_str!("../assets/page.html");

/// Assets by name, with their content type. A module of the client must be listed here, with
/// the strict content type of a script, or the browser refuses to load it.
const ASSETS: &[Asset] = &[
    asset!("tokens.css", "text/css"),
    asset!("buttons.css", "text/css"),
    asset!("status.css", "text/css"),
    asset!("masthead.css", "text/css"),
    asset!("meter.css", "text/css"),
    asset!("style.css", "text/css"),
    asset!("conclusion.css", "text/css"),
    asset!("markdown.css", "text/css"),
    asset!("tags.css", "text/css"),
    asset!("turn.css", "text/css"),
    asset!("sent.css", "text/css"),
    asset!("answer-card.css", "text/css"),
    asset!("choices.css", "text/css"),
    asset!("question.css", "text/css"),
    asset!("quiz.css", "text/css"),
    asset!("citations.css", "text/css"),
    asset!("diagrams.css", "text/css"),
    asset!("design.css", "text/css"),
    asset!("earlier.css", "text/css"),
    asset!("swipe.css", "text/css"),
    asset!("chat.css", "text/css"),
    asset!("layout.css", "text/css"),
    asset!("client/main.js", "text/javascript"),
    asset!("client/actions.js", "text/javascript"),
    asset!("client/change-size.js", "text/javascript"),
    asset!("client/chat.js", "text/javascript"),
    asset!("client/chat-quote.js", "text/javascript"),
    asset!("client/chips.js", "text/javascript"),
    asset!("client/choices.js", "text/javascript"),
    asset!("client/citations.js", "text/javascript"),
    asset!("client/conclusion.js", "text/javascript"),
    asset!("client/disclosure.js", "text/javascript"),
    asset!("client/connection.js", "text/javascript"),
    asset!("client/design.js", "text/javascript"),
    asset!("client/desk.js", "text/javascript"),
    asset!("client/dev.js", "text/javascript"),
    asset!("client/diagrams.js", "text/javascript"),
    asset!("client/dom.js", "text/javascript"),
    asset!("client/drafts.js", "text/javascript"),
    asset!("client/earlier.js", "text/javascript"),
    asset!("client/masthead.js", "text/javascript"),
    asset!("client/meter.js", "text/javascript"),
    asset!("client/page.js", "text/javascript"),
    asset!("client/question.js", "text/javascript"),
    asset!("client/quiz.js", "text/javascript"),
    asset!("client/route.js", "text/javascript"),
    asset!("client/socket.js", "text/javascript"),
    asset!("client/start.js", "text/javascript"),
    asset!("client/status.js", "text/javascript"),
    asset!("client/swipe.js", "text/javascript"),
    asset!("client/turn.js", "text/javascript"),
    asset!("client/sent.js", "text/javascript"),
    asset!("client/answer-card.js", "text/javascript"),
];

struct Asset {
    name: &'static str,
    content_type: &'static str,
    body: &'static str,
}

/// Where the page's files come from.
pub struct PageFiles(Source);

enum Source {
    Embedded,
    /// Development: the files are read again on every request.
    Disk {
        /// The directory that holds `assets/`.
        root: PathBuf,
        /// Counts the changes under `root`.
        changes: watch::Receiver<u64>,
        _watcher: notify::RecommendedWatcher,
    },
}

impl PageFiles {
    /// The files built into the binary.
    pub fn embedded() -> Self {
        Self(Source::Embedded)
    }

    /// Development: reads the files under `root`, this crate's directory, on every request,
    /// so an edit shows on the next load. An open page loads itself again when a file changes:
    /// the change comes from the file system's events (inotify on Linux), not from polling.
    pub fn from_dir(root: PathBuf) -> notify::Result<Self> {
        let (counter, changes) = watch::channel(0);
        let watcher = watch_files(&root, counter)?;
        Ok(Self(Source::Disk {
            root,
            changes,
            _watcher: watcher,
        }))
    }

    /// The page that loads the client, which loads itself again on a file change in
    /// development.
    pub(crate) fn shell(&self) -> String {
        match &self.0 {
            Source::Embedded => SHELL.replace("{dev}", ""),
            Source::Disk { root, changes, .. } => {
                let shell = std::fs::read_to_string(root.join("assets/page.html"))
                    .unwrap_or_else(|_| SHELL.to_owned());
                shell.replace("{dev}", &changes.borrow().to_string())
            }
        }
    }

    /// The content type and body of the asset `name`, a path under `assets/`.
    pub(crate) fn asset(&self, name: &str) -> Option<(&'static str, String)> {
        let asset = ASSETS.iter().find(|asset| asset.name == name)?;
        match &self.0 {
            Source::Embedded => Some((asset.content_type, asset.body.to_owned())),
            Source::Disk { root, .. } => {
                let body = std::fs::read_to_string(root.join("assets").join(asset.name)).ok()?;
                Some((asset.content_type, body))
            }
        }
    }

    /// The count of file changes, in development only.
    pub(crate) fn changes(&self) -> Option<watch::Receiver<u64>> {
        match &self.0 {
            Source::Embedded => None,
            Source::Disk { changes, .. } => Some(changes.clone()),
        }
    }
}

fn watch_files(
    root: &Path,
    counter: watch::Sender<u64>,
) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| event.kind.is_modify() || event.kind.is_create()) {
            counter.send_modify(|count| *count += 1);
        }
    })?;
    watcher.watch(&root.join("assets"), notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}
