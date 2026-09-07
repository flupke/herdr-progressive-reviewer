use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, bounded};

use crate::language::Project;

pub(super) struct ServerProcess {
    pub(super) child: Child,
    pub(super) uses_direnv: bool,
}

impl ServerProcess {
    pub(super) fn start(project: &Project) -> Result<Self, String> {
        ServerLauncher::new(project)
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", project.server.command()))
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ServerLauncher {
    direnv: Command,
    direct: Command,
}

impl ServerLauncher {
    fn new(project: &Project) -> Self {
        let mut direnv = Command::new("direnv");
        direnv
            .arg("exec")
            .arg(&project.root)
            .arg(project.server.command());
        let mut direct = Command::new(project.server.command());
        for command in [&mut direnv, &mut direct] {
            command
                .args(project.server.arguments())
                .current_dir(&project.root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
        Self { direnv, direct }
    }

    fn spawn(&mut self) -> io::Result<ServerProcess> {
        match self.direnv.spawn() {
            Ok(child) => Ok(ServerProcess {
                child,
                uses_direnv: true,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.direct.spawn().map(|child| ServerProcess {
                    child,
                    uses_direnv: false,
                })
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) struct StderrOutput {
    completed: Receiver<String>,
}

impl StderrOutput {
    pub(super) fn capture(stderr: ChildStderr) -> Self {
        let (sender, completed) = bounded(1);
        thread::spawn(move || {
            let _ = sender.send(Self::read_tail(stderr));
        });
        Self { completed }
    }

    fn read_tail(mut input: impl Read) -> String {
        let mut tail = DiagnosticLines::default();
        let mut buffer = [0; 1024];
        while let Ok(count) = input.read(&mut buffer) {
            if count == 0 {
                break;
            }
            for byte in &buffer[..count] {
                tail.push(*byte);
            }
        }
        tail.finish()
    }

    pub(super) fn failure(&self, server: &str) -> String {
        let stderr = self
            .completed
            .recv_timeout(Duration::from_millis(100))
            .unwrap_or_default();
        if stderr.is_empty() {
            format!("{server} stopped")
        } else {
            format!("{server} stopped: {}", Self::summary(&stderr))
        }
    }

    fn summary(stderr: &str) -> String {
        let diagnostic = stderr
            .lines()
            .find(|line| line.starts_with("direnv: error"))
            .map_or(stderr, |line| {
                line.split_once(" on PATH ")
                    .map_or(line, |(error, _)| error)
            });
        let single_line = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut chars = single_line.chars();
        let mut summary: String = chars.by_ref().take(300).collect();
        if chars.next().is_some() {
            summary.push('…');
        }
        summary
    }
}

#[derive(Default)]
struct DiagnosticLines {
    current: Vec<u8>,
    lines: VecDeque<String>,
    truncated: bool,
}

impl DiagnosticLines {
    fn push(&mut self, byte: u8) {
        if byte == b'\n' {
            self.end_line();
        } else if self.current.len() < 1024 {
            self.current.push(byte);
        } else {
            self.truncated = true;
        }
    }

    fn end_line(&mut self) {
        let mut line = String::from_utf8_lossy(&strip_ansi_escapes::strip(&self.current))
            .trim()
            .to_owned();
        if self.truncated {
            line.push('…');
        }
        if !line.is_empty() {
            if self.lines.len() == 4 {
                self.lines.pop_front();
            }
            self.lines.push_back(line);
        }
        self.current.clear();
        self.truncated = false;
    }

    fn finish(mut self) -> String {
        self.end_line();
        self.lines.into_iter().collect::<Vec<_>>().join("\n")
    }
}

#[cfg(all(test, unix))]
#[path = "process.tests.rs"]
mod tests;
