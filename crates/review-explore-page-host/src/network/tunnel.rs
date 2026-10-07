//! The running round shared over a Cloudflare quick tunnel, for a coworker anywhere: the
//! reviewer turns it on for the round that runs, and it ends with that round.
//!
//! The tunnel forwards to a listener of its own on this machine's loopback interface, which
//! serves the page behind the round's token on the network, the one of the phone's QR code, and
//! never the page of this machine and its token. That listener answers only the tunnel's public
//! host name, over HTTPS, which it learns from `cloudflared` before it serves anything; it
//! closes with the tunnel, so no other listener ever answers that name. The tunnel stops when
//! the reviewer turns it off, when its round ends (a new round, a Reset), when the page host
//! drops, and when `cloudflared` fails; `cloudflared` runs as a fork of the reviewer, which it
//! does not outlive.

use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use quick_tunnel::{QuickTunnel, TunnelEvent, TunnelProgram};
use review_explore_page::{ExplorePage, Hosts, PageFiles, PageRound, Rounds, Token, page_listener};
pub use review_explore_page_tunnel::TunnelState;
use tokio::runtime::Handle;

use super::{PageNetwork, PageTask, RoundTokens, Shares};

/// Receives each state of the tunnel, in order.
pub type TunnelReport = Arc<dyn Fn(TunnelState) + Send + Sync>;

/// Why no tunnel opens when no round runs.
const NO_ROUND: &str = "no round runs, start one first";

/// The tunnel of a page host: the one that runs, if any, and those being stopped.
#[derive(Default)]
pub(super) struct Tunnels {
    current: Option<TunnelShare>,
    /// How many tunnels were opened. Each tunnel has its number, so that a report from a tunnel
    /// that was closed since is ignored.
    opened: u64,
    /// The threads that stop closed tunnels, away from the page's thread and the reviewer's.
    stopping: Vec<thread::JoinHandle<()>>,
    watcher: Watcher,
}

/// Tells the tests each stage through which the task that ends a tunnel with its round kept the
/// tunnel; tells nothing outside the tests.
#[derive(Clone, Default)]
struct Watcher(#[cfg(test)] Option<std::sync::mpsc::Sender<review_explore_page::RoundStage>>);

impl Watcher {
    #[cfg(test)]
    fn kept(&self, stages: &review_explore_page::RoundFeed) {
        if let Some(kept) = &self.0 {
            let _ = kept.send(stages.stage());
        }
    }

    #[cfg(not(test))]
    #[allow(clippy::unused_self)]
    fn kept(&self, _stages: &review_explore_page::RoundFeed) {}
}

impl Tunnels {
    /// Whether a tunnel runs or opens.
    pub(super) fn running(&self) -> bool {
        self.current.is_some()
    }

    /// The tunnel numbered `number`, or the tunnel that runs when `number` is `None`.
    fn current(&mut self, number: Option<u64>) -> Option<&mut TunnelShare> {
        self.current
            .as_mut()
            .filter(|tunnel| number.is_none_or(|number| tunnel.number == number))
    }

    /// Stops `tunnel` on a thread of its own, which the page host waits for when it drops.
    fn stop_away(&mut self, tunnel: TunnelShare) {
        self.stopping.retain(|stop| !stop.is_finished());
        // Without a thread, the tunnel stops here, as the failed spawn drops it.
        if let Ok(stop) = thread::Builder::new()
            .name("tunnel stop".into())
            .spawn(move || drop(tunnel))
        {
            self.stopping.push(stop);
        }
    }
}

impl Shares {
    /// Ends the tunnel `number`, or the one that runs when `None`, reporting it as `state`:
    /// its page admits nothing any more, and it stops on a thread of its own.
    fn end_tunnel(&mut self, number: Option<u64>, state: TunnelState) {
        if self.tunnels.current(number).is_none() {
            return;
        }
        let Some(tunnel) = self.tunnels.current.take() else {
            return;
        };
        self.release_tokens();
        tunnel.close();
        (tunnel.report)(state);
        self.tunnels.stop_away(tunnel);
    }
}

impl PageNetwork {
    /// Shares the running round over a quick tunnel that `program` runs. `report` receives each
    /// state of the tunnel: opening, then its public address with the round's token, or why it
    /// failed; and off once the reviewer turns it off or the round ends. While a tunnel runs
    /// already, `report` receives where it stands. Returns at once: the tunnel opens on threads
    /// of its own.
    pub fn open_tunnel(&self, program: &TunnelProgram, report: &TunnelReport) {
        let mut shares = self.lock();
        if let Some(tunnel) = shares.tunnels.current(None) {
            report(tunnel.state.clone());
            return;
        }
        match self.start_tunnel(&mut shares, program, report) {
            Ok(tunnel) => {
                report(TunnelState::Opening);
                shares.tunnels.current = Some(tunnel);
            }
            Err(reason) => {
                shares.release_tokens();
                report(TunnelState::Failed(reason));
            }
        }
    }

    /// Stops the tunnel, if one runs or opens: its page admits nothing any more, its listener
    /// closes, and `cloudflared` ends, on a thread of its own. Reports it off at once.
    pub fn close_tunnel(&self) {
        self.lock().end_tunnel(None, TunnelState::Off);
    }

    /// Stops the tunnel, if any, and waits until `cloudflared` ended, with every tunnel being
    /// stopped: the page host drops.
    pub(crate) fn end_tunnels(&self) {
        let (tunnel, stopping) = {
            let mut shares = self.lock();
            let tunnel = shares.tunnels.current.take();
            shares.release_tokens();
            (tunnel, std::mem::take(&mut shares.tunnels.stopping))
        };
        drop(tunnel);
        for stop in stopping {
            let _ = stop.join();
        }
    }

    /// Starts a tunnel for the running round, with the round's token and a listener of its own,
    /// or says why it cannot.
    fn start_tunnel(
        &self,
        shares: &mut Shares,
        program: &TunnelProgram,
        report: &TunnelReport,
    ) -> Result<TunnelShare, String> {
        let round = self.round.stages().round().ok_or(NO_ROUND)?;
        let tokens = shares.tokens(&self.round, &self.runtime);
        let token = tokens.token_of(&round).ok_or(NO_ROUND)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|listener| {
                listener.set_nonblocking(true)?;
                Ok((listener.local_addr()?, listener))
            })
            .map_err(|error| format!("cannot listen for the tunnel: {error}"))?;
        let (local, listener) = listener;
        shares.tunnels.opened += 1;
        let number = shares.tunnels.opened;
        let network = self.downgrade();
        let process = QuickTunnel::start(program, local, move |event| {
            if let Some(network) = network.upgrade() {
                network.tunnel_event(number, event);
            }
        })
        .map_err(|failure| failure.to_string())?;
        Ok(TunnelShare {
            number,
            page: TunnelRounds {
                tokens,
                round: round.clone(),
                open: Arc::new(AtomicBool::new(true)),
            },
            token,
            state: TunnelState::Opening,
            report: Arc::clone(report),
            listener: Some(listener),
            process: Some(process),
            tasks: vec![self.watch_round(number, round, shares.tunnels.watcher.clone())],
        })
    }

    /// Serves the round's page to the tunnel `number` once it has its public host name, or ends
    /// the tunnel that failed; unless that tunnel was closed since.
    fn tunnel_event(&self, number: u64, event: TunnelEvent) {
        let mut shares = self.lock();
        match event {
            TunnelEvent::Opened { host } => {
                if let Some(tunnel) = shares.tunnels.current(Some(number)) {
                    tunnel.serve(&host, &self.runtime);
                }
            }
            TunnelEvent::Failed(failure) => {
                shares.end_tunnel(Some(number), TunnelState::Failed(failure.to_string()));
            }
        }
    }

    /// Ends the tunnel `number` once its round no longer runs: a new round, a Reset, or the end
    /// of the reviewer's session. The page's thread runs this task, and the end waits for that
    /// thread: it runs on a thread of its own.
    fn watch_round(&self, number: u64, round: String, watcher: Watcher) -> PageTask {
        let mut stages = self.round.stages().clone();
        let network = self.downgrade();
        PageTask::spawn(&self.runtime, async move {
            while stages.round().as_deref() == Some(round.as_str()) {
                watcher.kept(&stages);
                if !stages.changed().await {
                    break;
                }
            }
            if let Some(network) = network.upgrade() {
                thread::spawn(move || network.lock().end_tunnel(Some(number), TunnelState::Off));
            }
        })
    }
}

