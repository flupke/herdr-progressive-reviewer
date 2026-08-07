//! Shared integration-test fixtures.

#![allow(
    clippy::missing_panics_doc,
    reason = "test fixture methods fail fast by design"
)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use review_repository::repository::{PollResult, RepoType, Repository, Snapshot};

/// The repository layout for a jj integration fixture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JjLayout {
    /// A repository whose Git storage is private to jj.
    NonColocated,
    /// A repository with `.jj` and `.git` at its root.
    Colocated,
}

/// Create a temporary repository for one backend.
pub fn repository_fixture(repository_type: RepoType) -> Box<dyn ReviewRepositoryFixture> {
    match repository_type {
        RepoType::Git => Box::new(GitFixture::new()),
        RepoType::Jj => Box::new(JjFixture::new(JjLayout::NonColocated)),
    }
}

/// Poll one stable snapshot from a synchronous test repository.
pub fn complete_repository_snapshot(repository: &Repository) -> Snapshot {
    match repository.poll().unwrap() {
        PollResult::Complete(snapshot) => snapshot,
        PollResult::ChangedDuringPoll => panic!("fixture changed during a synchronous poll"),
    }
}

/// Common repository operations used by backend-neutral integration tests.
pub trait ReviewRepositoryFixture {
    /// Get the temporary repository root.
    fn root(&self) -> &Path;
    /// Write one repository-relative file.
    fn write(&self, relative_path: &str, contents: &[u8]);
    /// Delete one repository-relative file.
    fn remove(&self, relative_path: &str);
    /// Rename one repository-relative file.
    fn rename(&self, from: &str, to: &str);
    /// Create one repository-relative symbolic link.
    fn symlink(&self, target: &str, link: &str);
    /// Get the revision that identifies the current review context.
    fn revision_id(&self) -> String;
    /// Start a new review context after the current worktree state.
    fn new_change(&self, description: &str);
    /// Return to an earlier review context.
    fn edit(&self, revision: &str);
    /// Rewrite repository history without changing the reviewed file.
    fn rewrite_base_without_reviewed_file_change(&self);
}

/// A temporary jj repository for integration tests.
#[derive(Debug)]
pub struct JjFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

/// A temporary Git repository for integration tests.
#[derive(Debug)]
pub struct GitFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

impl GitFixture {
    /// Create an empty Git repository with one base commit.
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            _directory: directory,
            root,
        };
        fixture.git(["init", "--quiet"]);
        fixture.git(["config", "user.name", "Reviewer Test"]);
        fixture.git(["config", "user.email", "reviewer@example.invalid"]);
        fixture.git(["commit", "--quiet", "--allow-empty", "-m", "base"]);
        fixture
    }

    /// Get the temporary repository root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Add all worktree changes and commit them.
    pub fn commit_all(&self, message: &str) {
        self.git(["add", "-A"]);
        self.git(["commit", "--quiet", "-m", message]);
    }

    /// Run a Git command in the fixture and require success.
    pub fn git<'a>(&self, arguments: impl IntoIterator<Item = &'a str>) -> Output {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git fixture command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

impl Default for GitFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewRepositoryFixture for GitFixture {
    fn root(&self) -> &Path {
        self.root()
    }

    fn write(&self, relative_path: &str, contents: &[u8]) {
        let path = self.root().join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn remove(&self, relative_path: &str) {
        fs::remove_file(self.root().join(relative_path)).unwrap();
    }

    fn rename(&self, from: &str, to: &str) {
        fs::rename(self.root().join(from), self.root().join(to)).unwrap();
    }

    fn symlink(&self, target: &str, link: &str) {
        std::os::unix::fs::symlink(target, self.root().join(link)).unwrap();
    }

    fn revision_id(&self) -> String {
        String::from_utf8(self.git(["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_owned()
    }

    fn new_change(&self, description: &str) {
        self.commit_all(description);
    }

    fn edit(&self, revision: &str) {
        self.git(["reset", "--quiet", "--mixed", revision]);
    }

    fn rewrite_base_without_reviewed_file_change(&self) {
        self.git(["commit", "--quiet", "--amend", "-m", "rewritten base"]);
    }
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

impl ReviewRepositoryFixture for JjFixture {
    fn root(&self) -> &Path {
        self.root()
    }

    fn write(&self, relative_path: &str, contents: &[u8]) {
        JjFixture::write(self, relative_path, contents);
    }

    fn remove(&self, relative_path: &str) {
        JjFixture::remove(self, relative_path);
    }

    fn rename(&self, from: &str, to: &str) {
        JjFixture::rename(self, from, to);
    }

    fn symlink(&self, target: &str, link: &str) {
        JjFixture::symlink(self, target, link);
    }

    fn revision_id(&self) -> String {
        self.change_id()
    }

    fn new_change(&self, description: &str) {
        JjFixture::new_change(self, description);
    }

    fn edit(&self, revision: &str) {
        JjFixture::edit(self, revision);
    }

    fn rewrite_base_without_reviewed_file_change(&self) {
        let reviewed_change = self.change_id();
        let old_parent = format!("{reviewed_change}-");
        self.jj(["new", &old_parent, "-m", "new parent"]);
        self.write("unrelated.txt", b"new parent\n");
        let new_parent = self.change_id();
        self.jj(["rebase", "-r", &reviewed_change, "-d", &new_parent]);
        self.edit(&reviewed_change);
    }
}
