//! A PNG of a screen, rendered in the font tui-test bundles: tui-test replays the styled screen
//! that Herdr read in a terminal of the same size, and photographs it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tui_test::{
    AutomaticRecording, OpenOptions, Operation, OperationResult, RunOptions, ScreenshotResult,
    Session, Timeouts,
};
use unicode_width::UnicodeWidthChar;

use super::screen::Screen;

/// A saved screenshot.
pub(crate) struct Screenshot {
    pub(crate) path: PathBuf,
    /// The characters no font on this machine could draw, drawn as `?` in their cells.
    pub(crate) replaced: Vec<char>,
}

/// Save a PNG of `screen`, read with its styles, as `path`. Characters that no outline font of
/// this machine draws (a color emoji, often) are drawn as `?`, one per cell, rather than failing.
pub(crate) fn save(screen: &Screen, path: &Path) -> Result<Screenshot> {
    let styled = screen
        .styled
        .as_deref()
        .context("a screenshot needs the screen's styles")?;
    match render(screen, styled, path) {
        Ok(path) => Ok(Screenshot {
            path,
            replaced: Vec::new(),
        }),
        Err(error) => {
            let replaced = missing_glyphs(&format!("{error:#}"));
            if replaced.is_empty() {
                return Err(error);
            }
            let path = render(screen, &replace(styled, &replaced), path)?;
            Ok(Screenshot { path, replaced })
        }
    }
}

fn render(screen: &Screen, styled: &str, path: &Path) -> Result<PathBuf> {
    let replay = path.with_extension("ansi");
    // Hide the cursor, and end on the last row: a final newline would scroll the screen.
    fs::write(
        &replay,
        format!(
            "\x1b[?25l{}",
            styled.trim_end_matches('\n').replace('\n', "\r\n")
        ),
    )?;
    let session = Session::new(format!(
        "vision-screenshot-{}-{}",
        std::process::id(),
        screen.frame
    ));
    let saved = (|| {
        let defaults = OpenOptions::default();
        session.run(RunOptions {
            program: "/bin/cat".into(),
            args: vec![path_string(&replay)?],
            cwd: None,
            env: Vec::new(),
            cols: screen.columns,
            rows: screen.rows,
            wait_ready: Some(false),
            backend: defaults.backend,
            profile: defaults.profile,
            restart: false,
            timeouts: Timeouts::default(),
            recording: AutomaticRecording::default(),
        })?;
        session.execute(Operation::WaitExit {
            timeout_ms: Some(30_000),
        })?;
        let OperationResult::Screenshot(ScreenshotResult::Path(saved)) =
            session.execute(Operation::Screenshot {
                full: false,
                path: Some(path_string(path)?),
                zoom: None,
                background: None,
            })?
        else {
            bail!("tui-test did not save the screenshot");
        };
        Ok(PathBuf::from(saved))
    })();
    let _ = session.close();
    let _ = fs::remove_file(&replay);
    saved
}

/// The characters tui-test's error names as glyphs it could not render: `'🦀' (U+1F980)`.
fn missing_glyphs(error: &str) -> Vec<char> {
    error
        .split("(U+")
        .skip(1)
        .filter_map(|rest| {
            let hex = rest.split(')').next()?;
            char::from_u32(u32::from_str_radix(hex, 16).ok()?)
        })
        .collect()
}

/// `styled` with each of `missing` drawn as `?` in each of its cells.
fn replace(styled: &str, missing: &[char]) -> String {
    styled
        .chars()
        .map(|character| {
            if missing.contains(&character) {
                "?".repeat(character.width().unwrap_or(1).max(1))
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn path_string(path: &Path) -> Result<String> {
    Ok(path.to_str().context("the path is not UTF-8")?.to_owned())
}

#[cfg(test)]
#[path = "screenshot.tests.rs"]
mod tests;
