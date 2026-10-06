//! What a reviewer in a `make vision` session tells its driver through the terminal, in the
//! byte stream of its pane, so that Herdr reports each signal after the screen it follows.
//!
//! - Each painted frame ends with a terminal title, the [`FrameMarker`], that numbers it. Herdr
//!   reports a changed title as a pane update, so the driver waits on an event instead of a
//!   delay, and the screen it reads then holds that frame at least.
//! - The driver pastes an [`acknowledgement_request`] after its own input. The reviewer reads
//!   the input first, then the request, and names the request in the next frame's marker: that
//!   frame shows the reviewer's reaction to the input.

/// The terminal title a frame marker starts with.
const TITLE_PREFIX: &str = "reviewer vision";

/// The pasted text that asks for an acknowledgement, before its number.
const REQUEST_PREFIX: &str = "reviewer-vision-acknowledge:";

/// What the reviewer's terminal title says after a painted frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameMarker {
    /// The frames painted since the reviewer started, this one included.
    pub frame: u64,
    /// The latest acknowledgement request the reviewer read, 0 before the first.
    pub acknowledged: u64,
    /// The terminal's width in cells when the frame was painted.
    pub columns: u16,
    /// The terminal's height in cells when the frame was painted.
    pub rows: u16,
}

impl FrameMarker {
    /// The terminal title that carries this marker.
    pub fn title(&self) -> String {
        format!(
            "{TITLE_PREFIX} frame={} ack={} size={}x{}",
            self.frame, self.acknowledged, self.columns, self.rows
        )
    }

    /// The escape sequence that sets the terminal title to [`Self::title`] (OSC 2).
    pub fn escape_sequence(&self) -> String {
        format!("\x1b]2;{}\x07", self.title())
    }

    /// The marker a terminal title carries, if it carries one.
    pub fn parse(title: &str) -> Option<Self> {
        let mut fields = title.strip_prefix(TITLE_PREFIX)?.split_whitespace();
        let frame = fields.next()?.strip_prefix("frame=")?.parse().ok()?;
        let acknowledged = fields.next()?.strip_prefix("ack=")?.parse().ok()?;
        let (columns, rows) = fields.next()?.strip_prefix("size=")?.split_once('x')?;
        if fields.next().is_some() {
            return None;
        }
        Some(Self {
            frame,
            acknowledged,
            columns: columns.parse().ok()?,
            rows: rows.parse().ok()?,
        })
    }
}

/// The text the driver pastes to ask for acknowledgement `id`.
pub fn acknowledgement_request(id: u64) -> String {
    format!("{REQUEST_PREFIX}{id}")
}

/// The acknowledgement a pasted text asks for, if it is a request.
pub fn parse_acknowledgement_request(text: &str) -> Option<u64> {
    text.strip_prefix(REQUEST_PREFIX)?.parse().ok()
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
