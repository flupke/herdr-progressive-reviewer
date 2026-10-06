//! The packages whose entry in `Cargo.lock` changed: a version, a checksum, or the
//! dependencies of a package, added, removed or moved. Each is a package whose dependents the
//! change reaches.

use std::collections::{BTreeMap, BTreeSet};

/// The names of the packages whose entry differs between two versions of `Cargo.lock`, or
/// `None` when either has no package entry to compare.
pub(crate) fn changed_packages(before: &str, after: &str) -> Option<BTreeSet<String>> {
    let before = entries(before)?;
    let after = entries(after)?;
    let changed = before
        .iter()
        .filter(|(key, entry)| after.get(*key) != Some(entry))
        .chain(
            after
                .iter()
                .filter(|(key, entry)| before.get(*key) != Some(entry)),
        )
        .map(|((name, _), _)| name.clone())
        .collect();
    Some(changed)
}

/// Each `[[package]]` entry, by its name and version.
fn entries(lock: &str) -> Option<BTreeMap<(String, String), &str>> {
    let entries: BTreeMap<_, _> = lock
        .split("[[package]]")
        .skip(1)
        .filter_map(|entry| {
            Some((
                (field(entry, "name")?, field(entry, "version")?),
                entry.trim(),
            ))
        })
        .collect();
    (!entries.is_empty()).then_some(entries)
}

/// The value of a `key = "value"` line of an entry.
fn field(entry: &str, key: &str) -> Option<String> {
    entry.lines().find_map(|line| {
        let value = line
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?
            .trim();
        Some(value.trim_matches('"').to_owned())
    })
}

#[cfg(test)]
#[path = "lock.tests.rs"]
mod tests;
