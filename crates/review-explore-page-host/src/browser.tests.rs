use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use review_explore_page::RoundPublisher;

use super::*;
use crate::PageHost;
use crate::tests::page_round;

/// The workspace whose reviewer the tests open.
fn workspace() -> WorkspaceId {
    WorkspaceId("w1".into())
}

/// The page of [`workspace`]'s reviewer, recorded under `state`.
fn host(state: &Path) -> PageHost {
    let round = RoundPublisher::default();
    PageHost::start(
        page_round(&round),
        &PageDirectory::new(state),
        &workspace(),
        Path::new("/repositories/drafts"),
    )
    .unwrap()
}

/// Opens the page of [`workspace`] under `state` with `browser`.
fn opener(state: &Path, browser: Browser) -> PageOpener {
    PageOpener::new(PageDirectory::new(state), workspace(), browser)
}

/// A browser that writes the address it opens to `address_file`, then exits with `status`.
fn browser(directory: &Path, address_file: &Path, status: u8) -> Browser {
    let script = directory.join("browser");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf %s \"$2\" > '{}'\nexit {status}\n",
            address_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Browser::command(&format!("{} --new-tab", script.display()))
}

#[test]
fn the_opener_opens_the_page_of_the_workspaces_reviewer_in_the_browser() {
    let state = tempfile::tempdir().unwrap();
    let host = host(state.path());
    let address_file = state.path().join("address_file");

    opener(state.path(), browser(state.path(), &address_file, 0))
        .open()
        .unwrap();

    assert_eq!(std::fs::read_to_string(&address_file).unwrap(), host.url());
}

#[test]
fn without_an_open_reviewer_the_opener_opens_nothing_and_says_why() {
    let state = tempfile::tempdir().unwrap();
    let address_file = state.path().join("address_file");

    let failure = opener(state.path(), browser(state.path(), &address_file, 0))
        .open()
        .unwrap_err();

    assert_eq!(failure.url, None);
    assert!(!address_file.exists());
}

#[test]
fn a_browser_that_fails_leaves_the_address_to_open_by_hand() {
    let state = tempfile::tempdir().unwrap();
    let host = host(state.path());
    let address_file = state.path().join("address_file");

    let failure = opener(state.path(), browser(state.path(), &address_file, 3))
        .open()
        .unwrap_err();

    assert_eq!(failure.url.as_deref(), Some(host.url()));
    assert!(failure.reason.contains('3'), "{}", failure.reason);
}

#[test]
fn a_missing_browser_program_leaves_the_address_to_open_by_hand() {
    let state = tempfile::tempdir().unwrap();
    let host = host(state.path());
    let missing = state.path().join("no-such-browser");

    let failure = opener(
        state.path(),
        Browser::command(&missing.display().to_string()),
    )
    .open()
    .unwrap_err();

    assert_eq!(failure.url.as_deref(), Some(host.url()));
}
