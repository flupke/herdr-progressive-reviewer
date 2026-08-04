use std::path::PathBuf;

/// A result from the local Herdr client.
pub type Result<T> = std::result::Result<T, Error>;

/// A Herdr client or protocol error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A required run-time value was not supplied.
    #[error("required environment variable {name} is not set")]
    Environment { name: &'static str },

    /// Local protocol or state I/O failed.
    #[error("{operation} failed at {path:?}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A JSON protocol value was invalid.
    #[error("{operation} returned invalid JSON: {source}")]
    Json {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// Herdr rejected a socket request.
    #[error("{operation} failed: {message}")]
    Herdr {
        operation: &'static str,
        message: String,
    },

    /// An external protocol response was not valid.
    #[error("{operation} returned an invalid response: {detail}")]
    Protocol {
        operation: String,
        detail: &'static str,
    },
}
