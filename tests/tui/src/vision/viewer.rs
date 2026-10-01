//! The live viewer: shows a vision session's screens as the driver streams
//! them, and the Herdr pane that hosts it beside the driver's own pane.

use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result, ensure};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use tui_test::Size;

/// Enter the viewer's screen: alternate buffer, no cursor, and no line
/// wrapping, so rows wider than the pane are cut at its edge.
const ENTER: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J";
/// Leave it, ending any synchronized update a cut-off paint left open.
const LEAVE: &[u8] = b"\x1b[?2026l\x1b[0m\x1b[?7h\x1b[?25h\x1b[?1049l";

/// Show the stream at `socket` until the session ends, `q` is pressed or a
/// signal stops it. A viewer running in its own Herdr pane closes that pane
/// as it leaves, which happens however the driver stops: the system closes a
/// dead process's connections.
pub fn view(socket: &Path, own_pane: Option<&str>) -> Result<()> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("no vision session streams at {}", socket.display()))?;
    let leaving = Arc::new(Leaving {
        terminal: KeyboardMode::quiet(),
        own_pane: own_pane.map(str::to_owned),
    });
    let mut signals = Signals::new([SIGHUP, SIGINT, SIGTERM])?;
    let on_signal = Arc::clone(&leaving);
    thread::spawn(move || {
        if signals.forever().next().is_some() {
            on_signal.now();
        }
    });
    if let Ok(mut keyboard) = File::open("/dev/tty") {
        let on_key = Arc::clone(&leaving);
        thread::spawn(move || {
            // Keys are not forwarded to the session; `q` only closes the viewer.
            let mut key = [0];
            while matches!(keyboard.read(&mut key), Ok(1)) {
                if key[0] == b'q' {
                    on_key.now();
                }
            }
        });
    }
    // Lock stdout per write: a key or a signal leaves through it too.
    let result = show(&mut stream, &mut std::io::stdout());
    leaving.finish();
    result
}

/// What the viewer undoes as it leaves.
struct Leaving {
    terminal: Option<KeyboardMode>,
    own_pane: Option<String>,
}

impl Leaving {
    /// Leave from a key or a signal, while the stream is still running.
    fn now(&self) -> ! {
        let mut terminal = std::io::stdout();
        let _ = terminal.write_all(LEAVE);
        let _ = terminal.flush();
        self.finish();
        std::process::exit(0);
    }

    fn finish(&self) {
        if let Some(terminal) = &self.terminal {
            terminal.restore();
        }
        if let (Some(pane), Some((herdr, _))) = (&self.own_pane, Herdr::current()) {
            herdr.close_pane(pane);
        }
    }
}

/// The terminal's keyboard settings, saved while keys are read one at a
/// time without echo, so a keypress never draws over the stream.
struct KeyboardMode {
    saved: String,
}

impl KeyboardMode {
    /// Turn echo and line buffering off, or `None` when stdin is no terminal.
    fn quiet() -> Option<Self> {
        let saved = stty(&["-g"])?;
        stty(&["-echo", "-icanon", "min", "1"])?;
        Some(Self {
            saved: saved.trim().to_owned(),
        })
    }

    fn restore(&self) {
        let _ = stty(&[&self.saved]);
    }
}

fn stty(arguments: &[&str]) -> Option<String> {
    let output = Command::new("stty")
        .args(arguments)
        .stdin(File::open("/dev/tty").ok()?)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Copy the stream to the terminal until it ends, inside the viewer's screen.
fn show(stream: &mut impl Read, terminal: &mut impl Write) -> Result<()> {
    terminal.write_all(ENTER)?;
    terminal.flush()?;
    let mut buffer = vec![0; 64 * 1024];
    let result = (|| -> Result<()> {
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            terminal.write_all(&buffer[..read])?;
            terminal.flush()?;
        }
    })();
    terminal.write_all(LEAVE)?;
    terminal.flush()?;
    result
}

/// Where the viewer pane opens beside the driver's pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Split {
    Right,
    Down,
}

impl Split {
    fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }

    /// The pane length this split divides, and the one it leaves whole.
    fn lengths(self, size: Size) -> (u16, u16) {
        match self {
            Self::Right => (size.cols, size.rows),
            Self::Down => (size.rows, size.cols),
        }
    }
}

/// Where the viewer pane opens. A part left `None` is fitted to the driver's
/// pane, so the viewer shows the whole session screen when the pane allows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub split: Option<Split>,
    /// The viewer's share of the split pane.
    pub ratio: Option<f64>,
}

/// The fitted viewer takes this share of the driver's pane at most, and at
/// least its complement, so neither pane disappears.
const MAX_SHARE: f64 = 0.75;
/// Cells a Herdr pane may spend on its border.
const BORDER: u16 = 2;

impl Placement {
    /// The split and share, when both were chosen.
    fn chosen(self) -> Option<(Split, f64)> {
        Some((self.split?, self.ratio?))
    }

