use std::fs;

use herdr_client::protocol::{Agent, AgentStatus, PaneId, TabId, WorkspaceId};
use review_guide::{
    AnchoredGuideItem, DiffRangeAnchor, GuideAnchorKind, GuideItem, GuideItemStatus,
    GuideRequestId, GuideScope, GuideSnapshot, GuideTarget, ReviewCheckpoint,
};

use super::Fixture;
use crate::{GuideRequestRecord, GuideRequestState, StateKey};

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
    assert_eq!(&fs::read(snapshot).unwrap()[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
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
    let mut request = GuideRequestRecord {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new("review-unit", "checkpoint"),
        request_id: GuideRequestId::new("request"),
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
        state: GuideRequestState::Incomplete,
        created_at: String::new(),
        updated_at: String::new(),
        transport_directory: Some(std::env::temp_dir().join("herdr-review-guide-test")),
        error: None,
    };

    fixture.store().save_guide_request(&mut request).unwrap();
    let loaded = fixture.store().load_guide_requests("review-unit").unwrap();

    assert_eq!(loaded, vec![request]);
    assert_eq!(
        fixture
            .store()
            .active_guide_transport_directories()
            .unwrap(),
        vec![std::env::temp_dir().join("herdr-review-guide-test")]
    );
}
