use std::io::{self, Write};

use crossterm::QueueableCommand;
use crossterm::clipboard::CopyToClipboard;
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use vision_signal::FrameMarker;

/// The backend of the pane's terminal: draws frames, discards the cursor visibility cached before
/// input or focus changes, and writes to the clipboard.
pub(super) trait PaneBackend: Backend {
    fn invalidate_cursor_visibility(&mut self);

    /// Puts `text` on the clipboard with an OSC 52 write: Herdr takes a pane's write to the
    /// clipboard of the client in the foreground, as the outer terminal does without Herdr.
    fn copy_to_clipboard(&mut self, text: &str) -> io::Result<()>;

    /// Names acknowledgement request `id` in the frame markers of a vision session, from the
    /// next frame on; without frame markers, does nothing.
    fn acknowledge_input(&mut self, id: u64);
}

/// Avoid terminal traffic for unchanged frames, including repeated cursor hides.
pub(super) struct TerminalBackend<W: Write> {
    inner: CrosstermBackend<W>,
    cursor_hidden: Option<bool>,
    /// The frame markers of a vision session, `None` outside one.
    markers: Option<FrameMarker>,
    /// The size the terminal last reported, which the next frame marker names.
    size: std::cell::Cell<Size>,
    frame_open: bool,
}

impl<W: Write> TerminalBackend<W> {
    pub(super) fn new(writer: W) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
            cursor_hidden: None,
            markers: None,
            size: std::cell::Cell::new(Size::default()),
            frame_open: false,
        }
    }

    /// Paints each frame as one synchronized update and ends it with a [`FrameMarker`], for the
    /// driver of a vision session.
    pub(super) fn with_frame_markers(mut self) -> Self {
        self.markers = Some(FrameMarker::default());
        self
    }
}

impl<W: Write> PaneBackend for TerminalBackend<W> {
    fn invalidate_cursor_visibility(&mut self) {
        self.cursor_hidden = None;
    }

    fn copy_to_clipboard(&mut self, text: &str) -> io::Result<()> {
        self.inner.queue(CopyToClipboard::to_clipboard_from(text))?;
        Write::flush(&mut self.inner)
    }

    fn acknowledge_input(&mut self, id: u64) {
        if let Some(marker) = &mut self.markers {
            marker.acknowledged = id;
        }
    }
}

#[cfg(test)]
impl PaneBackend for ratatui::backend::TestBackend {
    // TestBackend does not cache cursor visibility commands.
    fn invalidate_cursor_visibility(&mut self) {}

    // TestBackend holds cells only: it has no output for an escape sequence.
    fn copy_to_clipboard(&mut self, _text: &str) -> io::Result<()> {
        Ok(())
    }

    // TestBackend paints no frame markers.
    fn acknowledge_input(&mut self, _id: u64) {}
}

impl<W: Write> Backend for TerminalBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        if self.markers.is_some() {
            self.inner.queue(BeginSynchronizedUpdate)?;
            self.frame_open = true;
        }
        let mut content = content.peekable();
        if content.peek().is_some() {
            // Reassert visibility before painting: the host may have exposed the
            // cursor since the last frame, leaving it on the last updated cell.
            self.inner.hide_cursor()?;
            self.cursor_hidden = Some(true);
            self.inner.draw(content)?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        if self.cursor_hidden != Some(true) {
            self.inner.hide_cursor()?;
            self.cursor_hidden = Some(true);
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        if self.cursor_hidden != Some(false) {
            self.inner.show_cursor()?;
            self.cursor_hidden = Some(false);
        }
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn append_lines(&mut self, count: u16) -> io::Result<()> {
        self.inner.append_lines(count)
    }

    fn size(&self) -> io::Result<Size> {
        let size = self.inner.size()?;
        self.size.set(size);
        Ok(size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.frame_open {
            self.inner.queue(EndSynchronizedUpdate)?;
            self.frame_open = false;
            let size = self.size.get();
            if let Some(marker) = &mut self.markers {
                marker.frame += 1;
                marker.columns = size.width;
                marker.rows = size.height;
                self.inner.write_all(marker.escape_sequence().as_bytes())?;
            }
        }
        Backend::flush(&mut self.inner)
    }
}

impl<W: Write> Write for TerminalBackend<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Raw terminal commands may change cursor visibility outside Backend.
        self.invalidate_cursor_visibility();
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

#[cfg(test)]
#[path = "terminal.tests.rs"]
mod tests;
