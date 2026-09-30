use std::sync::Arc;

use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::{Post, ThreadSource};

use super::*;

fn unit() -> ReviewUnit {
    "change".into()
}

fn new_thread(path: &str) -> Draft {
    Draft::start(
        path.into(),
        Arc::new(ThreadSource {
            excerpt: "+line".into(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "checkpoint".into(),
                old_path: None,
                new_path: Some(path.into()),
                old_lines: None,
                new_lines: Some(0..1),
                target_kind: AnchorKind::Lines,
                source_hunk_count: 0,
                old_content: None,
                new_content: Some(b"line\n".to_vec()),
                diff_hash: String::new(),
            },
        }),
    )
}

fn typed(drafts: &mut Drafts, id: DraftId, text: &str) -> Vec<ThreadCommand> {
    text.chars()
        .filter_map(|character| drafts.input(id, Key::Char(character)))
        .collect()
}

fn saved(commands: &[ThreadCommand]) -> Option<&Draft> {
    commands.iter().rev().find_map(|command| match command {
        ThreadCommand::SaveDraft { draft, .. } => Some(draft),
        _ => None,
    })
}

fn posted(submission: Option<Submission>) -> Post {
    match submission {
        Some(Submission::Posting(ThreadCommand::Post { post, .. })) => post,
        other => panic!("expected a post, got {other:?}"),
    }
}

/// A saved review holding `drafts`, as the thread owner would load it.
fn book_with(drafts: &[&Draft]) -> ReviewThreads {
    let mut book = ReviewThreads::new(unit());
    for draft in drafts {
        book.save_draft((*draft).clone()).unwrap();
    }
    book
}

fn texts(drafts: &Drafts) -> Vec<String> {
    drafts
        .in_review(&unit())
        .map(|(_, open)| open.editor().text())
        .collect()
}

#[test]
fn every_edit_saves_the_draft_and_a_blank_draft_disappears_when_parked() {
    let mut drafts = Drafts::default();
    let id = drafts.start(unit(), new_thread("lib.rs"));
    let commands = typed(&mut drafts, id, "Hi");
    assert_eq!(commands.len(), 2);
    assert_eq!(saved(&commands).unwrap().text, "Hi");
    drafts.park(id);
    assert_eq!(texts(&drafts), ["Hi"]);

    let blank = drafts.start(unit(), new_thread("lib.rs"));
    drafts.park(blank);
    assert!(drafts.get(blank).is_none());
    assert_eq!(texts(&drafts), ["Hi"]);
}

#[test]
fn a_file_holds_several_drafts_and_a_thread_holds_one_reply() {
    let mut drafts = Drafts::default();
    let first = drafts.start(unit(), new_thread("lib.rs"));
    let second = drafts.start(unit(), new_thread("lib.rs"));
    typed(&mut drafts, first, "First");
    typed(&mut drafts, second, "Second");
    assert_eq!(texts(&drafts), ["First", "Second"]);

    let mut book = ReviewThreads::new(unit());
    let post = posted(drafts.submit(first));
    book.post(post.clone()).unwrap();
    drafts.post_succeeded(&unit(), &post.message().id);
    let thread = book.thread(post.thread_id()).unwrap();
    let reply = drafts
        .start_reply(unit(), thread, post.message().id.clone())
        .unwrap();
    assert!(
        drafts
            .start_reply(unit(), thread, post.message().id.clone())
            .is_none(),
        "a thread holds one reply"
    );
    assert_eq!(drafts.for_thread(&unit(), &thread.id), Some(reply));
}

#[test]
fn submitting_posts_once_and_the_acknowledgement_settles_the_draft() {
    let mut drafts = Drafts::default();
    let id = drafts.start(unit(), new_thread("lib.rs"));
    typed(&mut drafts, id, "Question");
    let post = posted(drafts.submit(id));
    assert_eq!(post.message().text, "Question");
    assert!(drafts.get(id).unwrap().is_posting());
    assert!(
        drafts.submit(id).is_none(),
        "a post in flight is not repeated"
    );
    assert!(
        typed(&mut drafts, id, "x").is_empty(),
        "posting text is frozen"
    );
    assert!(drafts.cancel(id).is_none());

    drafts.post_failed(&unit(), &post.message().id);
    assert!(!drafts.get(id).unwrap().is_posting());
    let retried = posted(drafts.submit(id));
    assert_eq!(retried, post, "a retry keeps the publication identity");

    drafts.post_succeeded(&unit(), &post.message().id);
    assert!(drafts.get(id).is_none());
    assert_eq!(drafts.posted_as(id), Some(&post.message().id));
}

