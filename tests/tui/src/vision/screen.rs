//! The reviewer's screen as Herdr shows it, and the input that points at its cells.

use anyhow::{Result, bail, ensure};
use serde::Serialize;
use unicode_width::UnicodeWidthStr;
use vision_signal::FrameMarker;

/// One screen of the reviewer's pane, read after the frame that `frame` numbers.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Screen {
    /// The frame the screen holds at least: Herdr read it after this frame's marker.
    pub(crate) frame: u64,
    pub(crate) columns: u16,
    pub(crate) rows: u16,
    /// The visible rows, one line each.
    pub(crate) text: String,
    /// The same rows with their colors and styles as ANSI sequences, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) styled: Option<String>,
}

impl Screen {
    pub(crate) fn new(marker: &FrameMarker, text: String, styled: Option<String>) -> Self {
        Self {
            frame: marker.frame,
            columns: marker.columns,
            rows: marker.rows,
            text,
            styled,
        }
    }

    /// The text as the tools return it: numbered rows, trimmed, with its empty rows and QR code
    /// folded.
    pub(crate) fn compact(&self) -> String {
        super::compact::whole(&self.text)
    }

    pub(crate) fn shows(&self, text: &str) -> bool {
        self.text.contains(text)
    }

    /// The zero-based cell where `needle` first shows, counted in terminal columns, so wide
    /// characters before it count twice.
    pub(crate) fn locate(&self, needle: &str) -> Result<Cell> {
        ensure!(!needle.is_empty(), "the text to click is empty");
        let mut found = self.text.lines().enumerate().filter_map(|(row, line)| {
            line.find(needle).map(|start| Cell {
                x: u16::try_from(line[..start].width()).unwrap_or(u16::MAX),
                y: u16::try_from(row).unwrap_or(u16::MAX),
            })
        });
        let Some(cell) = found.next() else {
            bail!("the screen does not show {needle:?}");
        };
        Ok(cell)
    }

    /// Check that `cell` is on the screen.
    pub(crate) fn contains(&self, cell: Cell) -> Result<()> {
        ensure!(
            cell.x < self.columns && cell.y < self.rows,
            "cell ({}, {}) is outside the {}x{} screen",
            cell.x,
            cell.y,
            self.columns,
            self.rows
        );
        Ok(())
    }
}

/// A zero-based terminal cell: `x` is the column, `y` the row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Cell {
    pub(crate) x: u16,
    pub(crate) y: u16,
}

impl Cell {
    /// A left click on this cell, as a terminal with SGR mouse reporting sends it: a press and a
    /// release, one-based.
    pub(crate) fn left_click(self) -> String {
        let (x, y) = (u32::from(self.x) + 1, u32::from(self.y) + 1);
        format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m")
    }
}

#[cfg(test)]
#[path = "screen.tests.rs"]
mod tests;