/// The running round's page served to one tunnel, until it drops.
struct TunnelShare {
    number: u64,
    page: TunnelRounds,
    /// The round's token, which the tunnel's address carries.
    token: Token,
    /// Where the tunnel stands, as reported last.
    state: TunnelState,
    report: TunnelReport,
    /// The listener the tunnel forwards to, until the tunnel's public host name is known.
    listener: Option<TcpListener>,
    process: Option<QuickTunnel>,
    /// The task that ends the tunnel with its round, then the server.
    tasks: Vec<PageTask>,
}

impl TunnelShare {
    /// Serves the round's page on the tunnel's listener, for requests to `host` over HTTPS
    /// only, and reports the page's public address.
    fn serve(&mut self, host: &str, runtime: &Handle) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let page = ExplorePage::new(
            self.page.clone(),
            Hosts::tunnel(host),
            PageFiles::embedded(),
            |_| {},
        );
        let app = page.into_router(axum::Router::new());
        self.tasks.push(PageTask::spawn(runtime, async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            let _ = axum::serve(page_listener(listener), app).await;
        }));
        self.state = TunnelState::Open {
            url: self.token.tunnel_url(host),
        };
        (self.report)(self.state.clone());
    }

    /// Closes the tunnel's page at once: no request through the tunnel is admitted any more.
    fn close(&self) {
        self.page.open.store(false, Ordering::SeqCst);
    }
}

impl Drop for TunnelShare {
    /// Closes the page to the tunnel, then its listener, waiting briefly for the page's thread,
    /// then ends `cloudflared`, waiting until it ended.
    fn drop(&mut self) {
        self.close();
        for task in &self.tasks {
            task.abort();
        }
        for task in &self.tasks {
            task.wait_for_end();
        }
        drop(self.listener.take());
        drop(self.process.take());
    }
}

/// The round a tunnel shares, behind the round's token on the network, while the tunnel runs
/// and the round with it. A Reset from the tunnel's page hands it no token: the tunnel ends
/// with its round.
#[derive(Clone)]
struct TunnelRounds {
    tokens: RoundTokens,
    round: String,
    /// Whether the tunnel still runs.
    open: Arc<AtomicBool>,
}

impl Rounds for TunnelRounds {
    fn find(&self, token: &str) -> Option<PageRound> {
        let running = self.tokens.running();
        (self.open.load(Ordering::SeqCst) && running.as_deref() == Some(self.round.as_str()))
            .then(|| self.tokens.find(token))
            .flatten()
    }
}

#[cfg(test)]
#[path = "tunnel.tests.rs"]
mod tests;
