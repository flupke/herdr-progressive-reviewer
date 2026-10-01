use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use review_repository::repository::RepoType;
use reviewer_tui_tests::vision::{self, Options};

fn main() -> Result<()> {
    let mut options = Options {
        repository_type: RepoType::Jj,
        directory: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/vision")
            .join(format!(
                "{}-{}",
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
                std::process::id()
            )),
        json: false,
        commands: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => {
                options.repository_type = args.next().context("--repo needs git or jj")?.parse()?;
            }
            "--output" => {
                options.directory = std::env::current_dir()?
                    .join(args.next().context("--output needs a new directory")?);
            }
            "--json" => options.json = true,
            "--commands" => {
                options.commands = Some(
                    std::env::current_dir()?
                        .join(args.next().context("--commands needs a named pipe path")?),
                );
            }
            "--help" | "-h" => {
                println!(
                    "reviewer-vision [--repo git|jj] [--output NEW_DIRECTORY] [--json] [--commands PIPE]\n\nSend one JSON command per line:\n  {{\"action\":\"observe\"}}\n  {{\"action\":\"press\",\"key\":\"?\"}}\n  {{\"action\":\"type\",\"text\":\"hello\"}}\n  {{\"action\":\"click\",\"x\":5,\"y\":2}}  or  {{\"action\":\"click\",\"text\":\"☐\"}}\n  {{\"action\":\"resize\",\"cols\":70,\"rows\":20}}\n  {{\"action\":\"cells\",\"x\":0,\"y\":0,\"width\":10,\"height\":1}}\n  {{\"action\":\"screenshot\"}}\n  {{\"action\":\"jev\",\"path\":\"src/math.rs\",\"lines\":[2]}}\n  {{\"action\":\"note\",\"kind\":\"checked\",\"text\":\"Help closes with Escape\"}}\n  {{\"action\":\"reopen\"}}\n  {{\"action\":\"stop\"}}\n\nObserve accepts after (frame number) and timeout_ms (up to 30000).\nNote kinds: checked, finding, untested. EOF or SIGINT also stops the session; with --commands, only stop or a signal does."
                );
                return Ok(());
            }
            _ => bail!("unknown option {arg:?}; use --help"),
        }
    }
    vision::run(&options)
}
