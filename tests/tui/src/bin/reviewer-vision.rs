use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use review_repository::repository::RepoType;
use reviewer_tui_tests::vision::{self, Options, Placement, SplitDirection};

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
        viewer: None,
        stop_after_idle: Some(Duration::from_secs(30 * 60)),
    };
    let mut placement = Placement::default();
    let mut headless = false;
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
            "--view" => {
                let socket = PathBuf::from(args.next().context("--view needs a stream socket")?);
                let own_pane = match args.next().as_deref() {
                    Some("--close-pane") => {
                        Some(args.next().context("--close-pane needs a pane ID")?)
                    }
                    None => None,
                    Some(other) => bail!("unknown --view option {other:?}"),
                };
                return vision::view(&socket, own_pane.as_deref());
            }
            "--viewer" => {
                let split = match args.next().as_deref() {
                    Some("right") => Some(SplitDirection::Right),
                    Some("down") => Some(SplitDirection::Down),
                    Some("none") => None,
                    _ => bail!("--viewer needs right, down or none"),
                };
                // The last --viewer wins.
                headless = split.is_none();
                placement.split = split;
            }
            "--viewer-ratio" => {
                let ratio: f64 = args
                    .next()
                    .context("--viewer-ratio needs a number")?
                    .parse()?;
                ensure!(
                    ratio > 0.0 && ratio < 1.0,
                    "--viewer-ratio must be between 0 and 1"
                );
                placement.ratio = Some(ratio);
            }
            "--stop-after-idle" => {
                let minutes: u64 = args
                    .next()
                    .context("--stop-after-idle needs a number of minutes")?
                    .parse()?;
                options.stop_after_idle =
                    (minutes > 0).then(|| Duration::from_secs(minutes.saturating_mul(60)));
            }
            "--commands" => {
                options.commands = Some(
                    std::env::current_dir()?
                        .join(args.next().context("--commands needs a named pipe path")?),
                );
            }
            "--help" | "-h" => {
                println!(
                    "reviewer-vision [--repo git|jj] [--output NEW_DIRECTORY] [--json] [--commands PIPE]\n                [--viewer right|down|none] [--viewer-ratio SHARE]\n                [--stop-after-idle MINUTES]\nreviewer-vision --view STREAM_SOCKET [--close-pane PANE_ID]\n\nSend one JSON command per line:\n  {{\"action\":\"observe\"}}\n  {{\"action\":\"press\",\"key\":\"?\"}}\n  {{\"action\":\"type\",\"text\":\"hello\"}}\n  {{\"action\":\"click\",\"x\":5,\"y\":2}}  or  {{\"action\":\"click\",\"text\":\"☐\"}}\n  {{\"action\":\"resize\",\"cols\":70,\"rows\":20}}\n  {{\"action\":\"cells\",\"x\":0,\"y\":0,\"width\":10,\"height\":1}}\n  {{\"action\":\"wait\",\"text\":\"Jev: marked\",\"timeout_ms\":5000}}\n  {{\"action\":\"screenshot\"}}\n  {{\"action\":\"jev\",\"path\":\"src/math.rs\",\"lines\":[2]}}\n  {{\"action\":\"note\",\"kind\":\"checked\",\"text\":\"Help closes with Escape\"}}\n  {{\"action\":\"reopen\"}}\n  {{\"action\":\"stop\"}}\n\nObserve accepts after (frame number) and timeout_ms (up to 30000).\nWait accepts timeout_ms (default 5000, up to 30000).\nNote kinds: checked, finding, untested. EOF or SIGINT also stops the session; with --commands, only stop or a signal does.\nThe viewer fits the driver's pane unless --viewer or --viewer-ratio says otherwise.\nThe session stops after --stop-after-idle minutes (default 30) without a command; 0 never stops."
                );
                return Ok(());
            }
            _ => bail!("unknown option {arg:?}; use --help"),
        }
    }
    options.viewer = (!headless).then_some(placement);
    vision::run(&options)
}
