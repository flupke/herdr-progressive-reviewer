use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;

use super::Graph;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// `server` uses `core` and `serde`, and tests with `fixture`; `core` builds with `build-helper`.
fn graph() -> Graph {
    let package = |name: &str, local: bool| {
        json!({
            "name": name,
            "id": format!("{name}-id"),
            "source": if local { serde_json::Value::Null } else { json!("registry") },
            "manifest_path": format!("/repository/crates/{name}/Cargo.toml"),
        })
    };
    let dependency = |name: &str, kind: Option<&str>| json!({ "pkg": format!("{name}-id"), "dep_kinds": [{ "kind": kind }] });
    let metadata = json!({
        "packages": [
            package("server", true),
            package("core", true),
            package("fixture", true),
            package("serde", false),
            package("build-helper", false),
        ],
        "workspace_members": ["server-id", "core-id", "fixture-id"],
        "resolve": { "nodes": [
            { "id": "server-id", "deps": [
                dependency("core", None),
                dependency("serde", None),
                dependency("fixture", Some("dev")),
            ] },
            { "id": "core-id", "deps": [dependency("build-helper", Some("build"))] },
            { "id": "fixture-id", "deps": [dependency("core", None)] },
            { "id": "serde-id", "deps": [] },
            { "id": "build-helper-id", "deps": [] },
        ] },
    });
    Graph::new(Path::new("/repository"), &metadata).unwrap()
}

#[test]
fn local_packages_are_found_by_their_directory_and_members_by_their_id() {
    let graph = graph();

    assert_eq!(
        graph.packages.into_iter().collect::<Vec<_>>(),
        [
            ("crates/core".to_owned(), "core".to_owned()),
            ("crates/fixture".to_owned(), "fixture".to_owned()),
            ("crates/server".to_owned(), "server".to_owned()),
        ]
    );
    assert_eq!(graph.members, set(&["core", "fixture", "server"]));
}

#[test]
fn a_root_is_built_from_its_normal_and_build_dependencies_not_its_test_ones() {
    assert_eq!(
        graph().built_by("server").unwrap(),
        set(&["build-helper", "core", "serde", "server"])
    );
}

#[test]
fn every_kind_of_dependency_makes_a_dependent() {
    let dependents = graph().dependents();

    assert_eq!(dependents["core"], set(&["fixture", "server"]));
    assert_eq!(dependents["fixture"], set(&["server"]));
    assert_eq!(dependents["serde"], set(&["server"]));
}
