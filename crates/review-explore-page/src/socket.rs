//! The page's socket, at `/ws`: one for each open page, in both directions. The tool sends the
//! whole view when the socket opens and again at each change, numbered; nothing is replayed
//! after a reconnect, since the new socket gets the current view. The page sends the reviewer's
//! actions, and gets one reply for each (`crate::rpc`). A ping every few seconds lets the page
//! notice a socket that died without closing.
//!
//! The upgrade passes the page's host and origin checks like any request, and needs the token's
//! cookie. A socket whose token no longer opens a round is closed with [`TOKEN_ENDED`], at the
//! latest when the page sends a request, which is not carried out.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use tokio::sync::mpsc;

use crate::access::TokenCookie;
use crate::actions::Actions;
use crate::conversation::ThreadsFeed;
use crate::page::{ExplorePage, PageEvent};
use crate::round::{PageRound, RoundFeed};
use crate::rpc::{Notification, Outcome, Reply, Request, RpcError, Seq, StateParams};
use crate::status::StatusCard;
use crate::view::PageView;
use crate::{Rounds, notice::Notice};

/// Wraps `listener` so that each page's connection sends without Nagle's algorithm: a reply leaves
/// at once instead of after the delayed acknowledgement of the view sent before it, about 40 ms
/// later.
pub fn page_listener(
    listener: tokio::net::TcpListener,
) -> impl axum::serve::Listener<Io = tokio::net::TcpStream, Addr = std::net::SocketAddr> {
    axum::serve::ListenerExt::tap_io(listener, |stream| {
        // A connection that keeps Nagle's algorithm is only slower.
        let _ = stream.set_nodelay(true);
    })
}

/// How often the tool pings each open page.
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(10);

/// The close code of a socket whose token no longer opens a round: the page says how to open
/// the round again, and does not reconnect (RFC 6455 §7.4.2, the private range).
pub(crate) const TOKEN_ENDED: u16 = 4001;

/// How long a message waits for a page that stopped reading.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The close code of a socket whose round's owner is gone: the page reconnects.
const GOING_AWAY: u16 = 1001;

/// The upgrade of a page whose cookie carries a round's token.
pub(crate) async fn upgrade<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(token) = TokenCookie::read(&headers).map(str::to_owned) else {
        return page.refuse(PageEvent::WrongToken).into_response();
    };
    let Some(round) = page.rounds().find(&token) else {
        return page.refuse(PageEvent::WrongToken).into_response();
    };
    upgrade.on_upgrade(move |socket| {
        Connection {
            page,
            token,
            round,
            sent: None,
            pending: 0,
        }
        .run(socket)
    })
}

/// One open page.
struct Connection<R> {
    page: Arc<ExplorePage<R>>,
    /// The token the page opened with.
    token: String,
    round: PageRound,
    /// The number of the latest view sent.
    sent: Option<Seq>,
    /// The requests the page sent that wait for their reply.
    pending: usize,
}

/// Whether a page's token still opens a round.
enum Admission {
    Open,
    /// It does not, but a request waits for its reply, which may hand the page another token.
    Waiting,
    Ended,
}

/// Why a connection ends.
enum End {
    /// The page closed it, or it broke.
    Closed,
    /// The token no longer opens a round.
    TokenEnded,
    /// The round's owner is gone: the reviewer stops.
    OwnerGone,
}

impl<R: Rounds> Connection<R> {
    async fn run(mut self, mut socket: WebSocket) {
        let end = self.serve(&mut socket).await;
        let frame = match end {
            End::Closed => None,
            End::TokenEnded => Some(CloseFrame {
                code: TOKEN_ENDED,
                reason: "The token no longer opens a round".into(),
            }),
            End::OwnerGone => Some(CloseFrame {
                code: GOING_AWAY,
                reason: "The review tool stops".into(),
            }),
        };
        if let Some(frame) = frame {
            let _ = socket.send(Message::Close(Some(frame))).await;
        }
    }

