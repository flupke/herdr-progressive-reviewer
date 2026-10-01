//! A live stream of the session's screens: each frame the driver captures is
//! painted, whole, to every connected viewer the moment it is published.

use std::fmt::Write as _;
use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::{Context, Result};
use tui_test::terminal::cell::{Attrs, Color, EmuCell, UnderlineStyle};

use super::frame::Frame;

/// The socket viewers connect to, and the paints they are owed.
pub(super) struct FrameStream {
    path: PathBuf,
    viewers: Arc<Mutex<Viewers>>,
}

#[derive(Default)]
struct Viewers {
    connected: Vec<Arc<Viewer>>,
    /// The latest paint, sent to a viewer as soon as it connects.
    latest: Option<Arc<Vec<u8>>>,
}

/// One connected viewer: its own thread writes the newest paint, so a viewer
/// that reads slowly skips paints instead of stalling frame capture.
#[derive(Default)]
struct Viewer {
    next: Mutex<Option<Arc<Vec<u8>>>>,
    ready: Condvar,
    gone: AtomicBool,
    /// The session is over: the writer closes the connection.
    ended: AtomicBool,
}

impl Viewer {
    fn start(stream: UnixStream, latest: Option<Arc<Vec<u8>>>) -> Arc<Self> {
        let viewer = Arc::new(Self {
            next: Mutex::new(latest),
            ..Self::default()
        });
        let writer = Arc::clone(&viewer);
        thread::spawn(move || writer.write_paints(stream));
        viewer
    }

    fn end(&self) {
        // Take the lock so the writer cannot miss the wakeup between its
        // check and its wait.
        let _next = self.next.lock().unwrap();
        self.ended.store(true, Ordering::Relaxed);
        self.ready.notify_one();
    }

    fn offer(&self, paint: &Arc<Vec<u8>>) {
        *self.next.lock().unwrap() = Some(Arc::clone(paint));
        self.ready.notify_one();
    }

    fn write_paints(&self, mut stream: UnixStream) {
        loop {
            let paint = {
                let mut next = self.next.lock().unwrap();
                loop {
                    if let Some(paint) = next.take() {
                        break paint;
                    }
                    if self.ended.load(Ordering::Relaxed) {
                        // Dropping the connection tells the viewer the session ended.
                        return;
                    }
                    next = self.ready.wait(next).unwrap();
                }
            };
            if stream.write_all(&paint).is_err() {
                self.gone.store(true, Ordering::Relaxed);
                return;
            }
        }
    }
}

impl FrameStream {
    pub(super) fn start(path: &Path) -> Result<Self> {
        // A driver killed before cleaning up leaves its socket behind.
        if path
            .metadata()
            .is_ok_and(|metadata| metadata.file_type().is_socket())
        {
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)
            .with_context(|| format!("could not serve the live stream at {}", path.display()))?;
        let viewers = Arc::new(Mutex::new(Viewers::default()));
        let accepted = Arc::clone(&viewers);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut viewers = accepted.lock().unwrap();
                let viewer = Viewer::start(stream, viewers.latest.clone());
                viewers.connected.push(viewer);
            }
        });
        Ok(Self {
            path: path.to_owned(),
            viewers,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Offer `frame` to every viewer, forgetting viewers that went away.
    pub(super) fn send(&self, frame: &Frame) {
        let paint = Arc::new(paint(frame));
        let mut viewers = self.viewers.lock().unwrap();
        viewers
            .connected
            .retain(|viewer| !viewer.gone.load(Ordering::Relaxed));
        for viewer in &viewers.connected {
            viewer.offer(&paint);
        }
        viewers.latest = Some(paint);
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        for viewer in &self.viewers.lock().unwrap().connected {
            viewer.end();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A short socket path: socket addresses hold about a hundred bytes, which a
/// session directory or a nested `TMPDIR` easily exceeds.
pub(super) fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
        .join(format!("reviewer-vision-{}.sock", std::process::id()))
}

/// The whole screen as terminal output, in one synchronized update so a
/// viewer never shows half a frame.
fn paint(frame: &Frame) -> Vec<u8> {
    let mut paint = String::from("\x1b[?2026h\x1b[?25l\x1b[H\x1b[2J");
    for (row, cells) in frame.grid().iter().enumerate() {
        let _ = write!(paint, "\x1b[{};1H", row + 1);
        let mut style = String::new();
        for cell in cells.iter().filter(|cell| !cell.ch.is_empty()) {
            let next = sgr(cell);
            if next != style {
                paint.push_str(&next);
                style = next;
            }
            paint.push_str(&cell.ch);
        }
        paint.push_str("\x1b[0m");
    }
    let cursor = frame.cursor();
    if cursor.visible {
        let _ = write!(paint, "\x1b[{};{}H\x1b[?25h", cursor.y + 1, cursor.x + 1);
    }
    paint.push_str("\x1b[?2026l");
    paint.into_bytes()
}

/// The select graphic rendition sequence that gives a cell its style.
fn sgr(cell: &EmuCell) -> String {
    let mut sgr = String::from("\x1b[0");
    for (attr, code) in [
        (Attrs::BOLD, "1"),
        (Attrs::DIM, "2"),
        (Attrs::ITALIC, "3"),
        (Attrs::BLINK, "5"),
        (Attrs::INVERSE, "7"),
        (Attrs::INVISIBLE, "8"),
        (Attrs::STRIKE, "9"),
    ] {
        if cell.attrs.contains(attr) {
            let _ = write!(sgr, ";{code}");
        }
    }
    if cell.underline != UnderlineStyle::None {
        sgr.push_str(";4");
    }
    if let Some(color) = cell.fg {
        color_code(&mut sgr, color, 30);
    }
    if let Some(color) = cell.bg {
        color_code(&mut sgr, color, 40);
    }
    sgr.push('m');
    sgr
}

/// Append a colour: `base` is 30 for foreground and 40 for background.
fn color_code(sgr: &mut String, color: Color, base: u8) {
    let _ = match color {
        Color::Named(name) if name.index() < 8 => write!(sgr, ";{}", base + name.index()),
        Color::Named(name) => write!(sgr, ";{}", base + 60 + name.index() - 8),
        Color::Idx(index) => write!(sgr, ";{};5;{index}", base + 8),
        Color::Rgb(red, green, blue) => write!(sgr, ";{};2;{red};{green};{blue}", base + 8),
    };
}

#[cfg(test)]
#[path = "stream.tests.rs"]
mod tests;
