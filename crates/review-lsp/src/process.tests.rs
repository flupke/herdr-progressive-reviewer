use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::language::LanguageServer;

use super::*;

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    directory: tempfile::TempDir,
    launcher: ServerLauncher,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project with spaces");
        fs::create_dir(&root).unwrap();
        executable(
            &directory.path().join("expert"),
            "#!/bin/sh\nprintf '%s|%s|%s' \"$PWD\" \"$1\" \"${PROJECT_ENV-unset}\"\n",
        );
        let mut launcher = ServerLauncher::new(&Project {
            root,
            server: LanguageServer::Expert,
        });
        for command in [&mut launcher.direnv, &mut launcher.direct] {
            command.env_clear().env("PATH", directory.path());
        }
        Self {
            directory,
            launcher,
        }
    }

    fn output(&mut self) -> (bool, String, String, bool) {
        let mut process = self.launcher.spawn().unwrap();
        let mut stdout = String::new();
        process
            .child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let stderr = StderrOutput::read_tail(process.child.stderr.take().unwrap());
        let success = process.child.wait().unwrap().success();
        (process.uses_direnv, stdout, stderr, success)
    }
}

#[test]
fn missing_direnv_launches_the_server_directly() {
    let mut fixture = Fixture::new();
    let (direnv, stdout, stderr, success) = fixture.output();
    assert!(!direnv);
    assert!(success, "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "{}|--stdio|unset",
            fixture
                .directory
                .path()
                .join("project with spaces")
                .display()
        )
    );
}

#[test]
fn direnv_receives_the_project_and_supplies_the_server_environment() {
    let mut fixture = Fixture::new();
    executable(
        &fixture.directory.path().join("direnv"),
        "#!/bin/sh\n[ \"$1\" = exec ] || exit 2\n[ \"$2\" = \"$PWD\" ] || exit 3\nshift 2\nexport PROJECT_ENV=loaded\nexec \"$@\"\n",
    );
    let (direnv, stdout, stderr, success) = fixture.output();
    assert!(direnv);
    assert!(success, "{stderr}");
    assert!(stdout.ends_with("|--stdio|loaded"));
}

#[test]
fn direnv_failure_does_not_fall_back_to_the_inherited_environment() {
    let mut fixture = Fixture::new();
    executable(
        &fixture.directory.path().join("direnv"),
        "#!/bin/sh\necho '.envrc is blocked; run direnv allow' >&2\nexit 1\n",
    );
    let (direnv, stdout, stderr, success) = fixture.output();
    assert!(direnv);
    assert!(!success);
    assert!(stdout.is_empty());
    assert!(stderr.contains(".envrc is blocked"));
}

#[test]
fn stderr_keeps_a_bounded_tail_and_removes_terminal_escapes() {
    let input = format!("{}\n\x1b[31mblocked\x1b[0m\n", "x\n".repeat(10_000));
    let output = StderrOutput::read_tail(input.as_bytes());
    assert!(output.len() <= 4096);
    assert!(output.ends_with("blocked"));
    assert!(!output.contains('\x1b'));
}

#[test]
fn missing_server_error_survives_a_long_nix_path() {
    let input = format!(
        "direnv: loading .envrc\ndirenv: error command 'typescript-language-server' not found on PATH '{}'\n",
        "/nix/store/long-package-name/bin:".repeat(1000)
    );
    let stderr = StderrOutput::read_tail(input.as_bytes());
    assert_eq!(
        StderrOutput::summary(&stderr),
        "direnv: error command 'typescript-language-server' not found"
    );
}

#[test]
fn diagnostics_bound_long_lines_and_preserve_their_start() {
    let input = format!("Error: {}\n", "é".repeat(10_000));
    let stderr = StderrOutput::read_tail(input.as_bytes());
    assert!(stderr.len() < 1100);
    let summary = StderrOutput::summary(&stderr);
    assert!(summary.starts_with("Error: "));
    assert!(summary.ends_with('…'));
    assert_eq!(summary.chars().count(), 301);
}

#[test]
#[ignore = "requires direnv"]
fn real_direnv_loads_a_server_from_the_project_environment() {
    let direnv = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join("direnv"))
        .find(|path| path.is_file())
        .expect("direnv must be installed");
    let mut fixture = Fixture::new();
    let root = fixture.directory.path().join("project with spaces");
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    fs::rename(fixture.directory.path().join("expert"), bin.join("expert")).unwrap();
    std::os::unix::fs::symlink(&direnv, fixture.directory.path().join("direnv")).unwrap();
    fs::write(
        root.join(".envrc"),
        "export PROJECT_ENV=loaded\nPATH_add bin\n",
    )
    .unwrap();
    let mut allow = Command::new(direnv);
    allow.arg("allow").arg(&root);
    let path = std::env::join_paths([
        fixture.directory.path(),
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .unwrap();
    for command in [&mut allow, &mut fixture.launcher.direnv] {
        command
            .env_clear()
            .env("PATH", &path)
            .env("HOME", fixture.directory.path())
            .env("XDG_CONFIG_HOME", fixture.directory.path().join("config"))
            .env("XDG_DATA_HOME", fixture.directory.path().join("data"))
            .env("XDG_CACHE_HOME", fixture.directory.path().join("cache"));
    }
    assert!(allow.status().unwrap().success());
    let (direnv, stdout, stderr, success) = fixture.output();
    assert!(direnv);
    assert!(success, "{stderr}");
    assert!(stdout.ends_with("|--stdio|loaded"));
}
