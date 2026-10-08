//! A scripted implementation agent for Explore and review threads. The reviewer
//! records every prompt it sent as a numbered turn; `turn` reads the next one and
//! `reply` answers it through the reviewer's real MCP endpoint.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

pub(super) struct ScriptedAgent {
    /// Where the reviewer records the turns it sent.
    turns: PathBuf,
    port: u16,
    /// The last turn `next_turn` returned.
    returned: u64,
}

impl ScriptedAgent {
    pub(super) fn new(turns: PathBuf, port: u16) -> Self {
        Self {
            turns,
            port,
            returned: 0,
        }
    }

    /// Every turn the reviewer recorded, oldest first.
    fn turns(&self) -> Result<Vec<Value>> {
        let Ok(entries) = std::fs::read_dir(&self.turns) else {
            return Ok(Vec::new());
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();
        paths
            .iter()
            .map(|path| {
                serde_json::from_slice(&std::fs::read(path)?)
                    .with_context(|| format!("unreadable turn {}", path.display()))
            })
            .collect()
    }

    /// The first turn after `after`, or after the last one returned, waiting up to `guard`
    /// for the reviewer to send it. The wait ends on the reviewer's write of a turn file, not
    /// on a poll.
    pub(super) fn next_turn(&mut self, after: Option<u64>, guard: Duration) -> Result<Value> {
        self.next_turn_watching(after, guard, || {})
    }

    /// [`Self::next_turn`], which calls `watching` once it watches the turns and found none.
    fn next_turn_watching(
        &mut self,
        after: Option<u64>,
        guard: Duration,
        watching: impl FnOnce(),
    ) -> Result<Value> {
        let after = after.unwrap_or(self.returned);
        std::fs::create_dir_all(&self.turns)?;
        let (written, writes) = mpsc::channel();
        // A turn arrives as a rename of its partial file; reading the turns, as this wait does,
        // makes access events, which must not wake it.
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            if event.is_ok_and(|event| {
                matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
            }) {
                let _ = written.send(());
            }
        })?;
        watcher.watch(&self.turns, RecursiveMode::NonRecursive)?;
        let deadline = Instant::now() + guard;
        let mut watching = Some(watching);
        loop {
            let turns = self.turns()?;
            // Only the very next number, so a turn still being written is never skipped.
            if let Some(turn) = turns.iter().find(|turn| number(turn) == after + 1) {
                self.returned = number(turn);
                return Ok(turn.clone());
            }
            if let Some(watching) = watching.take() {
                watching();
            }
            // Only a write ends the wait: once the guard is spent, no last look finds a turn.
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || writes.recv_timeout(remaining).is_err() {
                bail!(
                    "the reviewer sent no turn after turn {after} in {} ms; the latest is {}",
                    guard.as_millis(),
                    turns.last().map_or(0, number)
                );
            }
        }
    }

    /// Answer turn `turn` by calling `tool`: the turn must be the latest one,
    /// and its identity fills what `arguments` leaves out.
    pub(super) fn reply(&self, turn: u64, tool: &str, mut arguments: Value) -> Result<Value> {
        let turns = self.turns()?;
        let latest = turns
            .last()
            .context("the reviewer has not sent a prompt yet")?;
        ensure!(
            number(latest) == turn,
            "turn {turn} is stale; latest is {}",
            number(latest)
        );
        // A turn without an identity of its own takes a reply that brings one,
        // to check how the reviewer rejects it.
        if arguments.get("review").is_none() {
            Identity::of(latest)?.fill(tool, &mut arguments)?;
        }
        self.post(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }))
    }

    fn post(&self, body: &Value) -> Result<Value> {
        let body = serde_json::to_vec(body)?;
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .context("the reviewer's MCP endpoint is not listening")?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n",
            self.port,
            body.len()
        )?;
        stream.write_all(&body)?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        let (head, payload) = response
            .split_once("\r\n\r\n")
            .context("the MCP endpoint sent no HTTP body")?;
        let status = head.lines().next().unwrap_or_default();
        if !status.contains(" 200 ") {
            bail!("MCP call failed: {status}: {payload}");
        }
        let reply: Value = serde_json::from_str(payload.trim())
            .with_context(|| format!("unexpected MCP reply: {payload}"))?;
        Ok(reply.get("result").cloned().unwrap_or(reply))
    }
}

fn number(turn: &Value) -> u64 {
    turn["turn"].as_u64().unwrap_or(0)
}

/// What a reply to a turn must carry, as the reviewer recorded it.
struct Identity {
    access: Value,
    round: Value,
    request: Value,
    checkpoint: Value,
    /// The answer the turn brought, null on a kickoff.
    answer: Value,
}

impl Identity {
    fn of(turn: &Value) -> Result<Self> {
        ensure!(
            turn["delivered"] == true,
            "turn {} was not delivered: {}",
            number(turn),
            turn["error"]
        );
        ensure!(
            turn["access"].is_string(),
            "turn {} is an {} prompt; it takes no reply",
            number(turn),
            turn["kind"].as_str().unwrap_or("unknown")
        );
        Ok(Self {
            access: turn["access"].clone(),
            round: turn["round"].clone(),
            request: turn["request"].clone(),
            checkpoint: turn["checkpoint"].clone(),
            answer: turn["answer"]["id"].clone(),
        })
    }

    /// Give `arguments` this identity where they leave it out: the access
    /// value, the turn fields where `tool` takes them, and the answer an
    /// interpretation is about.
    fn fill(&self, tool: &str, arguments: &mut Value) -> Result<()> {
        let object = arguments
            .as_object_mut()
            .context("reply arguments must be an object")?;
        object
            .entry("review")
            .or_insert_with(|| self.access.clone());
        // The thread tools take the access value alone.
        let fields = match tool {
            "submit_question" => object
                .entry("update")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .context("submit_question needs an update object")?,
            "submit_conclusion" => object,
            _ => return Ok(()),
        };
        self.fill_turn(fields);
        Ok(())
    }

    fn fill_turn(&self, fields: &mut Map<String, Value>) {
        fields
            .entry("instance")
            .or_insert_with(|| self.round.clone());
        fields
            .entry("request")
            .or_insert_with(|| self.request.clone());
        fields
            .entry("checkpoint")
            .or_insert_with(|| self.checkpoint.clone());
        if let Some(interpretation) = fields
            .get_mut("interpretation")
            .and_then(Value::as_object_mut)
        {
            interpretation
                .entry("answer")
                .or_insert_with(|| self.answer.clone());
        }
    }
}

#[cfg(test)]
#[path = "agent.tests.rs"]
mod tests;
