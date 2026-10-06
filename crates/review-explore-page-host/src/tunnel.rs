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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use quick_tunnel::{QuickTunnel, TunnelEvent, TunnelProgram};
use review_explore_page::{ExplorePage, Hosts, PageFiles, PageRound, Rounds, Token};
pub use review_explore_page_tunnel::TunnelState;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::network::{PageNetwork, RoundTokens, Shares, wait_for_end};

/// Receives each state of the tunnel, in order.
pub type TunnelReport = Arc<dyn Fn(TunnelState) + Send + Sync>;

/// What the tunnel's page says when no round runs to share.
const NO_ROUND: &str = "no round runs: start one, then share it";

impl PageNetwork {
    /// Shares the running round over a quick tunnel that `program` runs, unless a tunnel runs
    /// already. `report` receives each state of the tunnel: opening, then its public address
    /// with the round's token, or why it failed; and off once the reviewer turns it off or the
    /// round ends. Returns at once: the tunnel opens on threads of its own.
    pub fn open_tunnel(&self, program: &TunnelProgram, report: &TunnelReport) {
        let mut shares = self.lock();
        if shares.tunnel.is_some() {
            return;
        }
        let Some(round) = self.round.stages().round() else {
            report(TunnelState::Failed(NO_ROUND.into()));
            return;
        };
        let tokens = shares.tokens(&self.round, &self.runtime);
        let opened = tokens
            .token_of(&round)
            .ok_or_else(|| NO_ROUND.to_owned())
            .and_then(|token| {
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                    .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
                    .map_err(|error| format!("cannot listen for the tunnel: {error}"))?;
                Ok((token, listener))
            });
        let (token, listener) = match opened {
            Ok(opened) => opened,
            Err(reason) => {
                shares.release_tokens();
                report(TunnelState::Failed(reason));
                return;
            }
        };
        shares.tunnels += 1;
        let number = shares.tunnels;
        let local = listener.local_addr().ok();
        let opening = Opening {
            shares: Arc::downgrade(&self.shares),
            runtime: self.runtime.clone(),
            number,
        };
        let process = local
            .ok_or_else(|| "the tunnel's listener has no address".to_owned())
            .and_then(|local| {
                QuickTunnel::start(program, local, move |event| opening.event(event))
                    .map_err(|failure| failure.to_string())
            });
        let process = match process {
            Ok(process) => process,
            Err(reason) => {
                shares.release_tokens();
                report(TunnelState::Failed(reason));
                return;
            }
        };
        let watch = self.watch_round(number, round.clone());
        shares.tunnel = Some(TunnelShare {
            number,
            page: TunnelRounds {
                tokens,
                round,
                open: Arc::new(AtomicBool::new(true)),
            },
            token,
            report: Arc::clone(report),
            listener: Some(listener),
            process: Some(process),
            tasks: vec![watch],
        });
        report(TunnelState::Opening);
    }

    /// Stops the tunnel, if one runs or opens: its page admits nothing any more, its listener
    /// closes, and `cloudflared` ends, on a thread of its own. Reports it off at once.
    pub fn close_tunnel(&self) {
        self.close_tunnel_numbered(None);
    }

    /// Stops the tunnel `number`, or any when `None`, and reports it off.
    fn close_tunnel_numbered(&self, number: Option<u64>) {
        let mut shares = self.lock();
        let Some(tunnel) = shares
            .tunnel
            .take_if(|tunnel| number.is_none_or(|number| tunnel.number == number))
        else {
            return;
        };
        shares.release_tokens();
        (tunnel.report)(TunnelState::Off);
        stop_away(&mut shares, tunnel);
    }

    /// Stops the tunnel, if any, and waits until `cloudflared` ended, with every tunnel being
    /// stopped: the page host drops.
    pub(crate) fn end_tunnels(&self) {
        let (tunnel, stopping) = {
            let mut shares = self.lock();
            let tunnel = shares.tunnel.take();
            shares.release_tokens();
            (tunnel, std::mem::take(&mut shares.stopping))
        };
        drop(tunnel);
        for stop in stopping {
            let _ = stop.join();
        }
    }

    /// Stops the tunnel `number` once its round no longer runs: a new round, a Reset, or the
    /// end of the reviewer's session. The stop locks the shares away from the page's thread,
    /// which may be what a stop waits for.
    fn watch_round(&self, number: u64, round: String) -> JoinHandle<()> {
        let mut stages = self.round.stages().clone();
        let network = Arc::downgrade(&self.shares);
        let page = self.round.clone();
        let runtime = self.runtime.clone();
        self.runtime.spawn(async move {
            loop {
                if stages.round().as_deref() != Some(round.as_str()) || !stages.changed().await {
                    break;
                }
            }
            if let Some(shares) = network.upgrade() {
                let network = PageNetwork {
                    round: page,
                    runtime,
                    shares,
                };
                thread::spawn(move || network.close_tunnel_numbered(Some(number)));
            }
        })
    }
}

/// Stops `tunnel` on a thread of its own, which the page host waits for when it drops.
fn stop_away(shares: &mut Shares, tunnel: TunnelShare) {
    shares.stopping.retain(|stop| !stop.is_finished());
    // Without a thread, the tunnel stops here, as the failed spawn drops it.
    if let Ok(stop) = thread::Builder::new()
        .name("tunnel stop".into())
        .spawn(move || drop(tunnel))
    {
        shares.stopping.push(stop);
    }
}

/// What a tunnel's process reports to: the page's shares, while the page host lives.
struct Opening {
    shares: Weak<Mutex<Shares>>,
    runtime: Handle,
    number: u64,
}

impl Opening {
    /// Serves the round's page to the tunnel once it has its public host name, or stops the
    /// tunnel that failed; unless the tunnel was closed since.
    fn event(&self, event: TunnelEvent) {
        let Some(shares) = self.shares.upgrade() else {
            return;
        };
        let mut shares = shares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(tunnel) = shares
            .tunnel
            .as_mut()
            .filter(|tunnel| tunnel.number == self.number)
        else {
            return;
        };
        match event {
            TunnelEvent::Opened { host } => tunnel.serve(&host, &self.runtime),
            TunnelEvent::Failed(failure) => {
                let Some(tunnel) = shares.tunnel.take() else {
                    return;
                };
                shares.release_tokens();
                (tunnel.report)(TunnelState::Failed(failure.to_string()));
                stop_away(&mut shares, tunnel);
            }
        }
    }
}

/// The running round's page served to one tunnel, until it drops.
pub(crate) struct TunnelShare {
    number: u64,
    page: TunnelRounds,
    /// The round's token, which the tunnel's address carries.
    token: Token,
    report: TunnelReport,
    /// The listener the tunnel forwards to, until the tunnel's public host name is known.
    listener: Option<TcpListener>,
    process: Option<QuickTunnel>,
    /// The task that stops the tunnel with its round, then the server.
    tasks: Vec<JoinHandle<()>>,
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
        self.tasks.push(runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                return;
            };
            let _ = axum::serve(listener, app).await;
        }));
        (self.report)(TunnelState::Open {
            url: self.token.tunnel_url(host),
        });
    }
}

impl Drop for TunnelShare {
    /// Closes the page to the tunnel, then its listener, waiting briefly for the page's thread,
    /// then ends `cloudflared`, waiting until it ended.
    fn drop(&mut self) {
        self.page.open.store(false, Ordering::SeqCst);
        for task in &self.tasks {
            task.abort();
        }
        for task in &self.tasks {
            wait_for_end(task);
        }
        drop(self.listener.take());
        if let Some(process) = self.process.take() {
            process.stop();
        }
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
