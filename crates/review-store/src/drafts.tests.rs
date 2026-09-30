use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::{Draft, ThreadSource};
use serde_json::json;

use super::*;
use crate::stored_fixtures::{CONTEXT_KEY, DRAFTS_V1, THREADS_V4, inline_source, inline_thread};

fn source() -> Arc<ThreadSource> {
    Arc::new(ThreadSource {
        anchor: DiffRangeAnchor {
            source_checkpoint: "checkpoint".into(),
            old_path: None,
            new_path: Some("source.rs".into()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: AnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(b"original".to_vec()),
            diff_hash: String::new(),
        },
        excerpt: "+source".into(),
    })
}

fn open(directory: &tempfile::TempDir) -> ReviewStore {
    ReviewStore::open(directory.path().join("state"), directory.path()).unwrap()
}

fn draft(text: &str) -> Draft {
    let mut draft = Draft::start("source.rs".into(), source());
    draft.text = text.into();
    draft
}

fn save(store: &ReviewStore, unit: &ReviewUnit, draft: &Draft) {
    store
        .update_drafts(unit, |drafts, threads| drafts.save(draft.clone(), threads))
        .unwrap();
}

fn decoded(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(
        &zstd::stream::decode_all(std::fs::read(path).unwrap().as_slice()).unwrap(),
    )
    .unwrap()
}

#[test]
fn saving_or_discarding_a_draft_leaves_the_thread_document_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let unit: ReviewUnit = "change".into();
    save(&store, &unit, &draft("Before any thread"));
    let threads = store.review_record_path("conversations", &unit).unwrap();
    assert!(
        !threads.exists(),
        "a draft must not create the thread document"
    );

    let (_, book) = store
        .update_threads(&unit, |book| book.post(draft("Posted").post()))
        .unwrap();
    let written = std::fs::read(&threads).unwrap();
    let inode = std::fs::metadata(&threads).unwrap().ino();
    let mut reply = Draft::reply(&book.threads()[0], book.threads()[0].messages[0].id.clone());
    reply.text = "Reply".into();
    save(&store, &unit, &reply);
    store
        .update_drafts(&unit, |drafts, _| {
            drafts.discard(reply.thread_id());
            Ok(())
        })
        .unwrap();
    assert_eq!(std::fs::metadata(&threads).unwrap().ino(), inode);
    assert_eq!(std::fs::read(&threads).unwrap(), written);
}

#[test]
fn drafts_survive_restart_without_publishing_and_cancelling_discards_them() {
    let directory = tempfile::tempdir().unwrap();
    let unit: ReviewUnit = "change".into();
    let store = open(&directory);
    let draft = draft("Unposted\nmultiline question");
    save(&store, &unit, &draft);
    drop(store);

    let store = open(&directory);
    assert!(store.load_threads(&unit).unwrap().threads().is_empty());
    let recovered = store.load_drafts(&unit).unwrap();
    assert_eq!(recovered.drafts(), std::slice::from_ref(&draft));
    assert_eq!(recovered.drafts()[0].post(), draft.post());
    store
        .update_drafts(&unit, |drafts, _| {
            drafts.discard(draft.thread_id());
            Ok(())
        })
        .unwrap();
    assert!(open(&directory).load_drafts(&unit).unwrap().is_empty());
}

#[test]
fn a_post_interrupted_before_its_draft_is_discarded_never_brings_the_draft_back() {
    let directory = tempfile::tempdir().unwrap();
    let unit: ReviewUnit = "change".into();
    let store = open(&directory);
    let posted = draft("Posted question");
    let kept = draft("Still composing");
    save(&store, &unit, &posted);
    save(&store, &unit, &kept);
    // Posting writes the thread document first. Crash before the draft store is updated.
    store
        .update_threads(&unit, |book| book.post(posted.post()))
        .unwrap();
    drop(store);

    let store = open(&directory);
    assert_eq!(
        store.read_drafts(&unit).unwrap().drafts().len(),
        2,
        "the crash left the posted draft in the draft store"
    );
    assert_eq!(
        store.load_drafts(&unit).unwrap().drafts(),
        std::slice::from_ref(&kept)
    );
    assert!(
        store
            .update_drafts(&unit, |drafts, threads| drafts
                .save(posted.clone(), threads))
            .is_err()
    );
    let ((), saved) = store.update_drafts(&unit, |_, _| Ok(())).unwrap();
    assert_eq!(saved.drafts(), std::slice::from_ref(&kept));
    assert_eq!(store.read_drafts(&unit).unwrap().drafts(), [kept]);
}

