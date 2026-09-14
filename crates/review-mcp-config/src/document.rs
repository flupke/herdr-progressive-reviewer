use std::path::Path;

use review_mcp::Endpoint;
use serde_json::{Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::Client;

const SERVER: &str = "herdr_reviewer";

pub(super) struct Configuration<'a> {
    client: Client,
    url: String,
    command: &'a str,
    port: String,
}

impl<'a> Configuration<'a> {
    pub(super) fn new(
        client: Client,
        endpoint: Endpoint,
        bridge: &'a Path,
    ) -> Result<Self, String> {
        Ok(Self {
            client,
            url: endpoint.url(),
            command: bridge.to_str().ok_or("MCP executable path must be UTF-8")?,
            port: endpoint.address().port().to_string(),
        })
    }

    pub(super) fn insert(&self, original: &str) -> Result<Option<String>, String> {
        match self.client {
            Client::Codex => self.codex(original),
            Client::Claude => self.claude(original),
        }
    }

    fn codex(&self, original: &str) -> Result<Option<String>, String> {
        let mut document = original
            .parse::<DocumentMut>()
            .map_err(|_| "Invalid TOML; configuration was left unchanged")?;
        let mut table = Table::new();
        table.set_implicit(true);
        let servers = document
            .entry("mcp_servers")
            .or_insert(Item::Table(table))
            .as_table_like_mut()
            .ok_or("mcp_servers must be a table; configuration was left unchanged")?;
        let server = servers
            .entry(SERVER)
            .or_insert(Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or("herdr_reviewer must be a table")?;
        if server.get("command").and_then(Item::as_str) == Some(self.command)
            && server
                .get("args")
                .and_then(Item::as_array)
                .is_some_and(|args| {
                    args.len() == 1
                        && args.get(0).and_then(toml_edit::Value::as_str)
                            == Some(self.port.as_str())
                })
            && server.get("url").is_none()
        {
            return Ok(None);
        }
        if !server.is_empty() {
            self.owned_http(
                server.get("url").and_then(Item::as_str),
                server.iter().map(|(key, _)| key),
                &[
                    "url",
                    "enabled",
                    "required",
                    "startup_timeout_sec",
                    "tool_timeout_sec",
                    "enabled_tools",
                    "disabled_tools",
                    "default_tools_approval_mode",
                    "tools",
                ],
            )?;
        }
        server.remove("url");
        server.insert("command", value(self.command));
        server.insert("args", value(Array::from_iter([self.port.as_str()])));
        Ok(Some(document.to_string()))
    }

    fn claude(&self, original: &str) -> Result<Option<String>, String> {
        let mut document: Value = if original.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(original)
                .map_err(|_| "Invalid JSON; configuration was left unchanged")?
        };
        let servers = document
            .as_object_mut()
            .ok_or("Configuration must be a JSON object; it was left unchanged")?
            .entry("mcpServers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("mcpServers must be an object; configuration was left unchanged")?;
        if let Some(server) = servers.get(SERVER) {
            if server.get("command").and_then(Value::as_str) == Some(self.command)
                && server.get("args") == Some(&json!([self.port]))
                && server.get("type").and_then(Value::as_str) == Some("stdio")
                && server.get("url").is_none()
            {
                return Ok(None);
            }
            let object = server
                .as_object()
                .ok_or("herdr_reviewer must be an object")?;
            self.owned_http(
                server.get("url").and_then(Value::as_str),
                object.keys().map(String::as_str),
                &["url", "type"],
            )?;
            if server.get("type").and_then(Value::as_str) != Some("http") {
                return Err(self.conflict());
            }
        }
        servers.insert(
            SERVER.into(),
            json!({"type": "stdio", "command": self.command, "args": [self.port]}),
        );
        Ok(Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&document).expect("a JSON value serializes")
        )))
    }

    fn owned_http<'b>(
        &self,
        url: Option<&str>,
        keys: impl Iterator<Item = &'b str>,
        allowed: &[&str],
    ) -> Result<(), String> {
        if url == Some(self.url.as_str()) && keys.into_iter().all(|key| allowed.contains(&key)) {
            Ok(())
        } else {
            Err(self.conflict())
        }
    }

    fn conflict(&self) -> String {
        format!(
            "An existing herdr_reviewer registration differs from {}. It was left unchanged; update it manually or use an agent launch override for this workspace",
            self.url
        )
    }
}
