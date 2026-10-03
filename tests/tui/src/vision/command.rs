use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    Observe {
        after: Option<u64>,
        timeout_ms: Option<u64>,
    },
    Press {
        key: String,
    },
    Type {
        text: String,
    },
    /// Click a cell, or the first place the screen shows `text`.
    Click {
        x: Option<u16>,
        y: Option<u16>,
        text: Option<String>,
    },
    /// Wait until the screen shows `text`.
    Wait {
        text: String,
        timeout_ms: Option<u64>,
    },
    /// Save a PNG of the screen for judging how it looks.
    Screenshot,
    Resize {
        cols: u16,
        rows: u16,
    },
    Cells {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
    Note {
        kind: NoteKind,
        text: String,
    },
    /// Classify the hunks holding these one-based current lines of `path`
    /// insignificant, as Jev would, and run `rf`.
    Jev {
        path: String,
        lines: Vec<u32>,
    },
    /// Wait for the next Explore prompt the reviewer sent to its agent: the
    /// first turn after `after`, or after the last one this command returned.
    Turn {
        after: Option<u64>,
        timeout_ms: Option<u64>,
    },
    /// Answer turn `turn`, the latest one, by calling an MCP tool as the
    /// agent, with that turn's access and identity unless `arguments` set them.
    Reply {
        turn: u64,
        tool: String,
        arguments: serde_json::Value,
    },
    /// Run the Herdr action that opens the Explore page, as Herdr runs it in the
    /// session's workspace, with a browser that records the address it opens.
    ExplorePage,
    Reopen,
    Stop,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum NoteKind {
    Checked,
    Finding,
    Untested,
}
