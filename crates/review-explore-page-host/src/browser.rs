//! Opening the page of a workspace's open reviewer in the default browser of this machine, for
//! the Herdr action and for the pane's Start and Start with Challenger. `BROWSER` names the
//! program and its arguments; by default `xdg-open` on Linux, `open` on macOS.

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use herdr_client::protocol::WorkspaceId;
use review_explore_page_opening::PageNotOpened;

use crate::PageDirectory;

/// Opens the page that the open reviewer of a workspace serves on this machine.
#[derive(Clone, Debug)]
pub struct PageOpener {
    pages: PageDirectory,
    workspace: WorkspaceId,
    browser: Browser,
}

impl PageOpener {
    pub fn new(pages: PageDirectory, workspace: WorkspaceId, browser: Browser) -> Self {
        Self {
            pages,
            workspace,
            browser,
        }
    }

    /// Opens the page in the browser. Waits for the browser program to exit: run it off any
    /// thread that must stay responsive.
    pub fn open(&self) -> Result<(), PageNotOpened> {
        let url = self
            .pages
            .address(&self.workspace)
            .map_err(|error| PageNotOpened {
                url: None,
                reason: format!("Cannot read the address of the Explore page: {error}"),
            })?
            .ok_or_else(|| PageNotOpened {
                url: None,
                reason: "No open reviewer in this workspace: open the progressive reviewer, then its Explore page".into(),
            })?;
        self.browser.open(&url).map_err(|reason| PageNotOpened {
            url: Some(url),
            reason,
        })
    }
}

/// The program that opens an address in the default browser: `$BROWSER` when it is set (a
/// program and its arguments, separated by spaces), `xdg-open` on Linux, `open` on macOS.
#[derive(Clone, Debug)]
pub struct Browser {
    program: OsString,
    arguments: Vec<OsString>,
}

impl Browser {
    pub fn from_env() -> Self {
        match env::var("BROWSER") {
            Ok(command) if !command.trim().is_empty() => Self::command(&command),
            _ if cfg!(target_os = "macos") => Self::command("open"),
            _ => Self::command("xdg-open"),
        }
    }

    /// The program and arguments of `command`, separated by spaces, to which the address is
    /// added.
    pub fn command(command: &str) -> Self {
        let mut words = command.split_whitespace().map(OsString::from);
        Self {
            program: words.next().unwrap_or_default(),
            arguments: words.collect(),
        }
    }

    /// Opens `url`. The program's output is dropped: it would draw over the pane. A failure
    /// names the program by its file name, which the pane has room for.
    fn open(&self, url: &str) -> Result<(), String> {
        let program = Path::new(&self.program)
            .file_name()
            .unwrap_or(&self.program)
            .to_string_lossy();
        let status = Command::new(&self.program)
            .args(&self.arguments)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("{program} did not start: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{program} failed ({status})"))
        }
    }
}

#[cfg(test)]
#[path = "browser.tests.rs"]
mod tests;
