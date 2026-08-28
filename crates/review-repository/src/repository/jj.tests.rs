use review_test_support::{JjFixture, JjLayout};

use super::{parse_revision_candidates, revision_candidates};
use crate::repository::{Repository, RevisionDirection};

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
