use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use toasts::ToastId;

/// One LSP operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::FromRepr)]
#[repr(usize)]
pub enum Operation {
    /// Show documentation at the position.
    Hover,
    /// Find definitions at the position.
    Definition,
    /// Find references at the position.
    References,
}

impl Operation {
    /// Return the title for this operation's results.
    pub fn title(self) -> &'static str {
        match self {
            Self::Hover => "Documentation",
            Self::Definition => "Definitions",
            Self::References => "References",
        }
    }

    /// Return the progress message for this operation.
    pub fn progress_text(self) -> &'static str {
        match self {
            Self::Hover => "Loading documentation…",
            Self::Definition => "Finding definition…",
            Self::References => "Finding references…",
        }
    }

    /// Remove locations that this operation must not show.
    pub fn filter_locations(
        self,
        root: &Path,
        locations: Vec<SourceLocation>,
    ) -> Vec<SourceLocation> {
        locations
            .into_iter()
            .filter(|location| self != Self::References || location.path.starts_with(root))
            .collect()
    }
}

/// One source position from the visible review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    /// Long-toast ID for this request.
    pub toast_id: ToastId,
    /// Absolute disk path.
    pub path: PathBuf,
    /// Zero-based source line.
    pub line: u32,
    /// Zero-based byte offset into the UTF-8-encoded source line.
    pub byte_column: usize,
    /// Source line shown when the request was created.
    pub expected_line: String,
    /// Review snapshot ID used to reject late results.
    pub snapshot_id: String,
}

/// One normalized file location returned by the server.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceLocation {
    /// Absolute disk path.
    pub path: PathBuf,
    /// Zero-based start line.
    pub line: u32,
    /// Zero-based byte offset into the UTF-8-encoded source line.
    pub byte_column: usize,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based byte offset for the range end in the UTF-8-encoded source line.
    pub end_byte_column: usize,
}

impl SourceLocation {
    /// Return the repository-relative path, when this location is inside it.
    pub fn review_path(&self, root: &Path) -> Option<String> {
        let path = if self.path.is_absolute() {
            self.path.strip_prefix(root).ok()?
        } else {
            &self.path
        };
        Some(path.display().to_string())
    }

    /// Return the path to show for this repository.
    pub fn display_path(&self, root: &Path) -> String {
        self.review_path(root)
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Return the selected byte range within one source line.
    pub fn range_in_line(&self, line: u32, length: usize) -> Option<Range<usize>> {
        if line < self.line || line > self.end_line {
            return None;
        }
        Some(if self.line == self.end_line {
            self.byte_column.min(length)..self.end_byte_column.min(length)
        } else if line == self.line {
            self.byte_column.min(length)..length
        } else if line == self.end_line {
            0..self.end_byte_column.min(length)
        } else {
            0..length
        })
    }
}

/// Work sent to the LSP thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Command {
    /// Start rust-analyzer if it is not running.
    Initialize,
    /// Tell rust-analyzer about one open Rust document.
    OpenDocument(PathBuf),
    /// Run one LSP request.
    Request { operation: Operation, query: Query },
    /// Stop the server and worker.
    Shutdown,
}

/// Results sent back to the runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// rust-analyzer startup began.
    Initializing,
    /// rust-analyzer is ready.
    Ready,
    /// Hover content arrived.
    Hover {
        /// ID of the completed toast.
        toast_id: ToastId,
        /// Review snapshot ID of the request.
        snapshot_id: String,
        /// Markdown content, when the position has documentation.
        markdown: Option<String>,
    },
    /// Definition or reference locations arrived.
    Locations {
        /// ID of the completed toast.
        toast_id: ToastId,
        /// Operation that produced the locations.
        operation: Operation,
        /// Review snapshot ID of the request.
        snapshot_id: String,
        /// Normalized file locations.
        locations: Vec<SourceLocation>,
    },
    /// Startup or a request failed.
    Failed {
        /// ID of the failed toast, or `None` for background startup.
        toast_id: Option<ToastId>,
        /// Review snapshot ID, or `None` for background startup.
        snapshot_id: Option<String>,
        /// User-facing error message.
        message: String,
    },
}
