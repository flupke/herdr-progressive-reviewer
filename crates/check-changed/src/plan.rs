//! Which checks the changed files reach. Cargo's dependency graph says which crates a change
//! reaches, and which of them each end-to-end suite builds; the tables below say what the files
//! outside the crates belong to. A file the tables do not name runs every check, and
//! `--check-map` fails on it, so a new directory is never left unchecked.

use std::collections::BTreeSet;
use std::path::Path;

use crate::workspace::Workspace;

/// A check, as `make` runs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Check {
    /// `make lint`: clippy, the client's types and the complexity gate, a few seconds.
    Lint,
    /// `make test` for these crates only, with nextest.
    UnitTests(BTreeSet<String>),
    /// `make test` for the whole workspace, its doc tests included.
    AllUnitTests,
    /// An end-to-end suite.
    EndToEnd(Suite),
}

impl Check {
    /// The arguments of `make` for this check.
    pub(crate) fn make_arguments(&self) -> Vec<String> {
        match self {
            Self::Lint => vec!["lint".into()],
            Self::AllUnitTests => vec!["test".into()],
            Self::UnitTests(crates) => vec![
                "test".into(),
                format!(
                    "CRATES={}",
                    crates.iter().cloned().collect::<Vec<_>>().join(" ")
                ),
            ],
            Self::EndToEnd(suite) => vec![suite.make_target().into()],
        }
    }
}

/// An end-to-end suite.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Suite {
    /// The pane, driven in a private Herdr.
    Pane,
    /// The Explore page, in a browser.
    Page,
}

impl Suite {
    const ALL: [Self; 2] = [Self::Pane, Self::Page];

    fn make_target(self) -> &'static str {
        match self {
            Self::Pane => "e2e-tui",
            Self::Page => "e2e-explore",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Pane => "the pane's e2e tests",
            Self::Page => "the Explore page's e2e tests",
        }
    }
}

/// What a changed file belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Owner {
    /// A file of this local package, sources and assets alike.
    Package(String),
    /// A file of an end-to-end suite's harness.
    Suite(Suite),
    /// A file no check reads: documentation, agents' notes, tooling the checks do not run.
    Nothing,
    /// `Cargo.lock`: the packages whose entry changed say what it reaches.
    Lock,
    /// A file every check reads: the workspace's manifest, the toolchain, the Makefile.
    Everything,
    /// A file the tables do not name.
    Unmapped,
}

/// Files that every check reads. `Cargo.lock` is not one: its changed packages say which crates
/// it reaches.
const EVERYTHING: &[&str] = &[
    "Cargo.toml",
    "flake.nix",
    "flake.lock",
    "Makefile",
    ".config/",
];

/// The harnesses of the end-to-end suites.
const SUITES: &[(&str, Suite)] = &[
    ("tests/tui/", Suite::Pane),
    ("tests/explore-page/", Suite::Page),
];

/// Files that no check reads. Markdown outside the crates is one of them too: inside a crate, a
/// Markdown file may be compiled into it. `scripts/dev-shell` enters the shell the checks run in,
/// but no check runs it; `.envrc` and `herdr-plugin.toml` serve direnv and Herdr's install.
const NOTHING: &[&str] = &[
    "docs/",
    ".agents/",
    ".claude/",
    "scripts/",
    "LICENSE",
    ".gitignore",
    ".envrc",
    "herdr-plugin.toml",
];

/// What changed between two revisions.
#[derive(Debug, Default)]
pub(crate) struct Changes {
    /// The changed files, relative to the repository: both paths of a rename.
    pub(crate) paths: Vec<String>,
    /// The packages whose entry in `Cargo.lock` changed, when it did; `None` when `Cargo.lock`
    /// changed in a way these could not be read from.
    pub(crate) locked: Option<BTreeSet<String>>,
}

/// The checks a change reaches, and why.
#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct Plan {
    pub(crate) checks: Vec<Check>,
    pub(crate) reasons: Vec<String>,
}

/// The changed files, sorted by what they belong to.
#[derive(Default)]
struct Owners {
    /// The changed packages: local ones by their files, any one by its entry in `Cargo.lock`.
    packages: BTreeSet<String>,
    suites: BTreeSet<Suite>,
    /// Why every check runs, when it does.
    run_everything: Vec<String>,
}

