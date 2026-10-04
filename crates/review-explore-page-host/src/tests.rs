use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use herdr_client::protocol::WorkspaceId;
use review_explore_page::{CommandRefusal, CommandSender, RoundPublisher};

use super::*;

fn workspace(id: &str) -> WorkspaceId {
    WorkspaceId(id.into())
}

/// The page's side of `round`, whose commands no session takes.
pub(crate) fn page_round(round: &RoundPublisher) -> PageRound {
    let commands = CommandSender::new(|_, reply| {
        reply.send(Err(CommandRefusal::Failed(
            "No session in this test".into(),
        )));
    });
    PageRound::new(round.subscribe(), commands)
}

/// The review of the tests' reviewers.
const REVIEW: &str = "/repositories/drafts";

fn host(directory: &PageDirectory, workspace: &WorkspaceId) -> PageHost {
    host_of(directory, workspace, REVIEW)
}

fn host_of(directory: &PageDirectory, workspace: &WorkspaceId, review: &str) -> PageHost {
    let round = RoundPublisher::default();
    PageHost::start(page_round(&round), directory, workspace, Path::new(review)).unwrap()
}

/// The token of the page's address `url`.
fn token(url: &str) -> &str {
    url.rsplit('=').next().unwrap()
}

#[test]
fn a_restarted_reviewer_serves_its_page_at_the_same_address_with_the_same_token() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let before = host(&directory, &workspace("w1"));
    let url = before.url().to_owned();
    drop(before);

    let after = host(&directory, &workspace("w1"));

    assert_eq!(
        after.url(),
        url,
        "an open tab reconnects to the restarted reviewer"
    );
    assert_eq!(
        directory.address(&workspace("w1")).unwrap().as_deref(),
        Some(url.as_str())
    );
}

#[test]
fn a_reviewer_of_another_review_opens_its_page_behind_a_new_token() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let before = host(&directory, &workspace("w1"));
    let url = before.url().to_owned();
    drop(before);

    let other = host_of(&directory, &workspace("w1"), "/repositories/other");

    assert_ne!(token(other.url()), token(&url));
}

#[test]
fn a_second_open_reviewer_of_the_workspace_gets_a_token_of_its_own() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let first = host(&directory, &workspace("w1"));

    let second = host(&directory, &workspace("w1"));

    assert_ne!(token(first.url()), token(second.url()));
}

#[test]
fn the_action_finds_the_page_of_the_workspaces_open_reviewer_until_it_closes() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let host = host(&directory, &workspace("w1"));

    let url = directory.address(&workspace("w1")).unwrap().unwrap();
    assert_eq!(url, host.url());
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(url.contains("/?token="), "{url}");

    drop(host);
    assert_eq!(directory.address(&workspace("w1")).unwrap(), None);
}

#[test]
fn each_workspace_finds_the_page_of_its_own_reviewer() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let first = host(&directory, &workspace("w1"));
    let second = host(&directory, &workspace("w2"));

    assert_eq!(
        directory.address(&workspace("w1")).unwrap().as_deref(),
        Some(first.url())
    );
    assert_eq!(
        directory.address(&workspace("w2")).unwrap().as_deref(),
        Some(second.url())
    );
    assert_ne!(first.url(), second.url());
    assert_eq!(directory.address(&workspace("w3")).unwrap(), None);
}

#[test]
fn the_page_of_a_reviewer_that_died_is_not_found() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let host = host(&directory, &workspace("w1"));
    let record = std::fs::read(directory.record(&workspace("w1"))).unwrap();
    // A reviewer killed before it could remove its record leaves it behind.
    drop(host);
    std::fs::write(directory.record(&workspace("w1")), record).unwrap();

    assert_eq!(directory.address(&workspace("w1")).unwrap(), None);
}

#[test]
fn a_closing_reviewer_leaves_the_record_of_a_newer_one_in_place() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let older = host(&directory, &workspace("w1"));
    let newer = host(&directory, &workspace("w1"));

    drop(older);

    assert_eq!(
        directory.address(&workspace("w1")).unwrap().as_deref(),
        Some(newer.url())
    );
}

#[test]
fn only_the_user_can_read_the_address_and_its_token() {
    let state = tempfile::tempdir().unwrap();
    let directory = PageDirectory::new(state.path());
    let _host = host(&directory, &workspace("w1"));

    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&directory.record(&workspace("w1"))), 0o600);
    assert_eq!(mode(&state.path().join("explore-page")), 0o700);
}
