use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

use review_guide::{DiffRangeAnchor, GuideAnchorKind};
use review_threads::{Draft, MessageId, Post, ThreadSource};

use super::*;

fn source() -> Arc<ThreadSource> {
    Arc::new(ThreadSource {
        anchor: DiffRangeAnchor {
            source_checkpoint: "checkpoint".into(),
            old_path: None,
            new_path: Some("source.rs".into()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: GuideAnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(vec![b'x'; 2_000_000]),
            diff_hash: String::new(),
        },
        excerpt: "+source".into(),
    })
}

#[test]
fn small_updates_share_context_and_do_not_rewrite_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let unit = "change".into();
    let mut draft = Draft::start("source.rs".into(), source());
    draft.text = "Question".into();
    let post = draft.post();
    let thread = post.thread_id().clone();
    let (_, first) = store.update_threads(&unit, |book| book.post(post)).unwrap();
    let context = std::fs::read_dir(store.repository_dir.join("thread-contexts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = std::fs::File::open(&context).unwrap();
    let inode = file.metadata().unwrap().ino();
    let question = first.threads()[0].last_comment().unwrap().id.clone();
    let (_, second) = store
        .update_threads(&unit, |book| {
            book.answer(Post::answer(
                thread.clone(),
                MessageId::parse("93e8d6c7-e7f0-438d-9ca6-7f1be6ec470a").unwrap(),
                "Answer".into(),
                question,
            ))
        })
        .unwrap();
    let ((), third) = store
        .update_threads(&unit, |book| book.mark_read(&thread, book.sequence()))
        .unwrap();
    assert!(Arc::ptr_eq(
        &first.threads()[0].source,
        &second.threads()[0].source
    ));
    assert!(Arc::ptr_eq(
        &first.threads()[0].source,
        &third.threads()[0].source
    ));
    assert_eq!(std::fs::metadata(context).unwrap().ino(), inode);
    let path = store.threads_path(&unit).unwrap();
    let json = ReviewStore::decode_thread_json(&path, &std::fs::read(&path).unwrap()).unwrap();
    assert!(
        json.len() < 4096,
        "mutable metadata must not embed source snapshots"
    );
    let reopened = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    assert_eq!(reopened.load_threads(&unit).unwrap(), third);
}

#[test]
fn drafts_survive_restart_without_publishing_and_posting_removes_them_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let unit = "change".into();
    let mut draft = Draft::start("source.rs".into(), source());
    draft.text = "Unposted\nmultiline question".into();
    let post = draft.post();
    let thread = post.thread_id().clone();
    store
        .update_threads(&unit, |book| book.save_draft(draft.clone()))
        .unwrap();
    drop(store);
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let recovered = store.load_threads(&unit).unwrap();
    assert!(recovered.threads().is_empty());
    assert!(!recovered.has_new_messages());
    assert_eq!(recovered.drafts(), [draft]);
    assert_eq!(recovered.drafts()[0].post(), post);
    store
        .update_threads(&unit, |book| book.post(post.clone()))
        .unwrap();
    store.update_threads(&unit, |book| book.post(post)).unwrap();
    let posted = store.load_threads(&unit).unwrap();
    assert!(posted.drafts().is_empty());
    let mut reply = Draft::reply(
        posted.thread(&thread).unwrap(),
        posted.threads()[0].messages[0].id.clone(),
    );
    reply.text = "Do not publish me".into();
    store
        .update_threads(&unit, |book| book.save_draft(reply.clone()))
        .unwrap();
    assert_eq!(store.load_threads(&unit).unwrap().drafts(), [reply.clone()]);
    store
        .update_threads(&unit, |book| {
            book.discard_draft(&reply.target);
            Ok(())
        })
        .unwrap();
    let cancelled = ReviewStore::open(directory.path().join("state"), directory.path())
        .unwrap()
        .load_threads(&unit)
        .unwrap();
    assert!(cancelled.drafts().is_empty());
    assert_eq!(cancelled.threads()[0].messages.len(), 1);
}

#[test]
fn damaged_or_missing_context_never_replaces_the_history() {
    let directory = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let unit = "change".into();
    let mut draft = Draft::start("source.rs".into(), source());
    draft.text = "Question".into();
    store
        .update_threads(&unit, |book| book.post(draft.post()))
        .unwrap();
    let context = std::fs::read_dir(store.repository_dir.join("thread-contexts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let index = store.threads_path(&unit).unwrap();
    let original = std::fs::read(&index).unwrap();
    for damaged in [Some(b"corrupt".as_slice()), None] {
        if let Some(bytes) = damaged {
            std::fs::write(&context, bytes).unwrap();
        } else {
            std::fs::remove_file(&context).unwrap();
        }
        let reopened = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        assert!(reopened.update_threads(&unit, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&index).unwrap(), original);
    }
}
