//! `make check-changed`: runs the checks that the files changed between two jj revisions reach,
//! and nothing else: by default from the change's parent to the working copy. `--dry-run` prints
//! them without running them, and `--to` plans for another revision than the working copy, with
//! `--dry-run` only, since the checks run on the working copy. `--check-map` fails when a
//! tracked file is in no check's map, so that a new directory is mapped before it is skipped.

mod lock;
mod plan;
mod workspace;

use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use eyre::{Context, Result};

use plan::Changes;
use workspace::Workspace;

/// The revisions compared when none are given: the working copy's change and its parent.
const DEFAULT_BASE: &str = "@-";
const DEFAULT_TO: &str = "@";

/// What the command was asked to do.
struct Options {
    base: String,
    to: String,
    dry_run: bool,
    check_map: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            base: DEFAULT_BASE.to_owned(),
            to: DEFAULT_TO.to_owned(),
            dry_run: false,
            check_map: false,
        };
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--base" => options.base = revision(&argument, arguments.next())?,
                "--to" => options.to = revision(&argument, arguments.next())?,
                "--dry-run" => options.dry_run = true,
                "--check-map" => options.check_map = true,
                other => {
                    eyre::bail!(
                        "unknown argument {other:?}; use --base <revision>, --to <revision>, \
                         --dry-run or --check-map"
                    );
                }
            }
        }
        if options.to != DEFAULT_TO && !options.dry_run {
            eyre::bail!("--to plans for another revision than the working copy: add --dry-run");
        }
        Ok(options)
    }
}

fn revision(argument: &str, value: Option<String>) -> Result<String> {
    value.ok_or_else(|| eyre::eyre!("{argument} needs a jj revision"))
}

fn main() -> Result<ExitCode> {
    let options = Options::parse()?;
    let root = repository_root()?;
    let workspace = Workspace::read(&root)?;
    if options.check_map {
        check_map(&root, &workspace)
    } else {
        run(&root, &workspace, &options)
    }
}

/// Fails when a tracked file is in no check's map.
fn check_map(root: &Path, workspace: &Workspace) -> Result<ExitCode> {
    let tracked = lines(&jj(root, &["file", "list"])?);
    let unmapped = workspace.unmapped(&tracked);
    if unmapped.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "These files are in no check's map: add them to a table in \
         crates/check-changed/src/plan.rs, or put them in a crate.",
    );
    for path in unmapped {
        eprintln!("  {path}");
    }
    Ok(ExitCode::FAILURE)
}

/// Runs the checks the changed files reach, the cheapest first, and stops at the first that
/// fails.
fn run(root: &Path, workspace: &Workspace, options: &Options) -> Result<ExitCode> {
    let changes = changes(root, &options.base, &options.to)?;
    if changes.paths.is_empty() {
        // A gate that checks nothing passes nothing: the change to check is not the working
        // copy's, or `--base` names the wrong revision.
        eprintln!(
            "check-changed: no file changed from {} to {}: run it with the change to check as \
             the working copy (`jj edit`), or set CHECK_BASE",
            options.base, options.to
        );
        return Ok(ExitCode::FAILURE);
    }
    let plan = workspace.plan(&changes);
    for reason in &plan.reasons {
        println!("check-changed: {reason}");
    }
    let make = env::var("MAKE").unwrap_or_else(|_| "make".into());
    for check in &plan.checks {
        let arguments = check.make_arguments();
        println!("check-changed: make {}", arguments.join(" "));
        if options.dry_run {
            continue;
        }
        let status = Command::new(&make)
            .args(&arguments)
            .current_dir(root)
            .status()
            .context("run make")?;
        if !status.success() {
            return Ok(ExitCode::FAILURE);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The files changed from `base` to `to`, both paths of a rename included, and the packages
/// whose entry in `Cargo.lock` changed.
fn changes(root: &Path, base: &str, to: &str) -> Result<Changes> {
    let summary = jj(root, &["diff", "--summary", "--from", base, "--to", to])?;
    let paths: Vec<String> = summary.lines().flat_map(summary_paths).collect();
    let locked = if paths.iter().any(|path| path == "Cargo.lock") {
        // A lock added or removed at either end cannot be compared: every check runs.
        let lock = |revision| jj(root, &["file", "show", "-r", revision, "Cargo.lock"]).ok();
        lock(base)
            .zip(lock(to))
            .and_then(|(before, after)| lock::changed_packages(&before, &after))
    } else {
        Some(BTreeSet::new())
    };
    Ok(Changes { paths, locked })
}

/// The paths of one line of `jj diff --summary`: `M path`, or both sides of a rename or a copy
/// written `R {old => new}/file` or `R old => new`.
fn summary_paths(line: &str) -> Vec<String> {
    let Some((_, path)) = line.split_once(' ') else {
        return Vec::new();
    };
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        let (prefix, suffix) = (&path[..open], &path[close + 1..]);
        if let Some((old, new)) = path[open + 1..close].split_once(" => ") {
            return [old, new]
                .iter()
                .map(|middle| format!("{prefix}{middle}{suffix}").replace("//", "/"))
                .collect();
        }
    }
    match path.split_once(" => ") {
        Some((old, new)) => vec![old.to_owned(), new.to_owned()],
        None => vec![path.to_owned()],
    }
}

fn repository_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        jj(&env::current_dir()?, &["workspace", "root"])?.trim(),
    ))
}

fn jj(directory: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("jj")
        .args(arguments)
        .current_dir(directory)
        .output()
        .context("run jj")?;
    eyre::ensure!(
        output.status.success(),
        "jj {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

#[cfg(test)]
#[path = "main.tests.rs"]
mod tests;