/// A thread document written by a build that kept drafts inside it.
struct EmbeddedFixture {
    unit: ReviewUnit,
    new_thread: Draft,
    reply: Draft,
    document: serde_json::Value,
}

impl EmbeddedFixture {
    fn write(store: &ReviewStore) -> Self {
        store.write_context_fixture();
        let mut document: serde_json::Value = serde_json::from_str(THREADS_V4).unwrap();
        document["conversations"]["drafts"] = json!([
            {
                "target": {"File": "source.rs"},
                "source": {"context": CONTEXT_KEY},
                "reply_to": null,
                "text": "Unposted\nquestion",
                "id": "5d1b9a9e-8f43-4a4c-9d55-3f7d0b1c2e01",
                "thread": "9a7c3f10-2b8e-4f6d-a1c4-0e5b7d9f2a33",
            },
            {
                "target": {"Thread": "3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f"},
                "source": {"context": CONTEXT_KEY},
                "reply_to": "33333333-3333-4333-8333-333333333333",
                "text": "Unposted reply",
                "id": "0c2e4a6b-8d1f-4e3a-b5c7-d9e1f3a5b7c9",
                "thread": "3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f",
            },
        ]);
        let fixture = Self {
            new_thread: serde_json::from_value(Self::hydrated(&document, 0)).unwrap(),
            reply: serde_json::from_value(Self::hydrated(&document, 1)).unwrap(),
            unit: "change".into(),
            document,
        };
        fixture.restore(store);
        fixture
    }

    fn hydrated(document: &serde_json::Value, index: usize) -> serde_json::Value {
        let mut draft = document["conversations"]["drafts"][index].clone();
        draft["source"] = inline_source();
        draft
    }

    /// Put the embedded drafts back, as if the migration stopped before rewriting it.
    fn restore(&self, store: &ReviewStore) {
        store.write_json_fixture(&store.threads_fixture_path(), &self.document.to_string());
    }
}

#[test]
fn a_version_1_draft_document_loads_and_is_written_back_byte_for_byte() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let unit: ReviewUnit = "change".into();
    store.write_context_fixture();
    store.write_json_fixture(&store.threads_fixture_path(), THREADS_V4);
    let path = store.drafts_path(&unit).unwrap();
    store.write_json_fixture(&path, DRAFTS_V1);

    let threads = store.load_threads(&unit).unwrap();
    let drafts = store.load_drafts(&unit).unwrap();
    let [reply, new_thread] = drafts.drafts() else {
        panic!("the fixture holds two drafts");
    };
    assert_eq!(reply.text, "Unposted \"reply\"\n");
    assert_eq!(reply.thread_id(), &threads.threads()[1].id);
    assert!(new_thread.is_new_thread());
    assert_eq!(new_thread.path(), "source.rs");
    assert!(
        Arc::ptr_eq(&reply.source, &threads.threads()[1].source)
            && Arc::ptr_eq(&new_thread.source, &reply.source),
        "drafts and threads with the same source share one loaded copy"
    );

    store.save_drafts(&unit, &drafts).unwrap();
    assert_eq!(ReviewStore::decoded_fixture(&path), DRAFTS_V1);
}

#[test]
fn drafts_saved_inside_the_thread_document_move_to_the_draft_store_on_first_load() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let fixture = EmbeddedFixture::write(&store);
    let expected = [fixture.new_thread.clone(), fixture.reply.clone()];

    let threads = store.load_threads(&fixture.unit).unwrap();
    assert_eq!(threads.threads().len(), 2);
    assert_eq!(store.read_drafts(&fixture.unit).unwrap().drafts(), expected);
    let migrated = decoded(
        &store
            .review_record_path("conversations", &fixture.unit)
            .unwrap(),
    );
    assert_eq!(migrated["version"], 4, "earlier builds must still load it");
    assert!(migrated["conversations"].get("drafts").is_none());
    assert_eq!(
        migrated["conversations"]["threads"],
        fixture.document["conversations"]["threads"]
    );

    // Loading again, or after the migration stopped before rewriting the thread
    // document, neither loses nor duplicates a draft.
    let reopened = open(&directory);
    assert_eq!(
        reopened.load_drafts(&fixture.unit).unwrap().drafts(),
        expected
    );
    fixture.restore(&reopened);
    assert_eq!(reopened.load_threads(&fixture.unit).unwrap(), threads);
    assert_eq!(
        reopened.load_drafts(&fixture.unit).unwrap().drafts(),
        expected
    );
    assert_eq!(
        reopened.read_drafts(&fixture.unit).unwrap().drafts(),
        expected
    );
}

