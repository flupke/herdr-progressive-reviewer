use review_test_support::{JjFixture, JjLayout};

use super::{
    parse_revision_candidates, parse_revision_history, revision_candidates, revision_history,
};
use crate::repository::{ChangeId, Repository, RevisionDirection};

#[test]
fn revision_candidate_records_preserve_graph_order() {
    let candidates =
        parse_revision_candidates(b"first-full\0first\0First description\0second-full\0second\0\0")
            .unwrap();

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].change_id.as_str(), "first-full");
    assert_eq!(candidates[0].short_change_id, "first");
    assert_eq!(candidates[0].description, "First description");
    assert_eq!(candidates[1].change_id.as_str(), "second-full");
    assert_eq!(candidates[1].description, "");
}

#[test]
fn revision_candidate_records_require_complete_utf8_groups() {
    assert!(parse_revision_candidates(b"change\0short\0description").is_err());
    assert!(parse_revision_candidates(b"change\0short\0\0").is_ok());
    assert!(parse_revision_candidates(b"change\0short\0description\0extra\0").is_err());
    assert!(parse_revision_candidates(b"change\0short\0\xff\0").is_err());
}

#[test]
fn revision_history_records_keep_graph_text_and_extract_change_ids() {
    let history = parse_revision_history(
        b"\x1b[32m@\x1b[0m  \x1e\x1b[35mfull-id\x1b[39m:short:1:0\x1f\x1d\x1b[35mshort\x1b[39m message\n\xe2\x94\x82\n",
    )
    .unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].change_id.as_ref().unwrap().as_str(), "full-id");
    assert_eq!(history[0].short_change_id.as_deref(), Some("short"));
    assert_eq!(history[0].plain_text, "@  short message");
    assert!(history[0].is_current);
    assert!(!history[0].is_immutable);
    assert!(history[0].text.contains("\x1b[32m@"));
    assert_eq!(history[1].change_id, None);
    assert_eq!(history[1].short_change_id, None);
    assert_eq!(history[1].plain_text, "│");
    assert!(!history[1].is_current);
    assert!(!history[1].is_immutable);
}

#[test]
fn revision_candidates_and_edits_follow_the_real_jj_graph() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    let parent = fixture.change_id();

    fixture.new_change("first child");
    let first_child = fixture.change_id();
    fixture.edit(&parent);
    fixture.new_change("second child");
    let second_child = fixture.change_id();
    fixture.edit(&parent);

    let children = revision_candidates(&repository, RevisionDirection::Children).unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().any(|child| {
        child.change_id.as_str() == first_child && child.description == "first child"
    }));
    assert!(children.iter().any(|child| {
        child.change_id.as_str() == second_child && child.description == "second child"
    }));

    let first_child = children
        .iter()
        .find(|child| child.change_id.as_str() == first_child)
        .unwrap();
    assert!(repository.edit_revision(&first_child.change_id).unwrap());
    assert_eq!(fixture.change_id(), first_child.change_id.as_str());

    let parents = revision_candidates(&repository, RevisionDirection::Parents).unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].change_id.as_str(), parent);
}

#[test]
fn revision_history_uses_the_real_jj_graph_and_stops_at_the_immutable_boundary() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.new_change("first mutable change");
    let first = fixture.change_id();
    fixture.new_change("second mutable change");
    let second = fixture.change_id();

    let history = revision_history(&repository).unwrap();
    let change_ids = history
        .iter()
        .filter_map(|line| line.change_id.as_ref().map(ChangeId::as_str))
        .collect::<Vec<_>>();

    assert!(change_ids.contains(&second.as_str()));
    assert!(change_ids.contains(&first.as_str()));
    assert!(history.iter().any(|line| line.is_immutable));
    assert!(
        history
            .iter()
            .filter(|line| {
                line.change_id.as_ref().is_some_and(|change_id| {
                    change_id.as_str() == first || change_id.as_str() == second
                })
            })
            .all(|line| !line.is_immutable)
    );
    assert!(
        history
            .iter()
            .any(|line| line.plain_text.contains("first mutable change"))
    );
    assert!(history.len() >= 3);
    assert_eq!(
        history
            .iter()
            .find(|line| line.is_current)
            .unwrap()
            .change_id,
        Some(ChangeId::from(second.clone()))
    );
}

#[test]
fn revision_history_includes_mutable_children_after_editing_an_ancestor() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.new_change("mutable parent");
    let mutable_parent = fixture.change_id();
    fixture.new_change("mutable child");
    let mutable_child = fixture.change_id();
    fixture.jj(["edit", &mutable_parent]);

    let history = revision_history(&repository).unwrap();

    assert!(history.iter().any(|line| {
        !line.is_immutable
            && line
                .change_id
                .as_ref()
                .is_some_and(|change_id| change_id.as_str() == mutable_child)
    }));
}

#[test]
fn revision_history_includes_immutable_children_of_the_boundary() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.jj(["new", "root()", "-m", "first immutable child"]);
    let first_immutable_child = fixture.change_id();
    fixture.jj(["bookmark", "create", "first-immutable-child"]);
    fixture.jj(["new", "root()", "-m", "second immutable child"]);
    let second_immutable_child = fixture.change_id();
    fixture.jj(["bookmark", "create", "second-immutable-child"]);
    fixture.jj([
        "config",
        "set",
        "--repo",
        "revset-aliases.'immutable_heads()'",
        "first-immutable-child | second-immutable-child",
    ]);
    fixture.jj(["new", "root()", "-m", "current mutable change"]);

    let history = revision_history(&repository).unwrap();

    for immutable_child in [first_immutable_child, second_immutable_child] {
        assert!(history.iter().any(|line| {
            line.is_immutable
                && line
                    .change_id
                    .as_ref()
                    .is_some_and(|change_id| change_id.as_str() == immutable_child)
        }));
    }
}
