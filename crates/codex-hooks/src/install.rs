//! Installs the plugin in the user's Codex home, and removes it, as `make install` and
//! `make uninstall` do.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use eyre::WrapErr as _;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::app_server::AppServer;

/// The program of Codex's CLI.
const CODEX: &str = "codex";

/// The marketplace that lists the plugin.
const MARKETPLACE: &str = "herdr-progressive-reviewer";

/// The plugin, as Codex names it.
const PLUGIN: &str = "progressive-reviewer@herdr-progressive-reviewer";

/// The command of the plugin's hook in `plugin/hooks/hooks.json`, which the installation
/// replaces with one that names the checkout's `reviewer-control`.
const COMMAND: &str = "reviewer-control codex-hook";

/// The files of the marketplace and of its plugin, by their paths in the marketplace.
const FILES: [(&str, &str); 3] = [
    (
        ".agents/plugins/marketplace.json",
        include_str!("../.agents/plugins/marketplace.json"),
    ),
    (
        "plugin/.codex-plugin/plugin.json",
        include_str!("../plugin/.codex-plugin/plugin.json"),
    ),
    (
        "plugin/hooks/hooks.json",
        include_str!("../plugin/hooks/hooks.json"),
    ),
];

/// Installs the plugin, whose hook runs `control`, in the Codex home of `codex`'s environment
/// (`CODEX_HOME`, or `~/.codex`), and trusts its hook. The marketplace is written beside
/// `control`, where Codex finds it again. Run again, from another checkout too, it installs the
/// plugin of `control` in place of the earlier one.
pub fn install(control: &Path) -> eyre::Result<()> {
    let marketplace = marketplace_of(control);
    write_marketplace(&marketplace, control)?;
    let source = marketplace.to_string_lossy();
    // Codex refuses a marketplace of the same name from another source, such as an earlier
    // checkout's.
    if !succeeds(&["plugin", "marketplace", "add", &source]) {
        codex(&["plugin", "marketplace", "remove", MARKETPLACE])?;
        codex(&["plugin", "marketplace", "add", &source])?;
    }
    codex(&["plugin", "add", PLUGIN])?;
    let trusted = trust()?;
    eprintln!("Codex: trusted {trusted} hook(s) of {PLUGIN}");
    Ok(())
}

