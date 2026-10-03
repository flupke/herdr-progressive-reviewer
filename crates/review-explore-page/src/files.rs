//! The page's templates and assets: built into the binary, or read from disk in development.

use std::path::{Path, PathBuf};

use markdown_html::HtmlRenderer;
use minijinja::{Environment, Value};
use serde::Serialize;
use tokio::sync::watch;

/// Templates by name. minijinja escapes HTML in templates whose name ends with `.html`.
const TEMPLATES: &[(&str, &str)] = &[
    ("page.html", include_str!("../templates/page.html")),
    (
        "explanation.html",
        include_str!("../templates/explanation.html"),
    ),
    (
        "citations.html",
        include_str!("../templates/citations.html"),
    ),
];

/// Assets by name, with their content type.
const ASSETS: &[Asset] = &[
    Asset {
        name: "page.js",
        content_type: "text/javascript",
        body: include_str!("../assets/page.js"),
        development: false,
    },
    Asset {
        name: "wake.js",
        content_type: "text/javascript",
        body: include_str!("../assets/wake.js"),
        development: false,
    },
    Asset {
        name: "style.css",
        content_type: "text/css",
        body: include_str!("../assets/style.css"),
        development: false,
    },
    Asset {
        name: "markdown.css",
        content_type: "text/css",
        body: include_str!("../assets/markdown.css"),
        development: false,
    },
    Asset {
        name: "citations.css",
        content_type: "text/css",
        body: include_str!("../assets/citations.css"),
        development: false,
    },
    Asset {
        name: "dev.js",
        content_type: "text/javascript",
        body: include_str!("../assets/dev.js"),
        development: true,
    },
];

struct Asset {
    name: &'static str,
    content_type: &'static str,
    body: &'static str,
    /// Served only while the templates are read from disk.
    development: bool,
}

/// Where the page's markup, scripts and styles come from.
pub struct PageFiles(Source);

enum Source {
    Embedded(Environment<'static>),
    /// Development: the files are read again on every request.
    Disk {
        /// The directory that holds `templates/` and `assets/`.
        root: PathBuf,
        /// Counts the changes under `root`.
        changes: watch::Receiver<u64>,
        _watcher: notify::RecommendedWatcher,
    },
}

impl PageFiles {
    /// The files built into the binary.
    ///
    /// # Panics
    ///
    /// When a built-in template does not parse; the e2e tests catch it.
    pub fn embedded() -> Self {
        let mut environment = environment();
        for (name, source) in TEMPLATES {
            environment
                .add_template(name, source)
                .unwrap_or_else(|error| panic!("the built-in template {name} is invalid: {error}"));
        }
        Self(Source::Embedded(environment))
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

    pub(crate) fn render(
        &self,
        name: &str,
        context: impl Serialize,
    ) -> Result<String, minijinja::Error> {
        match &self.0 {
            Source::Embedded(environment) => environment.get_template(name)?.render(context),
            Source::Disk { root, .. } => {
                let mut environment = environment();
                environment.set_loader(minijinja::path_loader(root.join("templates")));
                environment.get_template(name)?.render(context)
            }
        }
    }

    /// The content type and body of the asset `name`.
    pub(crate) fn asset(&self, name: &str) -> Option<(&'static str, String)> {
        let asset = ASSETS.iter().find(|asset| asset.name == name)?;
        match &self.0 {
            Source::Embedded(_) if asset.development => None,
            Source::Embedded(_) => Some((asset.content_type, asset.body.to_owned())),
            Source::Disk { root, .. } => {
                let body = std::fs::read_to_string(root.join("assets").join(asset.name)).ok()?;
                Some((asset.content_type, body))
            }
        }
    }

    /// The count of file changes so far, in development only.
    pub(crate) fn dev_version(&self) -> Option<u64> {
        self.changes().map(|changes| *changes.borrow())
    }

    /// The count of file changes, in development only.
    pub(crate) fn changes(&self) -> Option<watch::Receiver<u64>> {
        match &self.0 {
            Source::Embedded(_) => None,
            Source::Disk { changes, .. } => Some(changes.clone()),
        }
    }
}

/// The templates' environment, with the filter `markdown`: `{{ text | markdown(2) }}` renders
/// the agent's Markdown `text` as HTML that sits under an `<h2>`, and puts it in the page as it
/// is.
fn environment() -> Environment<'static> {
    let mut environment = Environment::new();
    environment.add_filter("markdown", |text: &str, under_heading: usize| {
        Value::from_safe_string(
            HtmlRenderer::under_heading(under_heading)
                .render(text)
                .into_string(),
        )
    });
    environment
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
    for directory in ["templates", "assets"] {
        watcher.watch(&root.join(directory), notify::RecursiveMode::Recursive)?;
    }
    Ok(watcher)
}
