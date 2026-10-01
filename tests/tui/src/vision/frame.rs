use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, ensure};
use serde::Serialize;
use tui_test::profile::ColorSlot;
use tui_test::terminal::cell::{EmuCell, rows_to_strings};
use tui_test::terminal::emu::{ColorTable, Emulator};
use tui_test::{Cursor, Size};

const HISTORY_LIMIT: u64 = 64;
const QUIET_PERIOD: Duration = Duration::from_millis(100);

#[derive(Clone, Serialize)]
pub(super) struct Frame {
    pub(super) number: u64,
    pub(super) size: Size,
    cursor: Cursor,
    pub(super) text: String,
    #[serde(skip)]
    grid: Vec<Vec<EmuCell>>,
    #[serde(skip)]
    colors: ColorTable,
}

impl Frame {
    pub(super) fn capture(emulator: &dyn Emulator) -> Self {
        let grid = emulator.viewable_rows();
        let (cols, rows) = emulator.size();
        let (x, y) = emulator.cursor();
        let color = emulator.color(ColorSlot::Cursor);
        Self {
            number: 0,
            size: Size { cols, rows },
            cursor: Cursor {
                x,
                y,
                visible: emulator.cursor_visible(),
                shape: emulator.cursor_shape().name().into(),
                color: format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
            },
            text: rows_to_strings(&grid).join("\n"),
            grid,
            colors: emulator.colors(),
        }
    }

    pub(super) fn grid(&self) -> &[Vec<EmuCell>] {
        &self.grid
    }

    pub(super) fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    fn same_screen(&self, other: &Self) -> bool {
        self.size == other.size
            && self.grid == other.grid
            && self.colors == other.colors
            && self.cursor.x == other.cursor.x
            && self.cursor.y == other.cursor.y
            && self.cursor.visible == other.cursor.visible
            && self.cursor.shape == other.cursor.shape
            && self.cursor.color == other.cursor.color
    }

    pub(super) fn display(&self) -> String {
        format!(
            "Frame {} | {}x{} | cursor {} at {},{}\n{}\n",
            self.number,
            self.size.cols,
            self.size.rows,
            if self.cursor.visible {
                "visible"
            } else {
                "hidden"
            },
            self.cursor.x,
            self.cursor.y,
            self.text,
        )
    }

    pub(super) fn cells(&self, x: u16, y: u16, width: u16, height: u16) -> Result<Vec<String>> {
        ensure!(width > 0 && height > 0, "cell region must be nonempty");
        ensure!(
            u32::from(x) + u32::from(width) <= u32::from(self.size.cols)
                && u32::from(y) + u32::from(height) <= u32::from(self.size.rows),
            "cell region is outside the frame"
        );
        let mut cells = Vec::new();
        for row in y..y + height {
            for column in x..x + width {
                cells.push(format!(
                    "{column},{row}: {:?}",
                    self.grid[usize::from(row)][usize::from(column)]
                ));
            }
        }
        Ok(cells)
    }
}

pub(super) struct Frames {
    directory: PathBuf,
    stream: super::stream::FrameStream,
    state: Mutex<FrameState>,
    changed: Condvar,
}

struct FrameState {
    latest: Option<Frame>,
    changed_at: Instant,
    error: Option<String>,
}

impl Frames {
    pub(super) fn new(directory: PathBuf) -> Result<Self> {
        fs::create_dir(directory.join("frames"))?;
        Ok(Self {
            stream: super::stream::FrameStream::start(&super::stream::socket_path())?,
            directory,
            state: Mutex::new(FrameState {
                latest: None,
                changed_at: Instant::now(),
                error: None,
            }),
            changed: Condvar::new(),
        })
    }

    pub(super) fn publish(&self, mut frame: Frame) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if state
            .latest
            .as_ref()
            .is_some_and(|last| last.same_screen(&frame))
        {
            return Ok(());
        }
        frame.number = state.latest.as_ref().map_or(1, |last| last.number + 1);
        let text = frame.display();
        fs::write(self.path(frame.number), &text)?;
        // The frame ID, geometry, cursor and screen become visible together.
        fs::write(self.directory.join("latest.tmp"), &text)?;
        fs::rename(
            self.directory.join("latest.tmp"),
            self.directory.join("latest.txt"),
        )?;
        if frame.number > HISTORY_LIMIT {
            fs::remove_file(self.path(frame.number - HISTORY_LIMIT))?;
        }
        self.stream.send(&frame);
        state.latest = Some(frame);
        state.changed_at = Instant::now();
        self.changed.notify_all();
        Ok(())
    }

    pub(super) fn fail(&self, error: &anyhow::Error) {
        self.state.lock().unwrap().error = Some(format!("{error:#}"));
        self.changed.notify_all();
    }

    pub(super) fn latest(&self) -> Result<Frame> {
        self.wait(None, Duration::ZERO, false)
    }

    pub(super) fn wait(
        &self,
        after: Option<u64>,
        timeout: Duration,
        settle: bool,
    ) -> Result<Frame> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(error) = &state.error {
                return Err(anyhow!("frame capture failed: {error}"));
            }
            let fresh = state
                .latest
                .as_ref()
                .is_some_and(|frame| after.is_none_or(|number| frame.number > number));
            let quiet_at = state.changed_at + QUIET_PERIOD;
            let now = Instant::now();
            if (fresh && (!settle || now >= quiet_at)) || now >= deadline {
                return state
                    .latest
                    .clone()
                    .ok_or_else(|| anyhow!("no completed frame received"));
            }
            let wake = if fresh && settle {
                deadline.min(quiet_at)
            } else {
                deadline
            };
            (state, _) = self
                .changed
                .wait_timeout(state, wake.saturating_duration_since(now))
                .unwrap();
        }
    }

    /// Wait until the latest screen shows `text`. On timeout, return the
    /// latest screen and `false`.
    pub(super) fn wait_for_text(&self, text: &str, timeout: Duration) -> Result<(Frame, bool)> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(error) = &state.error {
                return Err(anyhow!("frame capture failed: {error}"));
            }
            let shown = state
                .latest
                .as_ref()
                .is_some_and(|frame| frame.text.contains(text));
            let now = Instant::now();
            if shown || now >= deadline {
                let frame = state
                    .latest
                    .clone()
                    .ok_or_else(|| anyhow!("no completed frame received"))?;
                return Ok((frame, shown));
            }
            (state, _) = self.changed.wait_timeout(state, deadline - now).unwrap();
        }
    }

    pub(super) fn path(&self, number: u64) -> PathBuf {
        self.directory
            .join("frames")
            .join(format!("{number:06}.txt"))
    }

    /// The socket a live viewer connects to.
    pub(super) fn stream_path(&self) -> &Path {
        self.stream.path()
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }
}
