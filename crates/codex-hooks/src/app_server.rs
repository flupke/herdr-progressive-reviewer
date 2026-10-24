//! [`AppServer`]: a Codex app server of the user's Codex home, on standard input and output,
//! through which the installation reads and writes Codex's configuration as Codex's own
//! clients do.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use eyre::WrapErr as _;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// A `codex app-server` that this process talks to in JSON-RPC, one message per line. Dropping
/// it closes the server's input, which stops it.
pub(crate) struct AppServer {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl AppServer {
    /// Starts the app server of `codex`, and makes the handshake.
    pub(crate) fn start(codex: &str) -> eyre::Result<Self> {
        let mut child = Command::new(codex)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err_with(|| format!("cannot start `{codex} app-server`"))?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().expect("piped"));
        let mut server = Self {
            child,
            input,
            output,
            next: 0,
        };
        server.request::<Value>(
            "initialize",
            &json!({"clientInfo": {"name": "herdr-progressive-reviewer", "version": env!("CARGO_PKG_VERSION")}}),
        )?;
        server.send(&json!({"method": "initialized"}))?;
        Ok(server)
    }

    /// Calls `method` with `params`, and returns its result, once the server answers it.
    pub(crate) fn request<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &Value,
    ) -> eyre::Result<T> {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"id": id, "method": method, "params": params}))?;
        loop {
            let mut line = String::new();
            if self.output.read_line(&mut line)? == 0 {
                eyre::bail!("Codex's app server stopped before it answered {method}");
            }
            let message: Value = serde_json::from_str(&line)
                .wrap_err_with(|| format!("Codex's app server wrote {line:?}"))?;
            // Notifications and the server's own requests go by.
            if message.get("id").and_then(Value::as_u64) != Some(id)
                || message.get("method").is_some()
            {
                continue;
            }
            if let Some(error) = message.get("error") {
                eyre::bail!("Codex's app server refused {method}: {error}");
            }
            return serde_json::from_value(message.get("result").cloned().unwrap_or(Value::Null))
                .wrap_err_with(|| format!("Codex's app server answered {method} with {message}"));
        }
    }

    fn send(&mut self, message: &Value) -> eyre::Result<()> {
        let input = self.input.as_mut().expect("open until dropped");
        writeln!(input, "{message}")?;
        input.flush()?;
        Ok(())
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.wait();
    }
}
