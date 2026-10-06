use std::collections::BTreeSet;

use super::changed_packages;

const BEFORE: &str = r#"version = 4

[[package]]
name = "base64"
version = "0.22.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaa"

[[package]]
name = "crossterm"
version = "0.29.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bbbb"
dependencies = [
 "bitflags",
]

[[package]]
name = "reviewer"
version = "0.2.0"
dependencies = [
 "crossterm",
]
"#;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_feature_that_adds_a_dependency_changes_the_package_that_gains_it_and_the_new_one() {
    let after = BEFORE.replace(
        "dependencies = [\n \"bitflags\",\n]",
        "dependencies = [\n \"base64\",\n \"bitflags\",\n]",
    ) + "\n[[package]]\nname = \"bitflags\"\nversion = \"2.0.0\"\nchecksum = \"cccc\"\n";

    assert_eq!(
        changed_packages(BEFORE, &after),
        Some(set(&["bitflags", "crossterm"]))
    );
}

#[test]
fn a_version_bump_changes_the_package_and_the_ones_that_name_it() {
    let after = BEFORE.replace("version = \"0.22.1\"", "version = \"0.22.2\"");

    assert_eq!(changed_packages(BEFORE, &after), Some(set(&["base64"])));
    assert_eq!(changed_packages(BEFORE, BEFORE), Some(set(&[])));
}

#[test]
fn a_lock_without_entries_cannot_be_compared() {
    assert_eq!(changed_packages(BEFORE, "version = 4\n"), None);
}
