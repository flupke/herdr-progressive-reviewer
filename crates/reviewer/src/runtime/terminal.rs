use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

/// A backend that can discard cursor visibility cached before input or focus changes.
pub(super) trait CursorBackend: Backend {
    fn invalidate_cursor_visibility(&mut self);
}

/// Avoid terminal traffic for unchanged frames, including repeated cursor hides.
pub(super) struct TerminalBackend<W: Write> {
    inner: CrosstermBackend<W>,
    cursor_hidden: Option<bool>,
}

impl<W: Write> TerminalBackend<W> {
    pub(super) fn new(writer: W) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
            cursor_hidden: None,
        }
    }
}

impl<W: Write> CursorBackend for TerminalBackend<W> {
    fn invalidate_cursor_visibility(&mut self) {
        self.cursor_hidden = None;
    }
}

#[cfg(test)]
impl CursorBackend for ratatui::backend::TestBackend {
    // TestBackend does not cache cursor visibility commands.
    fn invalidate_cursor_visibility(&mut self) {}
}

impl<W: Write> Backend for TerminalBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
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
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
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
