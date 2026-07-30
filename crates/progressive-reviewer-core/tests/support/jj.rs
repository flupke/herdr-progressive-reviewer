use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    /// Ask jj for the canonical fixture root.
    pub fn jj_root(&self) -> PathBuf {
        let output = Command::new("jj")
            .args(["--color=never", "--no-pager", "root"])
            .current_dir(&self.root)
            .output()
            .unwrap();
        assert!(output.status.success(), "jj root must succeed");
        PathBuf::from(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .trim_end_matches(['\r', '\n']),
        )
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
}
