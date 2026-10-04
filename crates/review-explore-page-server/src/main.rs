//! Serves the Explore page alone, with no pane, no agent and no Herdr: for working on the page,
//! and for its e2e tests (`tests/explore-page`).
//!
//! `explore-page-server [--port N] [--token T] [--data short|rich] [--dev DIR]`
//!
//! It listens on the loopback address and prints the address of a page that shows the fixed
//! question.
//!
//! - `--port 0` picks a free port.
//! - `--token` chooses the token of that address; it is random otherwise.
//! - `--data` picks what the agent posts ([`round_data`]): the `short` data set of the e2e
//!   tests, the default, or the `rich` one of the screenshot gallery, as long as a real round.
//! - `--dev` reads the templates and assets from `DIR`, the page crate's directory, on every
//!   request, and loads an open page again when one of them changes. A release build refuses
//!   it.
//!
//! Each e2e test opens its own session through the routes of [`control`].

mod changed_source;
mod control;
mod question_parts;
mod rich;
mod round_data;
mod sessions;
mod short;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use review_explore_page::{ExplorePage, Hosts, PageEvent, PageFiles, Token};

use crate::round_data::RoundData;
use crate::sessions::Sessions;

const USAGE: &str =
    "usage: explore-page-server [--port N] [--token T] [--data short|rich] [--dev DIR]";

struct Options {
    port: u16,
    token: Option<String>,
    data: &'static dyn RoundData,
    dev: Option<PathBuf>,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            port: 8790,
            token: None,
            data: &short::Short,
            dev: None,
        };
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
            match arg.as_str() {
                "--port" => {
                    let port = value()?;
                    options.port = port.parse().map_err(|_| format!("invalid port {port}"))?;
                }
                "--token" => options.token = Some(value()?),
                "--data" => {
                    let name = value()?;
                    options.data = round_data::by_name(&name)
                        .ok_or_else(|| format!("unknown data set {name}"))?;
                }
                "--dev" if cfg!(debug_assertions) => options.dev = Some(value()?.into()),
                "--dev" => return Err("--dev is only in debug builds".into()),
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

fn main() -> ExitCode {
    match Options::parse(std::env::args().skip(1)).and_then(serve) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("explore-page-server: {error}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn serve(options: Options) -> Result<(), String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, options.port));
    let listener = std::net::TcpListener::bind(address)
        .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
        .map_err(|error| format!("cannot listen on {address}: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let templates = match options.dev {
        Some(root) => PageFiles::from_dir(root.clone())
            .map_err(|error| format!("cannot watch {}: {error}", root.display()))?,
        None => PageFiles::embedded(),
    };

    let sessions = Sessions::new(options.data);
    let token = match options.token {
        Some(token) => Token::chosen(token)?,
        None => Token::random(),
    };
    let url = token.loopback_url(port);
    sessions.open(token, 1);
    let page = ExplorePage::new(sessions.clone(), Hosts::loopback(port), templates, log);
    let app = page.into_router(control::router(sessions.clone())).layer(
        axum::middleware::from_fn_with_state(sessions, control::away),
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|error| error.to_string())?;
        println!("Explore page: {url}");
        axum::serve(listener, app)
            .await
            .map_err(|error| error.to_string())
    })
}

/// The e2e run fails on a `csp violation` line in this log. The report comes from the browser:
/// it is quoted, so it cannot write a line of its own.
fn log(event: PageEvent) {
    match event {
        PageEvent::UnknownHost => println!("refused: unknown host"),
        PageEvent::ForeignOrigin => println!("refused: foreign origin"),
        PageEvent::WrongToken => println!("refused: wrong token"),
        PageEvent::CspViolation(report) => println!("csp violation: {report:?}"),
    }
}
