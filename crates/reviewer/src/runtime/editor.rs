//! Hand the terminal to the user's editor for one file.

use std::env;
use std::ffi::OsString;
use std::io::{self, stdout};
use std::path::Path;
use std::process::Command;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// Run `$VISUAL` or `$EDITOR` on a file at a zero-based line, then restore the UI terminal modes.
///
/// The outer error means the terminal could not be restored; the inner one reports the editor.
pub(super) fn open(path: &Path, line: Option<u32>) -> io::Result<io::Result<()>> {
    let result = disable_raw_mode()
        .and_then(|()| super::leave_terminal_modes(&mut stdout()))
        .and_then(|()| editor_command(configured_editor(), path, line).status())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("editor exited with {status}")))
            }
        });
    enable_raw_mode()?;
    super::enter_terminal_modes(&mut stdout())?;
    Ok(result)
}

fn configured_editor() -> OsString {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(env::var_os)
        .find(|editor| !editor.is_empty())
        .unwrap_or_else(|| "vi".into())
}

/// Run the editor through the shell, like Git, so the variable may include arguments.
fn editor_command(editor: OsString, path: &Path, line: Option<u32>) -> Command {
    let mut script = editor;
    script.push(r#" "$@""#);
    let mut command = Command::new("sh");
    command.arg("-c").arg(script).arg("editor");
    if let Some(line) = line {
        command.arg(format!("+{}", line.saturating_add(1)));
    }
    command.arg(path);
    command
}

#[cfg(test)]
#[path = "editor.tests.rs"]
mod tests;