    /// Choose the split and share that show most of a `session`-sized screen
    /// in a `pane`-sized pane, preferring the one leaving the driver more room.
    fn fit(self, pane: Size, session: Size) -> (Split, f64) {
        let fitted = |split: Split| {
            let (divided, whole) = split.lengths(pane);
            let (needed, across) = split.lengths(session);
            let needed = f64::from(needed + BORDER);
            // Half a cell more keeps rounding in Herdr from cutting a cell.
            let ratio = self.ratio.unwrap_or_else(|| {
                ((needed + 0.5) / f64::from(divided.max(1))).clamp(1.0 - MAX_SHARE, MAX_SHARE)
            });
            // The share of the session screen the viewer shows.
            let visible = (ratio * f64::from(divided) / needed).min(1.0)
                * (f64::from(whole) / f64::from(across + BORDER)).min(1.0);
            (split, ratio, visible)
        };
        let candidates = match self.split {
            Some(split) => vec![fitted(split)],
            None => vec![fitted(Split::Right), fitted(Split::Down)],
        };
        let (split, ratio, _) = candidates
            .into_iter()
            .max_by(|(_, left_ratio, left), (_, right_ratio, right)| {
                left.total_cmp(right)
                    .then(right_ratio.total_cmp(left_ratio))
            })
            .expect("a split is always a candidate");
        (split, ratio)
    }
}

/// The Herdr command line, aimed at one server.
pub(super) struct Herdr {
    binary: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl Herdr {
    /// The Herdr the driver runs in, and the driver's pane, when it runs in one.
    pub(super) fn current() -> Option<(Self, String)> {
        std::env::var_os("HERDR_SOCKET_PATH")?;
        let pane = std::env::var("HERDR_PANE_ID").ok()?;
        let binary = std::env::var_os("HERDR_BIN_PATH")
            .map_or_else(|| PathBuf::from("herdr"), PathBuf::from);
        Some((
            Self {
                binary,
                environment: Vec::new(),
            },
            pane,
        ))
    }

    #[cfg(test)]
    pub(super) fn with_environment(
        binary: PathBuf,
        environment: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            binary,
            environment: environment.into_iter().collect(),
        }
    }

    /// The size of `pane`, in cells.
    fn pane_size(&self, pane: &str) -> Result<Size> {
        let response: serde_json::Value =
            serde_json::from_slice(&self.run(&["pane", "layout", "--pane", pane])?)?;
        let rect = response["result"]["layout"]["panes"]
            .as_array()
            .and_then(|panes| panes.iter().find(|entry| entry["pane_id"] == pane))
            .map(|entry| &entry["rect"])
            .context("herdr pane layout did not list the pane")?;
        let length = |name: &str| -> Result<u16> {
            Ok(u16::try_from(
                rect[name].as_u64().context("pane size is not a number")?,
            )?)
        };
        Ok(Size {
            cols: length("width")?,
            rows: length("height")?,
        })
    }

    fn close_pane(&self, pane: &str) {
        let _ = self.run(&["pane", "close", pane]);
    }

    fn run(&self, arguments: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new(&self.binary)
            .args(arguments)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .output()?;
        ensure!(
            output.status.success(),
            "herdr {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output.stdout)
    }
}

/// A Herdr pane running the live viewer; closing the session closes it.
pub(super) struct ViewerPane {
    herdr: Herdr,
    pane: String,
}

impl ViewerPane {
    /// Split `target` as `placement` says, fitting what it leaves open to a
    /// `session`-sized screen, and run the viewer command, given the new
    /// pane's ID, in the new pane.
    pub(super) fn open(
        herdr: Herdr,
        target: &str,
        placement: Placement,
        session: Size,
        viewer: impl FnOnce(&str) -> Vec<String>,
    ) -> Result<Self> {
        let (split, ratio) = match placement.chosen() {
            Some(chosen) => chosen,
            None => placement.fit(herdr.pane_size(target)?, session),
        };
        let response: serde_json::Value = serde_json::from_slice(&herdr.run(&[
            "pane",
            "split",
            "--pane",
            target,
            "--direction",
            split.name(),
            // Herdr's ratio is the share the split pane keeps.
            "--ratio",
            &(1.0 - ratio).to_string(),
            "--no-focus",
        ])?)?;
        let pane = response["result"]["pane"]["pane_id"]
            .as_str()
            .context("herdr pane split returned no pane")?
            .to_owned();
        let opened = Self { herdr, pane };
        let command = viewer(&opened.pane);
        let mut run = vec!["pane", "run", opened.pane.as_str()];
        run.extend(command.iter().map(String::as_str));
        opened.herdr.run(&run)?;
        Ok(opened)
    }

    #[cfg(test)]
    pub(super) fn pane(&self) -> &str {
        &self.pane
    }
}

impl Drop for ViewerPane {
    fn drop(&mut self) {
        self.herdr.close_pane(&self.pane);
    }
}

#[cfg(test)]
#[path = "viewer.tests.rs"]
mod tests;
