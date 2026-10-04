use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::{Draft, MessageId, Post, ThreadSource};

use super::*;
use crate::stored_fixtures::code_source;

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
        code_source(&first.threads()[0]),
        code_source(&second.threads()[0])
    ));
    assert!(Arc::ptr_eq(
        code_source(&first.threads()[0]),
        code_source(&third.threads()[0])
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

#[test]
fn a_round_conversation_is_stored_between_threads_on_code_and_loads_again() {
    let directory = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let unit: ReviewUnit = "change".into();
    let mut code = Draft::start("source.rs".into(), source());
    code.text = "On code".into();
    let mut later = Draft::start("source.rs".into(), source());
    later.text = "Later on code".into();
    let round = Post::to_round(
        "round-1",
        "Beside the question".into(),
        Some(review_threads::AskedUnder::Question {
            question: "q-1".into(),
            version: 2,
            number: None,
        }),
        Some("a quoted passage".into()),
    );
    let ((), written) = store
        .update_threads(&unit, |book| {
            book.post(code.post())?;
            book.post(round)?;
            book.post(later.post())?;
            Ok(())
        })
        .unwrap();

    let reopened = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let loaded = reopened.load_threads(&unit).unwrap();
    assert_eq!(loaded, written);
    let conversation = loaded.round_conversation("round-1").unwrap();
    assert_eq!(
        conversation.messages[0].quote.as_deref(),
        Some("a quoted passage")
    );
    assert_eq!(code_source(&loaded.threads()[2]).excerpt, "+source");
}
