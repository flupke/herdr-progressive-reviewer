use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Result, ensure};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};

pub(super) struct Input {
    pub(super) events: Receiver<Option<io::Result<String>>>,
    signals: Handle,
    /// The command pipe, removed on exit so a later writer fails instead of
    /// blocking on a pipe nobody reads.
    pipe: Option<PathBuf>,
}

impl Input {
    pub(super) fn start(commands: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = &commands {
            Self::create_pipe(path)?;
        }
        let pipe = commands.clone();
        let (send, events) = mpsc::channel();
        let mut signals = Signals::new([SIGHUP, SIGINT, SIGTERM])?;
        let handle = signals.handle();
        let stop = send.clone();
        thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = stop.send(None);
            }
        });
        thread::spawn(move || {
            let Some(path) = commands else {
                for line in io::stdin().lock().lines() {
                    if send.send(Some(line)).is_err() {
                        return;
                    }
                }
                let _ = send.send(None);
                return;
            };
            // Opening blocks until the next writer, so each `echo` delivers
            // its commands and the session keeps running between them.
            loop {
                let Ok(pipe) = File::open(&path) else {
                    let _ = send.send(None);
                    return;
                };
                for line in BufReader::new(pipe).lines() {
                    if send.send(Some(line)).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Self {
            events,
            signals: handle,
            pipe,
        })
    }

    fn create_pipe(path: &Path) -> Result<()> {
        if path.exists() {
            ensure!(
                path.metadata()?.file_type().is_fifo(),
                "{} exists and is not a named pipe",
                path.display()
            );
            return Ok(());
        }
        let status = Command::new("mkfifo").arg(path).status()?;
        ensure!(status.success(), "mkfifo {} failed", path.display());
        Ok(())
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        self.signals.close();
        if let Some(pipe) = &self.pipe {
            let _ = std::fs::remove_file(pipe);
        }
        // Both input readers belong to this driver process. Do not block its
        // exit if a sandbox denies the signal iterator's socket wakeup.
    }
}
