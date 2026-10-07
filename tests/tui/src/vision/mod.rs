//! `make vision`: an MCP server that gives an agent eyes and hands on the reviewer, which runs in
//! a workspace of the user's Herdr on a scratch repository.

mod agent;
mod compact;
mod frames;
mod keys;
mod scratch;
mod screen;
mod screenshot;
mod server;
mod session;
mod workspace;

pub use server::{Setup, serve};