impl Owners {
    fn of(workspace: &Workspace, changes: &Changes) -> Self {
        let mut owners = Self::default();
        for path in &changes.paths {
            match workspace.owner(path) {
                Owner::Package(name) => {
                    owners.packages.insert(name);
                }
                Owner::Suite(suite) => {
                    owners.suites.insert(suite);
                }
                Owner::Lock => match &changes.locked {
                    Some(packages) => owners.packages.extend(packages.iter().cloned()),
                    None => owners
                        .run_everything
                        .push("Cargo.lock changed in a way check-changed cannot read".into()),
                },
                Owner::Nothing => {}
                Owner::Everything => owners
                    .run_everything
                    .push(format!("{path} reaches every check")),
                Owner::Unmapped => owners
                    .run_everything
                    .push(format!("{path} is in no check's map")),
            }
        }
        owners
    }
}

impl Workspace {
    /// What `path`, relative to the repository, belongs to.
    fn owner(&self, path: &str) -> Owner {
        if path == "Cargo.lock" {
            return Owner::Lock;
        }
        if EVERYTHING.iter().any(|rule| matches_rule(path, rule)) {
            return Owner::Everything;
        }
        let in_crates = path.starts_with("crates/") || path.starts_with("vendor/");
        let markdown = Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        if markdown && !in_crates {
            return Owner::Nothing;
        }
        if let Some((_, suite)) = SUITES.iter().find(|(rule, _)| matches_rule(path, rule)) {
            return Owner::Suite(*suite);
        }
        let package = self
            .packages
            .iter()
            .filter(|(directory, _)| path.starts_with(&format!("{directory}/")))
            .max_by_key(|(directory, _)| directory.len());
        if let Some((_, name)) = package {
            return Owner::Package(name.clone());
        }
        if NOTHING.iter().any(|rule| matches_rule(path, rule)) {
            return Owner::Nothing;
        }
        Owner::Unmapped
    }

    /// The checks that `changes` reach.
    pub(crate) fn plan(&self, changes: &Changes) -> Plan {
        let owners = Owners::of(self, changes);
        if !owners.run_everything.is_empty() {
            let mut checks = vec![Check::Lint, Check::AllUnitTests];
            checks.extend(Suite::ALL.map(Check::EndToEnd));
            return Plan {
                checks,
                reasons: owners.run_everything,
            };
        }
        let reached = self.reached(&owners.packages);
        let tested: BTreeSet<_> = reached.intersection(&self.members).cloned().collect();
        let mut plan = Plan::default();
        if !tested.is_empty() {
            plan.reasons.push(format!(
                "changed packages: {}",
                owners
                    .packages
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            plan.checks.push(Check::UnitTests(tested));
        }
        for suite in Suite::ALL {
            let built = self.built.get(&suite);
            if owners.suites.contains(&suite) {
                plan.reasons
                    .push(format!("{}: their harness changed", suite.name()));
                plan.checks.push(Check::EndToEnd(suite));
            } else if built.is_some_and(|built| !reached.is_disjoint(built)) {
                plan.reasons
                    .push(format!("{}: they build a changed crate", suite.name()));
                plan.checks.push(Check::EndToEnd(suite));
            }
        }
        if plan.checks.is_empty() {
            plan.reasons
                .push("no changed file is read by a check".into());
        } else {
            plan.checks.insert(0, Check::Lint);
        }
        plan
    }

    /// The tracked files that the tables do not name.
    pub(crate) fn unmapped<'a>(&self, tracked: &'a [String]) -> Vec<&'a str> {
        tracked
            .iter()
            .filter(|path| self.owner(path) == Owner::Unmapped)
            .map(String::as_str)
            .collect()
    }

    /// `packages` and every package that depends on one of them, transitively.
    fn reached(&self, packages: &BTreeSet<String>) -> BTreeSet<String> {
        let mut reached = packages.clone();
        let mut pending: Vec<String> = packages.iter().cloned().collect();
        while let Some(package) = pending.pop() {
            for dependent in self.dependents.get(&package).into_iter().flatten() {
                if reached.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
        reached
    }
}

/// Whether `path` is the file `rule` names, or is under the directory `rule` names with a
/// trailing slash.
fn matches_rule(path: &str, rule: &str) -> bool {
    if rule.ends_with('/') {
        path.starts_with(rule)
    } else {
        path == rule
    }
}

#[cfg(test)]
#[path = "plan.tests.rs"]
mod tests;
