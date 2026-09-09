use std::path::Path;

use serde_json::{Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

use crate::Client;

const SERVER: &str = "herdr_reviewer";

pub(super) struct Configuration<'a> {
    client: Client,
    command: &'a str,
}

impl<'a> Configuration<'a> {
    pub(super) fn new(client: Client, bridge: &'a Path) -> Result<Self, String> {
        Ok(Self {
            client,
            command: bridge.to_str().ok_or("MCP executable path must be UTF-8")?,
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
                .is_some_and(Array::is_empty)
            && server.get("url").is_none()
        {
            return Ok(Self::forward_port(server)?.then(|| document.to_string()));
        }
        if !server.is_empty() {
            return Err(Self::conflict());
        }
        server.insert("command", value(self.command));
        server.insert("args", value(Array::new()));
        Self::forward_port(server)?;
        Ok(Some(document.to_string()))
    }

    fn forward_port(server: &mut dyn TableLike) -> Result<bool, String> {
        let variables = server
            .entry("env_vars")
            .or_insert(value(Array::new()))
            .as_array_mut()
            .ok_or("env_vars must be an array; configuration was left unchanged")?;
        if variables
            .iter()
            .any(|name| name.as_str() == Some("HERDR_REVIEWER_MCP_PORT"))
        {
            return Ok(false);
        }
        variables.push("HERDR_REVIEWER_MCP_PORT");
        Ok(true)
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
                && server.get("args") == Some(&json!([]))
                && server.get("type").and_then(Value::as_str) == Some("stdio")
                && server.get("url").is_none()
            {
                return Ok(None);
            }
            return Err(Self::conflict());
        }
        servers.insert(
            SERVER.into(),
            json!({"type": "stdio", "command": self.command, "args": []}),
        );
        Ok(Some(format!(
            "{}\n",
            serde_json::to_string_pretty(&document).expect("a JSON value serializes")
        )))
    }

    fn conflict() -> String {
        "An existing herdr_reviewer registration differs from automatic repository routing. It was left unchanged; update it manually or use an agent launch override for this workspace".into()
    }
}
