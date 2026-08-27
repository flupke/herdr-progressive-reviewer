use std::fs;

use herdr_client::protocol::{Agent, AgentStatus, PaneId, TabId, WorkspaceId};
use review_guide::{
    AnchoredGuideItem, DiffRangeAnchor, GuideAnchorKind, GuideItem, GuideItemStatus,
    GuideRequestId, GuideScope, GuideSnapshot, GuideTarget, ReviewCheckpoint,
};

use super::Fixture;
use crate::{Error, GuideRequestRecord, GuideRequestState, StateKey};

fn guide(request_id: &str, checkpoint: &str, text: &str) -> GuideSnapshot {
    GuideSnapshot {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new("review-unit", checkpoint),
        request_id: GuideRequestId::new(request_id),
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
    let mut guide = guide("large-request", "large-checkpoint", "large guide");
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

fn guide_request(request_id: &str, state: GuideRequestState) -> GuideRequestRecord {
    GuideRequestRecord {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new("review-unit", "checkpoint"),
        request_id: GuideRequestId::new(request_id),
        scope: GuideScope::All,
        agent: Agent {
            pane_id: PaneId("pane".to_owned()),
            tab_id: TabId("tab".to_owned()),
            workspace_id: WorkspaceId("workspace".to_owned()),
            name: None,
            display_agent: None,
            agent: Some("codex".to_owned()),
            agent_status: AgentStatus::Working,
            agent_session: None,
            cwd: None,
        },
        state,
        created_at: String::new(),
        updated_at: String::new(),
        transport_directory: Some(
            std::env::temp_dir().join(format!("herdr-review-guide-{request_id}")),
        ),
        error: None,
    }
}

#[test]
fn latest_guide_changes_without_removing_snapshot_history() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = guide("request-one", "checkpoint-one", "first");
    let second = guide("request-two", "checkpoint-two", "second");

    store.save_guide(&first).unwrap();
    store.save_guide(&second).unwrap();

    assert_eq!(store.load_guide("review-unit").unwrap(), Some(second));
    let snapshots = store
        .repository_dir
        .join("guides")
        .join(StateKey::hash(b"review-unit").0)
        .join("snapshots");
    assert_eq!(
        fs::read_dir(snapshots)
            .unwrap()
            .flat_map(|directory| fs::read_dir(directory.unwrap().path()).unwrap())
            .count(),
        2
    );
}

#[test]
fn guide_snapshots_are_deduplicated_and_zstd_compressed() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide_with_repeated_anchor_contents();

    store.save_guide(&guide).unwrap();

    let unit_directory = store
        .repository_dir
        .join("guides")
        .join(StateKey::hash(b"review-unit").0);
    let latest = unit_directory.join("state.json");
    let snapshot = unit_directory
        .join("snapshots")
        .join(StateKey::hash(b"large-checkpoint").0)
        .join(format!("{}.json.zst", StateKey::hash(b"large-request").0));
    assert!(fs::metadata(latest).unwrap().len() < crate::MAX_STATE_FILE_BYTES);
    assert!(fs::metadata(&snapshot).unwrap().len() < crate::MAX_STATE_FILE_BYTES);
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
fn missing_latest_guide_snapshot_is_ignored() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide("request", "checkpoint", "text");
    store.save_guide(&guide).unwrap();
    let snapshot = store.guide_snapshot_path(&guide.review_checkpoint, &guide.request_id);
    fs::remove_file(snapshot).unwrap();

    assert_eq!(store.load_guide("review-unit").unwrap(), None);
}

#[test]
fn malformed_latest_guide_is_ignored() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store
        .save_guide(&guide("request", "checkpoint", "text"))
        .unwrap();
    let latest = store.guide_state_path("review-unit");
    fs::write(latest, b"{invalid").unwrap();

    assert_eq!(store.load_guide("review-unit").unwrap(), None);
}

#[test]
fn incomplete_guide_requests_survive_store_reopen() {
    let fixture = Fixture::new();
    let mut request = guide_request("request", GuideRequestState::Incomplete);

    fixture.store().save_guide_request(&mut request).unwrap();
    let loaded = fixture.store().load_guide_requests("review-unit").unwrap();

    assert_eq!(loaded, vec![request]);
    assert_eq!(
        fixture
            .store()
            .active_guide_transport_directories()
            .unwrap(),
        vec![std::env::temp_dir().join("herdr-review-guide-request")]
    );
}

#[test]
fn guide_keys_reject_empty_long_or_unsafe_values() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for review_unit in ["", "unsafe/path", &"x".repeat(257)] {
        let mut invalid = guide("request", "checkpoint", "text");
        invalid.review_checkpoint.review_unit = review_unit.to_owned();
        assert!(matches!(
            store.save_guide(&invalid),
            Err(Error::InvalidStateKey {
                field: "review unit"
            })
        ));
        assert!(matches!(
            store.load_guide(review_unit),
            Err(Error::InvalidStateKey {
                field: "review unit"
            })
        ));
    }
    for checkpoint in ["", "unsafe/path", &"x".repeat(257)] {
        let invalid = guide("request", checkpoint, "text");
        assert!(matches!(
            store.save_guide(&invalid),
            Err(Error::InvalidStateKey {
                field: "checkpoint"
            })
        ));
    }
    for request_id in ["", "unsafe/path", &"x".repeat(257)] {
        let invalid = guide(request_id, "checkpoint", "text");
        assert!(matches!(
            store.save_guide(&invalid),
            Err(Error::InvalidStateKey {
                field: "request ID"
            })
        ));
    }
}

