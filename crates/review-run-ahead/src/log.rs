//! Run-ahead's log, which every host writes.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// Run-ahead's log, which says what the forks did, one line per event.
#[derive(Clone, Debug, Default)]
pub struct RunAheadLog {
    /// Where it writes; `None` writes nothing.
    path: Option<PathBuf>,
}

impl RunAheadLog {
    /// A log written to `path`; none for `None`.
    pub fn at(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Adds `line`, with the time and the reviewer's process.
    pub fn write(&self, line: &str) {
        let Some(path) = &self.path else {
            return;
        };
        let now = time::OffsetDateTime::now_utc();
        let line = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} UTC [pid {}] {line}\n",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            now.millisecond(),
            std::process::id(),
        );
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            // One write per line, so the lines of several threads do not interleave.
            let _ = file.write_all(line.as_bytes());
        }
    }
}
