use std::collections::{HashMap, HashSet};

/// A shared projection used by thread placement, badges and navigation.
#[derive(Default)]
pub struct ThreadPaths {
    current: HashSet<String>,
    renamed: HashMap<String, Option<String>>,
}

impl ThreadPaths {
    pub fn new(files: impl IntoIterator<Item = (String, Option<String>)>) -> Self {
        let mut paths = Self::default();
        for (current, old) in files {
            if let Some(old) = old.filter(|old| old != &current) {
                paths
                    .renamed
                    .entry(old)
                    .and_modify(|target| *target = None)
                    .or_insert_with(|| Some(current.clone()));
            }
            paths.current.insert(current);
        }
        paths
    }

    /// Prefer an exact path; follow a rename only when there is one destination.
    /// Ambiguous copies retain their original path and saved context.
    pub fn resolve<'a>(&'a self, original: &'a str) -> &'a str {
        if self.current.contains(original) {
            original
        } else {
            self.renamed
                .get(original)
                .and_then(Option::as_deref)
                .unwrap_or(original)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_keep_the_original_and_only_unambiguous_renames_move_threads() {
        let files = vec![
            ("copy".into(), Some("source".into())),
            ("source".into(), Some("source".into())),
        ];
        assert_eq!(ThreadPaths::new(files).resolve("source"), "source");
        assert_eq!(
            ThreadPaths::new([("copy".into(), Some("source".into()))]).resolve("source"),
            "copy"
        );
        assert_eq!(
            ThreadPaths::new([
                ("copy".into(), Some("source".into())),
                ("other".into(), Some("source".into()))
            ])
            .resolve("source"),
            "source"
        );
    }
}