    async fn serve(&mut self, socket: &mut WebSocket) -> End {
        let mut stages: RoundFeed = self.round.stages.clone();
        let mut picks = self.page.picks.subscribe();
        let mut threads = self
            .round
            .conversation
            .as_ref()
            .map(|conversation| conversation.threads.clone());
        let (done, mut replies) = mpsc::unbounded_channel::<Reply>();
        let mut heartbeat = tokio::time::interval(HEARTBEAT);
        heartbeat.tick().await;
        if self.send_view(socket).await.is_err() {
            return End::Closed;
        }
        loop {
            let sent = tokio::select! {
                incoming = socket.recv() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        // A token can end with no change of the round, as when the tunnel the
                        // page came through stops: its requests are not carried out.
                        match self.admission() {
                            Admission::Open => self.receive(text.as_str(), &done),
                            Admission::Waiting => self.refuse(text.as_str(), &done),
                            Admission::Ended => return End::TokenEnded,
                        }
                        Ok(())
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => return End::Closed,
                    Some(Ok(_)) => Ok(()),
                },
                changed = stages.changed() => {
                    if !changed {
                        return End::OwnerGone;
                    }
                    match self.admission() {
                        Admission::Ended => return End::TokenEnded,
                        // A Reset that ended the token hands the page the next one with its
                        // reply.
                        Admission::Waiting => Ok(()),
                        Admission::Open => self.send_view(socket).await,
                    }
                }
                Ok(()) = picks.changed() => self.send_view(socket).await,
                true = threads_changed(&mut threads) => self.send_view(socket).await,
                Some(reply) = replies.recv() => {
                    self.pending -= 1;
                    if let Reply::Result { result: Outcome { reopen: Some(token), .. }, .. } = &reply {
                        self.token.clone_from(token);
                    }
                    // The view first, so that the page shows what the action changed once it
                    // has the reply.
                    let sent = match self.send_view(socket).await {
                        Ok(()) => send(socket, &reply).await,
                        Err(error) => Err(error),
                    };
                    if matches!(self.admission(), Admission::Ended) {
                        return End::TokenEnded;
                    }
                    sent
                }
                _ = heartbeat.tick() => match self.admission() {
                    Admission::Ended => return End::TokenEnded,
                    Admission::Waiting | Admission::Open => send(socket, &Notification::Ping).await,
                },
            };
            if sent.is_err() {
                return End::Closed;
            }
        }
    }

    /// Whether the page's token still opens a round.
    fn admission(&self) -> Admission {
        if self.page.rounds().find(&self.token).is_some() {
            Admission::Open
        } else if self.pending > 0 {
            Admission::Waiting
        } else {
            Admission::Ended
        }
    }

    /// Carries out a request on its own task, so that the socket keeps sending while the round's
    /// owner works; the reply comes back through `done`.
    fn receive(&mut self, text: &str, done: &mpsc::UnboundedSender<Reply>) {
        let request = match serde_json::from_str::<Request>(text) {
            Ok(request) => request,
            Err(error) => {
                self.answer_error(text, RpcError::INVALID_REQUEST, error.to_string(), done);
                return;
            }
        };
        self.pending += 1;
        let page = self.page.clone();
        let round = self.round.clone();
        let done = done.clone();
        tokio::spawn(async move {
            let actions = Actions {
                page: &page,
                round: &round,
            };
            let result = actions.call(request.call).await;
            let _ = done.send(reply(request.id, result));
        });
    }

    /// Refuses a request sent while the page's token opens no round any more, but an earlier
    /// request waits for its reply, which may hand the page another token.
    fn refuse(&mut self, text: &str, done: &mpsc::UnboundedSender<Reply>) {
        let message = "The token no longer opens a round".to_owned();
        self.answer_error(text, RpcError::STALE, message, done);
    }

    /// Answers the request `text` with an error, when its ID can be read; drops anything else.
    fn answer_error(
        &mut self,
        text: &str,
        code: i32,
        message: String,
        done: &mpsc::UnboundedSender<Reply>,
    ) {
        let Some(id) = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|value| value.get("id")?.as_u64())
        else {
            return;
        };
        self.pending += 1;
        let _ = done.send(Reply::Error {
            id,
            error: RpcError {
                code,
                message,
                data: None,
            },
        });
    }

    /// Sends the view, unless the page has this number already.
    async fn send_view(&mut self, socket: &mut WebSocket) -> Result<(), axum::Error> {
        let snapshot = self.round.stages.latest();
        let threads = self
            .round
            .conversation
            .as_ref()
            .map(|conversation| conversation.threads.latest());
        let seq = Seq {
            revision: snapshot.revision,
            picks: self.page.picks.count(snapshot.round.as_deref()),
            threads: threads.as_ref().map_or(0, |threads| threads.revision),
        };
        if self.sent == Some(seq) {
            return Ok(());
        }
        let view = PageView::new(&snapshot, &self.page.picks, threads.as_ref());
        let state = Notification::State(Box::new(StateParams {
            epoch: self.page.epoch().to_owned(),
            seq,
            view,
        }));
        send(socket, &state).await?;
        self.sent = Some(seq);
        Ok(())
    }
}

/// Waits for the next change of the review threads, when the page offers a conversation; never
/// returns otherwise. False once their publisher is gone, after which it never returns either.
async fn threads_changed(threads: &mut Option<ThreadsFeed>) -> bool {
    let Some(feed) = threads else {
        return std::future::pending().await;
    };
    if feed.changed().await {
        return true;
    }
    *threads = None;
    false
}

/// The reply to the request `id`.
fn reply(id: u64, result: Result<Outcome, Notice>) -> Reply {
    match result {
        Ok(result) => Reply::Result { id, result },
        Err(notice) => Reply::Error {
            id,
            error: RpcError {
                code: notice.code(),
                message: notice.message(),
                data: Some(StatusCard::of_notice(&notice)),
            },
        },
    }
}

/// Sends `message`, unless the page stops reading for longer than [`SEND_TIMEOUT`]: its socket
/// then ends, so that it does not hold the connection's other work.
async fn send(socket: &mut WebSocket, message: &impl serde::Serialize) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).map_err(axum::Error::new)?;
    tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(text.into())))
        .await
        .map_err(axum::Error::new)?
}

#[cfg(test)]
#[path = "socket.tests.rs"]
mod tests;
