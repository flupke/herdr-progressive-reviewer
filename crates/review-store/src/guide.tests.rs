use std::fs;
use std::os::unix::fs::PermissionsExt;

use review_guide::{
    AnchoredGuideItem, DiffRangeAnchor, GuideAnchorKind, GuideItem, GuideItemStatus, GuideScope,
    GuideSnapshot, GuideTarget, ReviewCheckpoint,
};

use super::Fixture;
use crate::{Error, StateKey};

fn guide(checkpoint: &str, text: &str) -> GuideSnapshot {
    GuideSnapshot {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new("review-unit", checkpoint),
        scope: GuideScope::All,
        items: vec![GuideItem {
            target: GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            },
            text: text.to_owned(),
            status: GuideItemStatus::Matched,
        }],
        anchored_items: Vec::new(),
    }
}

fn guide_with_repeated_anchor_contents() -> GuideSnapshot {
    let mut guide = guide("large-checkpoint", "large guide");
    let content = vec![0; 400_000];
    guide.anchored_items = (0..3)
        .map(|index| AnchoredGuideItem {
            text: format!("explanation {index}"),
            anchor: DiffRangeAnchor {
                source_checkpoint: guide.review_checkpoint.checkpoint.clone(),
                old_path: None,
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: None,
                new_lines: Some(index..index.saturating_add(1)),
                target_kind: GuideAnchorKind::Lines,
                source_hunk_count: 1,
                old_content: None,
                new_content: Some(content.clone()),
                diff_hash: "diff-hash".to_owned(),
            },
        })
        .collect();
    guide
}

#[test]
fn latest_guide_changes_without_removing_checkpoint_history() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = guide("checkpoint-one", "first");
    let replacement = guide("checkpoint-one", "replacement");
    let second = guide("checkpoint-two", "second");

    store.save_guide(&first).unwrap();
    store.save_guide(&replacement).unwrap();
    store.save_guide(&second).unwrap();

    assert_eq!(store.load_guide("review-unit").unwrap(), Some(second));
    let snapshots = store
        .repository_dir
        .join("guides")
        .join(StateKey::hash(b"review-unit").0)
        .join("snapshots");
    assert_eq!(fs::read_dir(snapshots).unwrap().count(), 2);
}

#[test]
fn guide_snapshots_are_deduplicated_and_zstd_compressed() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide_with_repeated_anchor_contents();

    store.save_guide(&guide).unwrap();

    let snapshot = store.guide_snapshot_path(&guide.review_checkpoint);
    let compressed = fs::read(snapshot).unwrap();
    assert_eq!(&compressed[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
    let stored: serde_json::Value =
        serde_json::from_slice(&zstd::stream::decode_all(compressed.as_slice()).unwrap()).unwrap();
    assert_eq!(stored["anchor_contents"].as_array().unwrap().len(), 1);
    assert_eq!(
        stored["anchor_content_indices"],
        serde_json::json!([0, 0, 0])
    );
    assert_eq!(store.load_guide("review-unit").unwrap(), Some(guide));
}

#[test]
fn missing_or_malformed_latest_guide_is_ignored() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide("checkpoint", "text");
    store.save_guide(&guide).unwrap();
    fs::remove_file(store.guide_snapshot_path(&guide.review_checkpoint)).unwrap();
    assert_eq!(store.load_guide("review-unit").unwrap(), None);

    store.save_guide(&guide).unwrap();
    fs::write(store.guide_state_path("review-unit"), b"{invalid").unwrap();
    assert_eq!(store.load_guide("review-unit").unwrap(), None);
}

#[test]
fn guide_keys_reject_empty_long_or_unsafe_values() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for review_unit in ["", "unsafe/path", &"x".repeat(257)] {
        let mut invalid = guide("checkpoint", "text");
        invalid.review_checkpoint.review_unit = review_unit.to_owned();
        assert!(matches!(
            store.save_guide(&invalid),
            Err(Error::InvalidStateKey {
                field: "review unit"
            })
        ));
        assert!(store.guide_mailbox_directory(review_unit).is_err());
    }
    for checkpoint in ["", "unsafe/path", &"x".repeat(257)] {
        let invalid = guide(checkpoint, "text");
        assert!(matches!(
            store.save_guide(&invalid),
            Err(Error::InvalidStateKey {
                field: "checkpoint"
            })
        ));
    }
}

#[test]
fn mailbox_path_is_stable_and_private() {
    let fixture = Fixture::new();
    let store = fixture.store();

    let first = store.guide_mailbox_directory("review-unit").unwrap();
    let second = store.guide_mailbox_directory("review-unit").unwrap();

    assert_eq!(first, second);
    assert_eq!(fs::metadata(first).unwrap().permissions().mode() & 0o077, 0);
}

#[test]
fn stored_guide_envelope_requires_the_current_version_and_anchor_count() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide_with_repeated_anchor_contents();
    store.save_guide(&guide).unwrap();
    let snapshot_path = store.guide_snapshot_path(&guide.review_checkpoint);
    let original = zstd::stream::decode_all(fs::read(&snapshot_path).unwrap().as_slice()).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&original).unwrap();

    for (field, value) in [
        ("storage_version", serde_json::json!(2)),
        ("anchor_content_indices", serde_json::json!([0, 0])),
    ] {
        let mut altered = original.clone();
        altered[field] = value;
        let compressed =
            zstd::stream::encode_all(serde_json::to_vec(&altered).unwrap().as_slice(), 3).unwrap();
        fs::write(&snapshot_path, compressed).unwrap();
        assert_eq!(store.load_guide("review-unit").unwrap(), None, "{field}");
    }
}
