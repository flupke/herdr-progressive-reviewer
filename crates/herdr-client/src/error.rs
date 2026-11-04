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

    /// Herdr wrote a prompt into an agent's pane, but the agent showed no sign of starting
    /// on it: the text may still wait in the agent's prompt box.
    #[error("the agent did not start on the prompt: {message}")]
    AgentNotStarted { message: String },

    /// An event stream was cancelled before or while it subscribed.
    #[error("the host's event stream was cancelled")]
    Cancelled,

    /// The host cannot do what the reviewer asks of it, such as herdr-core keeping an agent's
    /// session.
    #[error("{operation} is not supported here: {reason}")]
    Unsupported {
        operation: &'static str,
        reason: &'static str,
    },

    /// Herdr, or herdr-core, rejected a socket request.
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
