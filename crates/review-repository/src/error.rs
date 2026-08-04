//! Repository and command errors.

use std::ffi::OsString;
use std::path::PathBuf;

/// A repository operation result.
pub type Result<T> = std::result::Result<T, Error>;

/// A repository or command error with no repository file content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Local repository state I/O failed.
    #[error("{operation} failed at {path:?}: {source}")]
    Io {
        /// The operation that failed.
        operation: &'static str,
        /// The repository state path.
        path: PathBuf,
        /// The operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A child process did not start.
    #[error("{operation} could not start {program:?} in {current_dir:?}: {source}")]
    Spawn {
        /// The operation that failed.
        operation: String,
        /// The executable that could not start.
        program: OsString,
        /// The requested working directory, if one was set.
        current_dir: Option<PathBuf>,
        /// The operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A command returned an unsuccessful exit status.
    #[error("{operation} failed with exit code {code:?}")]
    CommandFailed {
        /// The operation that failed.
        operation: String,
        /// The process exit code, if the platform supplied one.
        code: Option<i32>,
    },

    /// A child process exceeded its captured-output limit.
    #[error("{operation} produced more than 256 MiB of output in {path:?}")]
    CommandOutputTooLarge {
        /// The operation that produced too much output.
        operation: String,
        /// The repository where the command ran.
        path: PathBuf,
    },

    /// A child process was canceled during shutdown.
    #[error("{operation} was canceled in {path:?}")]
    CommandCancelled {
        /// The operation that was canceled.
        operation: String,
        /// The repository where the command ran.
        path: PathBuf,
    },

    /// A directory is not in a supported repository.
    #[error("{path} is not in a jj or Git repository")]
    NotRepository {
        /// The directory where repository discovery started.
        path: PathBuf,
    },

    /// An external protocol response was not valid.
    #[error("{operation} returned an invalid response: {detail}")]
    Protocol {
        /// The protocol operation that failed.
        operation: String,
        /// A content-free explanation of the mismatch.
        detail: &'static str,
    },
}