#[test]
fn blank_submission_and_cancel_discard_the_saved_draft() {
    let mut drafts = Drafts::default();
    let blank = drafts.start(unit(), new_thread("lib.rs"));
    typed(&mut drafts, blank, "  ");
    let Some(Submission::Cancelled(ThreadCommand::DiscardDraft { thread_id, .. })) =
        drafts.submit(blank)
    else {
        panic!("blank text cancels");
    };
    assert!(drafts.get(blank).is_none());
    assert_eq!(drafts.posted_as(blank), None);

    let id = drafts.start(unit(), new_thread("lib.rs"));
    typed(&mut drafts, id, "Unwanted");
    let thread = drafts.get(id).unwrap().draft().thread_id().clone();
    assert_ne!(thread, thread_id);
    assert_eq!(
        drafts.cancel(id),
        Some(ThreadCommand::DiscardDraft {
            review_unit: unit(),
            thread_id: thread,
        })
    );
    assert!(drafts.get(id).is_none());
}

#[test]
fn saved_drafts_are_recovered_once_and_cancelled_ones_stay_gone() {
    let mut original = Drafts::default();
    let kept = original.start(unit(), new_thread("lib.rs"));
    let commands = typed(&mut original, kept, "Recovered");
    let recovered = saved(&commands).unwrap().clone();
    let mut blank = new_thread("lib.rs");
    blank.text = " ".into();
    let book = book_with(&[&recovered, &blank]);

    let mut restarted = Drafts::default();
    restarted.recover(&book);
    restarted.recover(&book);
    assert_eq!(
        texts(&restarted),
        ["Recovered"],
        "blank drafts are not reopened"
    );
    let (id, open) = restarted.in_review(&unit()).next().unwrap();
    assert_eq!(open.draft(), &recovered);
    assert_eq!(
        posted(restarted.submit(id)),
        recovered.post(),
        "recovery keeps the publication identity"
    );

    let mut cancelling = Drafts::default();
    cancelling.recover(&book);
    let (id, _) = cancelling.in_review(&unit()).next().unwrap();
    cancelling.cancel(id).unwrap();
    cancelling.recover(&book);
    assert!(
        texts(&cancelling).is_empty(),
        "a stale review cannot resurrect it"
    );

    let mut other_review = Drafts::default();
    other_review.recover(&book);
    assert!(other_review.in_review(&"other".into()).next().is_none());
}

#[test]
fn a_draft_posted_elsewhere_settles_or_is_renewed_with_its_edits() {
    for edited in [false, true] {
        let mut drafts = Drafts::default();
        let id = drafts.start(unit(), new_thread("lib.rs"));
        let draft = saved(&typed(&mut drafts, id, "Shared")).unwrap().clone();
        let stale = book_with(&[&draft]);
        if edited {
            typed(&mut drafts, id, " and more");
        }
        let mut book = ReviewThreads::new(unit());
        book.post(draft.post()).unwrap();

        let commands = drafts.reconcile(&book);
        drafts.recover(&stale);
        if edited {
            let renewed = saved(&commands).expect("the edited text is saved again");
            assert_ne!(renewed.message_id(), draft.message_id());
            assert_ne!(renewed.thread_id(), draft.thread_id());
            assert_eq!(renewed.text, "Shared and more");
            assert_eq!(drafts.get(id).unwrap().draft(), renewed);
            assert_eq!(texts(&drafts), ["Shared and more"]);
            book.post(posted(drafts.submit(id)))
                .expect("the renewed draft publishes under its own identity");
        } else {
            assert!(commands.is_empty());
            assert_eq!(drafts.posted_as(id), Some(draft.message_id()));
            assert!(
                texts(&drafts).is_empty(),
                "the posted draft is not recovered"
            );
        }
    }
}

#[test]
fn drafts_belong_to_their_review() {
    let mut drafts = Drafts::default();
    let id = drafts.start(unit(), new_thread("lib.rs"));
    typed(&mut drafts, id, "Private");
    assert!(drafts.in_review(&"other".into()).next().is_none());
    let message = drafts.get(id).unwrap().draft().message_id().clone();
    drafts.post_succeeded(&"other".into(), &message);
    assert_eq!(texts(&drafts), ["Private"]);
}
