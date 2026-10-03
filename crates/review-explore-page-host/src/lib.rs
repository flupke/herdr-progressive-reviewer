//! The Explore page of an open reviewer: served on this machine for the round of the
//! reviewer's Explore session, and found by the Herdr action that opens it in a browser.
//!
//! The reviewer starts a [`PageHost`], which serves the page on a free loopback port behind a
//! new token, and leaves the page's address in the plugin's state directory, in the record of
//! its Herdr workspace. The action reads that record through the [`PageDirectory`].

use std::fmt::Write as _;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use herdr_client::protocol::WorkspaceId;
use review_explore_page::{ExplorePage, Hosts, PageFiles, RoundFeed, Rounds, Token};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

/// How long the action waits for a recorded page to answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// Where open reviewers leave the address of their Explore page: one record per Herdr
/// workspace, readable only by the user, since the address carries the page's token.
pub struct PageDirectory(PathBuf);

/// The address of a page, as its record holds it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PageAddress {
    port: u16,
    /// The address that opens the page, with its token.
    url: String,
}

impl PageAddress {
    /// Whether the page answers: the reviewer that recorded it may have died without
    /// removing its record.
    fn answers(&self) -> bool {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let health = || -> io::Result<bool> {
            let mut stream = TcpStream::connect_timeout(&address, ANSWER_TIMEOUT)?;
            stream.set_read_timeout(Some(ANSWER_TIMEOUT))?;
            stream.set_write_timeout(Some(ANSWER_TIMEOUT))?;
            write!(
                stream,
                "GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )?;
            let mut status = [0; 12];
            stream.read_exact(&mut status)?;
            Ok(status.ends_with(b" 204"))
        };
        health().unwrap_or(false)
    }
}

impl PageDirectory {
    /// The records under the plugin's state directory.
    pub fn new(state_dir: &Path) -> Self {
        Self(state_dir.join("explore-page"))
    }

    /// The address of the page that the open reviewer of `workspace` serves, or `None` when
    /// no reviewer of that workspace serves one.
    pub fn address(&self, workspace: &WorkspaceId) -> io::Result<Option<String>> {
        let record = match fs::read(self.record(workspace)) {
            Ok(record) => record,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(serde_json::from_slice::<PageAddress>(&record)
            .ok()
            .filter(PageAddress::answers)
            .map(|address| address.url))
    }

    /// The record of `workspace`'s page, named after a digest of the workspace ID so that any
    /// ID makes a file name.
    fn record(&self, workspace: &WorkspaceId) -> PathBuf {
        let digest = Sha256::digest(workspace.0.as_bytes());
        let name = digest.iter().fold(String::new(), |mut name, byte| {
            let _ = write!(name, "{byte:02x}");
            name
        });
        self.0.join(name)
    }

    /// Writes `address` to `record`, one of this directory's records, replacing any earlier
    /// record whole.
    fn publish(&self, record: &Path, address: &PageAddress) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.0)?;
        let partial = record.with_extension(format!("{}.partial", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&partial)?;
        file.write_all(&serde_json::to_vec(address)?)?;
        file.sync_all()?;
        fs::rename(&partial, record)
    }
}

/// The Explore page of one reviewer, served on this machine until the host drops. It shows the
/// round that the reviewer's Explore session publishes.
pub struct PageHost {
    address: PageAddress,
    record: PathBuf,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl PageHost {
    /// Serves the page of `round` on a free loopback port, behind a new token, and records its
    /// address for `workspace` in `directory`.
    pub fn start(
        round: RoundFeed,
        directory: &PageDirectory,
        workspace: &WorkspaceId,
    ) -> io::Result<Self> {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = Token::random();
        let address = PageAddress {
            port,
            url: token.url(port),
        };
        let page = ExplorePage::new(
            OneRound { token, round },
            Hosts::loopback(port),
            PageFiles::embedded(),
            |_| {},
        );
        let app = page.into_router(axum::Router::new());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = thread::Builder::new()
            .name("explore-page".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        return;
                    };
                    let _ = axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = stopped.await;
                        })
                        .await;
                });
            })?;
        let host = Self {
            address,
            record: directory.record(workspace),
            stop: Some(stop),
            thread: Some(thread),
        };
        // Once recorded, the address leaves with the host, however this ends.
        directory.publish(&host.record, &host.address)?;
        Ok(host)
    }

    /// The address that opens the page, with its token.
    pub fn url(&self) -> &str {
        &self.address.url
    }
}

impl Drop for PageHost {
    fn drop(&mut self) {
        // A newer reviewer of the workspace may have replaced the record: leave its record.
        let ours = fs::read(&self.record)
            .ok()
            .and_then(|record| serde_json::from_slice::<PageAddress>(&record).ok())
            .is_some_and(|address| address == self.address);
        if ours {
            let _ = fs::remove_file(&self.record);
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The one round of a reviewer, behind the token of its page's address.
struct OneRound {
    token: Token,
    round: RoundFeed,
}

impl Rounds for OneRound {
    fn find(&self, token: &str) -> Option<RoundFeed> {
        self.token.matches(token).then(|| self.round.clone())
    }
}

#[cfg(test)]
mod tests;
