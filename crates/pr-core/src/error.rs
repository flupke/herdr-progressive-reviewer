//! Errors shared by the reviewer processes.

use std::ffi::OsString;
use std::path::PathBuf;

/// A result that uses the shared reviewer error.
pub type Result<T> = std::result::Result<T, Error>;

/// An error with operation context and no repository file content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
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
