//! Errors shared by the reviewer processes.

use std::ffi::OsString;
use std::path::PathBuf;

/// A result that uses the shared reviewer error.
pub type Result<T> = std::result::Result<T, Error>;

/// An error with operation context and no repository file content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A required run-time value was not supplied.
    #[error("required environment variable {name} is not set")]
    Environment {
        /// The missing variable.
        name: &'static str,
    },

    /// Local protocol or state I/O failed.
    #[error("{operation} failed at {path:?}: {source}")]
    Io {
        /// The operation that failed.
        operation: &'static str,
        /// The socket or state path.
        path: PathBuf,
        /// The operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A JSON protocol value was invalid.
    #[error("{operation} returned invalid JSON: {source}")]
    Json {
        /// The protocol operation that failed.
        operation: &'static str,
        /// The JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Herdr rejected a socket request.
    #[error("{operation} failed: {message}")]
    Herdr {
        /// The socket method.
        operation: &'static str,
        /// Herdr's content-free error text.
        message: String,
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

    /// A directory is not in a jj workspace.
    #[error("{path} is not in a jj workspace")]
    NotJjRepository {
        /// The directory where repository discovery started.
        path: PathBuf,
    },

    /// A tool did not return its name followed by a semantic version.
    #[error("{tool} returned an invalid version")]
    InvalidVersion {
        /// The tool whose output was invalid.
        tool: &'static str,
    },

    /// An installed tool is older than the supported minimum.
    #[error("{tool} {found} is not supported; version {minimum} or later is required")]
    UnsupportedVersion {
        /// The tool that is too old.
        tool: &'static str,
        /// The minimum supported version.
        minimum: semver::Version,
        /// The installed version.
        found: semver::Version,
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
