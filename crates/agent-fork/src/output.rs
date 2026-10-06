//! What a fork prints, and how it ended.

use std::fmt;

/// Receives a fork's standard output, line by line, then how the fork ended.
pub trait ForkOutput: Send + 'static {
    /// One line of the fork's standard output, without its line end.
    fn line(&mut self, line: &str);

    /// One line of the fork's standard error, without its line end, for a fork that reports
    /// there what it does. Ignored unless the output reads it; the [`Exit`] keeps the end of
    /// the standard error either way.
    fn error_line(&mut self, _line: &str) {}

    /// The fork ended, as `exit` says. Called once, after the lines that could still be read.
    fn ended(self: Box<Self>, exit: Exit);
}

/// How a fork's process ended.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Exit {
    /// The exit status as the system words it, such as `exit status: 0` or `signal: 15`.
    pub status: String,
    /// The end of its standard error, at most a few kilobytes.
    pub stderr: String,
}

impl fmt::Display for Exit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stderr = self.stderr.trim();
        if stderr.is_empty() {
            write!(formatter, "{}", self.status)
        } else {
            write!(formatter, "{} ({stderr})", self.status)
        }
    }
}
