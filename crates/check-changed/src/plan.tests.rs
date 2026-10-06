use std::collections::{BTreeMap, BTreeSet};

use super::{Changes, Check, Plan, Suite};
use crate::workspace::Workspace;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// `core` is used by `ui` and `server`; `ui` by `app`, which the pane runs; `server` is the
/// page's server; `markdown` is vendored, outside the members; `serde`, from the registry, is
/// used by `core`.
fn workspace() -> Workspace {
    Workspace {
        packages: BTreeMap::from([
            ("crates/core".into(), "core".into()),
            ("crates/ui".into(), "ui".into()),
            ("crates/app".into(), "app".into()),
            ("crates/server".into(), "server".into()),
            ("crates/server/nested".into(), "nested".into()),
            ("vendor/markdown".into(), "markdown".into()),
        ]),
        members: set(&["core", "ui", "app", "server", "nested"]),
        dependents: BTreeMap::from([
            ("core".into(), set(&["ui", "server"])),
            ("ui".into(), set(&["app"])),
            ("markdown".into(), set(&["ui"])),
            ("serde".into(), set(&["core"])),
        ]),
        built: BTreeMap::from([
            (
                Suite::Pane,
                set(&["app", "ui", "core", "markdown", "serde"]),
            ),
            (Suite::Page, set(&["server", "core", "serde"])),
        ]),
    }
}

fn changes(paths: &[&str]) -> Changes {
    Changes {
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        locked: Some(BTreeSet::new()),
    }
}

fn checks(paths: &[&str]) -> Vec<Check> {
    workspace().plan(&changes(paths)).checks
}

fn every_check() -> Vec<Check> {
    vec![
        Check::Lint,
        Check::AllUnitTests,
        Check::AllIntegrationTests,
        Check::EndToEnd(Suite::Pane),
        Check::EndToEnd(Suite::Page),
    ]
}

#[test]
fn a_crate_change_tests_it_and_its_dependents_and_the_suites_that_build_one_of_them() {
    assert_eq!(
        checks(&["crates/ui/src/lib.rs"]),
        [
            Check::Lint,
            Check::UnitTests(set(&["app", "ui"])),
            Check::IntegrationTests(set(&["app", "ui"])),
            Check::EndToEnd(Suite::Pane),
        ]
    );
    assert_eq!(
        checks(&["crates/core/src/lib.rs"]),
        [
            Check::Lint,
            Check::UnitTests(set(&["app", "core", "server", "ui"])),
            Check::IntegrationTests(set(&["app", "core", "server", "ui"])),
            Check::EndToEnd(Suite::Pane),
            Check::EndToEnd(Suite::Page),
        ]
    );
}

#[test]
fn a_file_belongs_to_the_innermost_package_whatever_its_kind() {
    // A client asset or a prompt in Markdown is the crate's, as its sources are.
    assert_eq!(
        checks(&["crates/server/assets/client/page.js"]),
        [
            Check::Lint,
            Check::UnitTests(set(&["server"])),
            Check::IntegrationTests(set(&["server"])),
            Check::EndToEnd(Suite::Page),
        ]
    );
    assert_eq!(
        checks(&["crates/server/nested/prompts/rules.md"]),
        [
            Check::Lint,
            Check::UnitTests(set(&["nested"])),
            Check::IntegrationTests(set(&["nested"])),
        ]
    );
}

#[test]
fn a_vendored_change_tests_the_members_that_use_it() {
    assert_eq!(
        checks(&["vendor/markdown/src/lib.rs"]),
        [
            Check::Lint,
            Check::UnitTests(set(&["app", "ui"])),
            Check::IntegrationTests(set(&["app", "ui"])),
            Check::EndToEnd(Suite::Pane),
        ]
    );
}

#[test]
fn a_locked_package_that_changed_reaches_the_crates_that_use_it() {
    let change = Changes {
        paths: vec!["Cargo.lock".into(), "crates/app/Cargo.toml".into()],
        locked: Some(set(&["serde"])),
    };

    assert_eq!(
        workspace().plan(&change).checks,
        [
            Check::Lint,
            Check::UnitTests(set(&["app", "core", "server", "ui"])),
            Check::IntegrationTests(set(&["app", "core", "server", "ui"])),
            Check::EndToEnd(Suite::Pane),
            Check::EndToEnd(Suite::Page),
        ]
    );
    let unreadable = Changes {
        paths: vec!["Cargo.lock".into()],
        locked: None,
    };
    assert_eq!(workspace().plan(&unreadable).checks, every_check());
}

#[test]
fn a_harness_change_runs_its_own_suite_only() {
    assert_eq!(
        checks(&["tests/explore-page/tests/start.e2e.ts"]),
        [Check::Lint, Check::EndToEnd(Suite::Page)]
    );
    assert_eq!(
        checks(&["tests/tui/src/lib.rs"]),
        [Check::Lint, Check::EndToEnd(Suite::Pane)]
    );
}

#[test]
fn documentation_and_tooling_no_check_runs_reach_no_check() {
    assert_eq!(
        checks(&[
            "AGENTS.md",
            ".agents/wiki/checks.md",
            "docs/design/explore-page/README.md",
            "tests/tui/tests/README.md",
            "herdr-plugin.toml",
            ".envrc",
        ]),
        []
    );
}

#[test]
fn a_manifest_or_an_unmapped_file_runs_every_check() {
    for path in [
        "Cargo.toml",
        "flake.nix",
        ".config/nextest.toml",
        "tools/new.sh",
    ] {
        let Plan { checks, reasons } = workspace().plan(&changes(&[path]));

        assert_eq!(checks, every_check(), "{path}");
        assert!(
            reasons.iter().any(|reason| reason.contains(path)),
            "{reasons:?}"
        );
    }
}

#[test]
fn the_map_check_names_the_tracked_files_no_table_maps() {
    let tracked = [
        "crates/ui/src/lib.rs".to_owned(),
        "tools/new.sh".to_owned(),
        "scripts/dev-shell".to_owned(),
        "README.md".to_owned(),
    ];

    assert_eq!(workspace().unmapped(&tracked), ["tools/new.sh"]);
}