/// Removes the trust of the plugin's hooks, the plugin and its marketplace from the Codex home
/// of `codex`'s environment, and the marketplace written beside `control`.
pub fn uninstall(control: &Path) -> eyre::Result<()> {
    let removed = untrust()?;
    eprintln!("Codex: removed the trust of {removed} hook(s) of {PLUGIN}");
    codex(&["plugin", "remove", PLUGIN])?;
    codex(&["plugin", "marketplace", "remove", MARKETPLACE])?;
    match fs::remove_dir_all(marketplace_of(control)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Where the marketplace of the plugin whose hook runs `control` is written.
fn marketplace_of(control: &Path) -> std::path::PathBuf {
    control.with_file_name("codex-plugin")
}

/// Writes the marketplace in `directory`, with the plugin whose hook runs `control`.
fn write_marketplace(directory: &Path, control: &Path) -> eyre::Result<()> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let command = serde_json::to_string(&hook_command(control))?;
    for (path, text) in FILES {
        let path = directory.join(path);
        fs::create_dir_all(path.parent().expect("a file of the marketplace"))?;
        let text = text.replace(&serde_json::to_string(COMMAND)?, &command);
        fs::write(&path, text).wrap_err_with(|| format!("cannot write {}", path.display()))?;
    }
    Ok(())
}

/// The command of the hook that runs `control`: Codex runs it with the user's shell, as
/// `$SHELL -lc <command>`.
fn hook_command(control: &Path) -> String {
    let control = control.to_string_lossy().replace('\'', r"'\''");
    format!("'{control}' {}", crate::SUBCOMMAND)
}

/// Runs `codex` with `arguments`, its output the user's.
fn codex(arguments: &[&str]) -> eyre::Result<()> {
    let status = Command::new(CODEX)
        .args(arguments)
        .status()
        .wrap_err_with(|| format!("cannot run `{CODEX}`"))?;
    eyre::ensure!(
        status.success(),
        "`{CODEX} {}` failed: {status}",
        arguments.join(" ")
    );
    Ok(())
}

/// Whether `codex` with `arguments` succeeds; its output is shown only then.
fn succeeds(arguments: &[&str]) -> bool {
    Command::new(CODEX)
        .args(arguments)
        .output()
        .is_ok_and(|output| {
            if output.status.success() {
                eprint!("{}", String::from_utf8_lossy(&output.stdout));
            }
            output.status.success()
        })
}

/// The answer to `hooks/list`.
#[derive(Deserialize)]
struct HooksList {
    data: Vec<HooksOfDirectory>,
}

#[derive(Deserialize)]
struct HooksOfDirectory {
    hooks: Vec<ListedHook>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedHook {
    key: String,
    current_hash: String,
    plugin_id: Option<String>,
}

/// Trusts each hook of the plugin, as Codex's `/hooks` does: Codex runs a hook whose hash its
/// configuration's `hooks.state` holds. Returns how many it trusted.
fn trust() -> eyre::Result<usize> {
    let mut server = AppServer::start(CODEX)?;
    let listed: HooksList = server.request("hooks/list", &json!({}))?;
    let state: Map<String, Value> = listed
        .data
        .into_iter()
        .flat_map(|directory| directory.hooks)
        .filter(|hook| hook.plugin_id.as_deref() == Some(PLUGIN))
        .map(|hook| (hook.key, json!({"trusted_hash": hook.current_hash})))
        .collect();
    eyre::ensure!(!state.is_empty(), "Codex lists no hook of {PLUGIN}");
    server.request::<Value>(
        "config/batchWrite",
        &json!({"edits": [{"keyPath": "hooks.state", "mergeStrategy": "upsert", "value": state}]}),
    )?;
    Ok(state.len())
}

/// The answer to `config/read` with its layers.
#[derive(Deserialize)]
struct ConfigRead {
    layers: Vec<ConfigLayer>,
}

#[derive(Deserialize)]
struct ConfigLayer {
    name: LayerName,
    version: String,
    config: Value,
}

#[derive(Deserialize)]
struct LayerName {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

/// Removes the trust of each hook of the plugin from the user's configuration: Codex has no
/// edit that removes a key, so the user's `hooks.state` is written again without them. Returns
/// how many it removed.
fn untrust() -> eyre::Result<usize> {
    let mut server = AppServer::start(CODEX)?;
    let read: ConfigRead = server.request("config/read", &json!({"includeLayers": true}))?;
    let Some(user) = read
        .layers
        .into_iter()
        .find(|layer| layer.name.kind == "user" && layer.name.profile.is_none())
    else {
        return Ok(0);
    };
    let state = user
        .config
        .pointer("/hooks/state")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let (removed, kept) = without_plugin(state);
    if removed > 0 {
        server.request::<Value>(
            "config/batchWrite",
            &json!({
                "edits": [{"keyPath": "hooks.state", "mergeStrategy": "replace", "value": kept}],
                "filePath": user.name.file,
                "expectedVersion": user.version,
            }),
        )?;
    }
    Ok(removed)
}

/// `state`, a configuration's `hooks.state`, without the entries of the plugin's hooks, and how
/// many those were. A hook's key starts with the plugin's ID.
fn without_plugin(state: Map<String, Value>) -> (usize, Map<String, Value>) {
    let prefix = format!("{PLUGIN}:");
    let before = state.len();
    let kept: Map<String, Value> = state
        .into_iter()
        .filter(|(key, _)| !key.starts_with(&prefix))
        .collect();
    (before - kept.len(), kept)
}

#[cfg(test)]
#[path = "install.tests.rs"]
mod tests;