#[test]
fn a_draft_an_earlier_build_saved_after_migration_replaces_the_stored_one() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let fixture = EmbeddedFixture::write(&store);
    let mut older = fixture.reply.clone();
    older.text = "Older reply text".into();
    let elsewhere = draft("Only in the draft store");
    store.load_threads(&fixture.unit).unwrap();
    store
        .update_drafts(&fixture.unit, |drafts, _| {
            drafts.keep(older.clone());
            drafts.keep(elsewhere.clone());
            Ok(())
        })
        .unwrap();
    // An earlier build rewrites the thread document with the drafts it knows.
    fixture.restore(&store);

    let mut texts = store
        .load_drafts(&fixture.unit)
        .unwrap()
        .drafts()
        .iter()
        .map(|draft| draft.text.clone())
        .collect::<Vec<_>>();
    texts.sort();
    assert_eq!(
        texts,
        [
            "Only in the draft store",
            "Unposted\nquestion",
            "Unposted reply"
        ]
    );
}

#[test]
fn an_unreadable_draft_store_never_hides_the_threads_or_loses_embedded_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let fixture = EmbeddedFixture::write(&store);
    let drafts = store.drafts_path(&fixture.unit).unwrap();
    std::fs::create_dir_all(drafts.parent().unwrap()).unwrap();
    std::fs::write(&drafts, "unreadable").unwrap();

    let threads = store.load_threads(&fixture.unit).unwrap();
    assert_eq!(threads.threads().len(), 2);
    assert!(store.load_drafts(&fixture.unit).is_err());
    assert!(store.update_threads(&fixture.unit, |_| Ok(())).is_err());
    let index = store
        .review_record_path("conversations", &fixture.unit)
        .unwrap();
    assert_eq!(decoded(&index), fixture.document);
    assert_eq!(std::fs::read_to_string(drafts).unwrap(), "unreadable");
}

#[test]
fn drafts_inside_a_version_3_thread_document_move_to_the_draft_store() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let unit: ReviewUnit = "change".into();
    let index = store.threads_fixture_path();
    store.write_json_fixture(
        &index,
        &json!({"version": 3, "conversations": {
            "review_unit": "change",
            "threads": [inline_thread(json!({
                "id": "0b6f2a4e-1c3d-4e5f-8a9b-0c1d2e3f4a5b",
                "messages": [{"id": "11111111-1111-4111-8111-111111111111", "author": "reviewer", "text": "Posted question", "sequence": 1}],
            }))],
            "sequence": 1,
            "drafts": [{
                "target": {"File": "source.rs"},
                "source": inline_source(),
                "reply_to": null,
                "text": "Inline draft",
                "id": "3f0c1d2e-4b5a-4c6d-8e7f-9a0b1c2d3e4f",
                "thread": "7e6d5c4b-3a2f-4e1d-9c0b-a1b2c3d4e5f6",
            }],
        }})
        .to_string(),
    );

    let threads = store.load_threads(&unit).unwrap();
    assert_eq!(threads.threads().len(), 1);
    assert_eq!(threads.threads()[0].messages[0].text, "Posted question");
    let drafts = store.load_drafts(&unit).unwrap();
    assert_eq!(drafts.drafts().len(), 1);
    assert_eq!(drafts.drafts()[0].text, "Inline draft");
    assert_eq!(*drafts.drafts()[0].source, *threads.threads()[0].source);
    let migrated = decoded(&index);
    assert_eq!(migrated["version"], 4);
    assert!(migrated["conversations"].get("drafts").is_none());
}