#[test]
fn latest_pointer_and_snapshot_must_match_the_requested_review_unit() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let stored_guide = guide("request", "checkpoint", "text");
    store.save_guide(&stored_guide).unwrap();
    let state_path = store.guide_state_path("review-unit");
    let original_state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();

    for (field, value) in [
        ("storage_version", serde_json::json!(2)),
        ("latest_checkpoint_id", serde_json::json!("unsafe/path")),
        ("latest_request_id", serde_json::json!("unsafe/path")),
    ] {
        let mut state = original_state.clone();
        state[field] = value;
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(store.load_guide("review-unit").unwrap(), None, "{field}");
    }

    let invalid_request_id = GuideRequestId::new("unsafe/path");
    let valid_snapshot =
        store.guide_snapshot_path(&stored_guide.review_checkpoint, &stored_guide.request_id);
    let invalid_snapshot =
        store.guide_snapshot_path(&stored_guide.review_checkpoint, &invalid_request_id);
    fs::create_dir_all(invalid_snapshot.parent().unwrap()).unwrap();
    fs::copy(&valid_snapshot, &invalid_snapshot).unwrap();
    let mut invalid_request_state = original_state.clone();
    invalid_request_state["latest_request_id"] = serde_json::json!("unsafe/path");
    fs::write(
        &state_path,
        serde_json::to_vec(&invalid_request_state).unwrap(),
    )
    .unwrap();
    assert_eq!(store.load_guide("review-unit").unwrap(), None);

    fs::write(&state_path, serde_json::to_vec(&original_state).unwrap()).unwrap();
    let snapshot_path =
        store.guide_snapshot_path(&stored_guide.review_checkpoint, &stored_guide.request_id);
    let decoded = zstd::stream::decode_all(fs::read(&snapshot_path).unwrap().as_slice()).unwrap();
    let mut snapshot: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    snapshot["guide"]["review_unit"] = serde_json::json!("other-unit");
    let altered =
        zstd::stream::encode_all(serde_json::to_vec(&snapshot).unwrap().as_slice(), 3).unwrap();
    fs::write(snapshot_path, altered).unwrap();

    assert_eq!(store.load_guide("review-unit").unwrap(), None);
}

#[test]
fn guide_snapshot_envelope_must_have_the_current_version_and_anchor_count() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let guide = guide_with_repeated_anchor_contents();
    store.save_guide(&guide).unwrap();
    let snapshot_path = store.guide_snapshot_path(&guide.review_checkpoint, &guide.request_id);
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

#[test]
fn only_resumable_request_states_have_active_transports() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for (index, state) in [
        GuideRequestState::Preparing,
        GuideRequestState::Submitted,
        GuideRequestState::Incomplete,
        GuideRequestState::Completed,
        GuideRequestState::Failed,
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = guide_request(&format!("request-{index}"), state);
        store.save_guide_request(&mut request).unwrap();
        assert!(request.created_at.contains('T'));
        assert!(request.created_at.ends_with('Z'));
        assert!(request.updated_at.contains('T'));
        assert!(request.updated_at.ends_with('Z'));
    }

    let mut transports = store.active_guide_transport_directories().unwrap();
    transports.sort();
    assert_eq!(
        transports,
        [0, 1, 2]
            .map(|index| std::env::temp_dir().join(format!("herdr-review-guide-request-{index}")))
    );
}

#[test]
fn missing_guide_directories_load_as_empty() {
    let fixture = Fixture::new();
    let store = fixture.store();

    assert!(store.load_guide_requests("review-unit").unwrap().is_empty());
    assert!(
        store
            .active_guide_transport_directories()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn guide_directory_read_errors_are_reported() {
    let requests_fixture = Fixture::new();
    let requests_store = requests_fixture.store();
    let requests = requests_store
        .repository_dir
        .join("guides")
        .join(StateKey::hash(b"review-unit").0)
        .join("requests");
    fs::create_dir_all(requests.parent().unwrap()).unwrap();
    fs::write(&requests, b"not a directory").unwrap();
    assert!(matches!(
        requests_store.load_guide_requests("review-unit"),
        Err(Error::StateIo { .. })
    ));

    let active_fixture = Fixture::new();
    let active_store = active_fixture.store();
    fs::write(
        active_store.repository_dir.join("guides"),
        b"not a directory",
    )
    .unwrap();
    assert!(matches!(
        active_store.active_guide_transport_directories(),
        Err(Error::StateIo { .. })
    ));
}

#[test]
fn request_history_ignores_wrong_schema_and_review_unit() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut request = guide_request("request", GuideRequestState::Incomplete);
    store.save_guide_request(&mut request).unwrap();
    let request_path = store
        .repository_dir
        .join("guides")
        .join(StateKey::hash(b"review-unit").0)
        .join("requests")
        .join(format!("{}.json", StateKey::hash(b"request").0));
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(&request_path).unwrap()).unwrap();

    for (field, value) in [
        ("schema_version", serde_json::json!(2)),
        ("review_unit", serde_json::json!("other-unit")),
    ] {
        let mut altered = original.clone();
        altered[field] = value;
        fs::write(&request_path, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert!(
            store.load_guide_requests("review-unit").unwrap().is_empty(),
            "{field}"
        );
    }
}
