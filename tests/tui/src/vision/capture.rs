use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, ensure};
use notify::{RecursiveMode, Watcher};
use tui_test::terminal::emu::Emulator;
use tui_test::{Backend, OpenOptions};

use super::frame::{Frame, Frames};

// Standard synchronized-update terminator emitted by Crossterm after a paint.
const FRAME_END: &[u8] = b"\x1b[?2026l";

pub(super) struct Capture {
    stop: mpsc::Sender<Notice>,
    worker: Option<JoinHandle<()>>,
}

enum Notice {
    Changed,
    Failed(notify::Error),
    Stop,
}

impl Capture {
    pub(super) fn start(path: &Path, frames: Arc<Frames>) -> Result<Self> {
        let (send, receive) = mpsc::channel();
        let notices = send.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let notice = match event {
                    Ok(event) if event.kind.is_modify() || event.kind.is_create() => {
                        Notice::Changed
                    }
                    Ok(_) => return,
                    Err(error) => Notice::Failed(error),
                };
                let _ = notices.send(notice);
            })?;
        // Register before reading to cover output produced during startup.
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        let mut recording = Recording::new(File::open(path)?);
        let worker = thread::spawn(move || {
            let _watcher = watcher;
            let result = (|| -> Result<()> {
                loop {
                    recording.drain(&frames)?;
                    match receive.recv()? {
                        Notice::Changed => {}
                        Notice::Failed(error) => return Err(error.into()),
                        Notice::Stop => {
                            recording.drain(&frames)?;
                            return Ok(());
                        }
                    }
                }
            })();
            if let Err(error) = result {
                frames.fail(&error);
            }
        });
        Ok(Self {
            stop: send,
            worker: Some(worker),
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stop.send(Notice::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Recording {
    reader: BufReader<File>,
    partial_line: Vec<u8>,
    replay: Option<Replay>,
}

impl Recording {
    fn new(file: File) -> Self {
        Self {
            reader: BufReader::new(file),
            partial_line: Vec::new(),
            replay: None,
        }
    }

    fn drain(&mut self, frames: &Frames) -> Result<()> {
        while self.reader.read_until(b'\n', &mut self.partial_line)? > 0 {
            if !self.partial_line.ends_with(b"\n") {
                break;
            }
            let line = std::mem::take(&mut self.partial_line);
            if let Some(replay) = &mut self.replay {
                let (_, kind, data): (f64, String, String) = serde_json::from_slice(&line)?;
                replay.event(&kind, &data, frames)?;
            } else {
                #[derive(serde::Deserialize)]
                struct Header {
                    version: u8,
                    width: u16,
                    height: u16,
                }
                let header: Header = serde_json::from_slice(&line)?;
                ensure!(header.version == 2, "expected asciinema v2 recording");
                self.replay = Some(Replay::new(header.width, header.height)?);
            }
        }
        Ok(())
    }
}

struct Replay {
    emulator: Box<dyn Emulator>,
    pending: Vec<u8>,
    marker_prefix: usize,
}

impl Replay {
    fn new(cols: u16, rows: u16) -> Result<Self> {
        Ok(Self {
            emulator: Backend::Alacritty.build(cols, rows, &OpenOptions::default().profile)?,
            pending: Vec::new(),
            marker_prefix: 0,
        })
    }

    fn event(&mut self, kind: &str, data: &str, frames: &Frames) -> Result<()> {
        match kind {
            "o" => self.output(data.as_bytes(), frames)?,
            "r" => {
                // Resize events are ordered with PTY output in the cast.
                self.emulator.process(&std::mem::take(&mut self.pending));
                let (cols, rows) = data.split_once('x').context("invalid recorded resize")?;
                self.emulator.resize(cols.parse()?, rows.parse()?);
            }
            _ => {}
        }
        Ok(())
    }

    fn output(&mut self, bytes: &[u8], frames: &Frames) -> Result<()> {
        for byte in bytes {
            self.pending.push(*byte);
            self.marker_prefix = if *byte == FRAME_END[self.marker_prefix] {
                self.marker_prefix + 1
            } else {
                usize::from(*byte == FRAME_END[0])
            };
            if self.marker_prefix == FRAME_END.len() {
                self.marker_prefix = 0;
                self.emulator.process(&std::mem::take(&mut self.pending));
                frames.publish(Frame::capture(self.emulator.as_ref()))?;
                // Only the live tui-test session answers terminal queries.
                self.emulator.take_pending_writes();
            }
        }
        ensure!(
            self.pending.len() <= 4 * 1024 * 1024,
            "terminal output has no completed paint boundary"
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "capture.tests.rs"]
mod tests;
