//! Shared integration-test fixtures.

#![allow(
    clippy::missing_panics_doc,
    reason = "test fixture methods fail fast by design"
)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository layout for a jj integration fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JjLayout {
    /// A repository whose Git storage is private to jj.
    NonColocated,
    /// A repository with `.jj` and `.git` at its root.
    Colocated,
}

/// A temporary jj repository for integration tests.
#[derive(Debug)]
pub struct JjFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

impl JjFixture {
    /// Create an empty repository with the selected layout.
    pub fn new(layout: JjLayout) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        let layout_argument = match layout {
            JjLayout::NonColocated => "--no-colocate",
            JjLayout::Colocated => "--colocate",
        };
        let output = Command::new("jj")
            .args([
                OsString::from("--color=never"),
                OsString::from("--no-pager"),
                OsString::from("git"),
                OsString::from("init"),
                OsString::from(layout_argument),
                root.as_os_str().to_owned(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj fixture initialization must succeed"
        );

        Self {
            _directory: directory,
            root,
        }
    }

    /// Get the temporary repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Write one repository-relative fixture file.
    pub fn write(&self, relative_path: impl AsRef<Path>, contents: &[u8]) {
        let relative_path = relative_path.as_ref();
        assert!(
            !relative_path.is_absolute()
                && relative_path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_))),
            "fixture path must be a safe relative path"
        );
        let path = self.root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// Delete one repository-relative fixture file.
    pub fn remove(&self, relative_path: impl AsRef<Path>) {
        fs::remove_file(self.root.join(relative_path)).unwrap();
    }

    /// Rename one repository-relative fixture file.
    pub fn rename(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) {
        fs::rename(self.root.join(from), self.root.join(to)).unwrap();
    }

    /// Create one repository-relative symbolic link.
    pub fn symlink(&self, target: impl AsRef<Path>, link: impl AsRef<Path>) {
        std::os::unix::fs::symlink(target, self.root.join(link)).unwrap();
    }

    /// Start a new jj change with the current change as its parent.
    pub fn new_change(&self, description: &str) {
        self.jj(["new", "-m", description]);
    }

    /// Edit an existing jj change.
    pub fn edit(&self, revision: &str) {
        self.jj(["edit", revision]);
    }

    /// Get the full stable change ID for the working copy.
    pub fn change_id(&self) -> String {
        let output = self.jj(["log", "--no-graph", "-r", "@", "-T", r#"change_id ++ "\n""#]);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    /// Run a jj command in the fixture and require success.
    pub fn jj<'a>(&self, arguments: impl IntoIterator<Item = &'a str>) -> Output {
        let output = Command::new("jj")
            .args(["--color=never", "--no-pager"])
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj fixture command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}
