use fs2::FileExt;
use review_threads::ReviewThreads;
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;

use super::thread_sources::StoredSource;
use super::{Error, Result, ReviewStore, StateKey};

#[cfg(test)]
#[path = "thread_storage.tests.rs"]
mod storage_tests;

#[derive(Deserialize)]
struct Version {
    version: u8,
}

#[derive(Deserialize, Serialize)]
struct StoredThreads<S = std::sync::Arc<review_threads::ThreadSource>> {
    version: u8,
    conversations: ReviewThreads<S>,
}

impl ReviewStore {
    /// Apply a change to the latest history under a lock shared by reviewer processes.
    pub fn update_threads<T>(
        &self,
        review_unit: &ReviewUnit,
        update: impl FnOnce(&mut ReviewThreads) -> std::result::Result<T, String>,
    ) -> Result<(T, ReviewThreads)> {
        let path = self.threads_path(review_unit)?.with_extension("lock");
        self.create_dir(&self.repository_dir.join("conversations"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|source| Error::StateIo {
                operation: "open conversation lock",
                path: path.clone(),
                source,
            })?;
        lock.lock_exclusive().map_err(|source| Error::StateIo {
            operation: "lock conversation",
            path,
            source,
        })?;
        let mut book = self.load_threads(review_unit)?;
        let original = book.clone();
        let result = update(&mut book).map_err(Error::ThreadUpdate)?;
        if book != original {
            self.save_threads(&book)?;
        }
        Ok((result, book))
    }

    /// Persist all conversations for a logical review independently of its current diff.
    fn save_threads(&self, conversations: &ReviewThreads) -> Result<()> {
        let path = self.threads_path(&conversations.review_unit)?;
        self.atomic_compressed_json(
            &path,
            &StoredThreads {
                version: 4,
                conversations: conversations
                    .clone()
                    .try_map_sources(|source| self.save_thread_source(&source))?,
            },
            "write review threads",
        )
    }

    /// Restore conversations even when their original paths no longer exist.
    pub fn load_threads(&self, review_unit: &ReviewUnit) -> Result<ReviewThreads> {
        let path = self.threads_path(review_unit)?;
        let Some(bytes) = Self::read_bytes(&path, "read review threads", None)? else {
            return Ok(ReviewThreads::new(review_unit.clone()));
        };
        let json = Self::decode_thread_json(&path, &bytes)?;
        let version: Version =
            serde_json::from_slice(&json).map_err(|source| Error::StateJson {
                operation: "decode review threads",
                path: path.clone(),
                source,
            })?;
        if version.version == 4 {
            let stored: StoredThreads<StoredSource> =
                serde_json::from_slice(&json).map_err(|source| Error::StateJson {
                    operation: "decode review threads",
                    path,
                    source,
                })?;
            if &stored.conversations.review_unit != review_unit {
                return Err(Error::InvalidStateKey {
                    field: "thread review unit",
                });
            }
            return stored
                .conversations
                .try_map_sources(|source| self.load_thread_source(source));
        }
        let mut stored: StoredThreads =
            serde_json::from_slice(&json).map_err(|source| Error::StateJson {
                operation: "decode review threads",
                path,
                source,
            })?;
        if !matches!(stored.version, 2 | 3) || &stored.conversations.review_unit != review_unit {
            return Err(Error::InvalidStateKey {
                field: "thread storage version or review unit",
            });
        }
        if stored.version == 2 {
            stored.conversations.recover_retrieved_comments();
        }
        stored.conversations.migrate_answered_positions();
        Ok(stored.conversations)
    }

    fn threads_path(&self, review_unit: &ReviewUnit) -> Result<std::path::PathBuf> {
        if review_unit.is_empty() {
            return Err(Error::InvalidStateKey {
                field: "review unit",
            });
        }
        Ok(self.repository_dir.join("conversations").join(format!(
            "{}.json.zst",
            StateKey::hash(review_unit.as_str().as_bytes()).0
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use review_guide::{DiffRangeAnchor, GuideAnchorKind};
    use review_threads::{MessageId, Post};

    #[test]
    fn concurrent_reviewers_append_without_replacing_each_others_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let post = Post::start(
            DiffRangeAnchor {
                source_checkpoint: "original".into(),
                old_path: None,
                new_path: Some("a.rs".into()),
                old_lines: None,
                new_lines: Some(1..2),
                target_kind: GuideAnchorKind::Lines,
                source_hunk_count: 1,
                old_content: None,
                new_content: None,
                diff_hash: String::new(),
            },
            "+original".into(),
            "First".into(),
        );
        let thread = post.thread_id().clone();
        store
            .update_threads(&"change".into(), |book| book.post(post))
            .unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for prefix in ["reviewer one", "reviewer two"] {
                let store = &store;
                let barrier = &barrier;
                let thread = &thread;
                scope.spawn(move || {
                    barrier.wait();
                    for number in 0..5 {
                        store
                            .update_threads(&"change".into(), |book| {
                                book.post(Post::reply(
                                    thread.clone(),
                                    format!("{prefix}: {number}"),
                                ))
                            })
                            .unwrap();
                    }
                });
            }
        });
        let book = store.load_threads(&"change".into()).unwrap();
        assert_eq!(book.threads()[0].messages.len(), 11);
        for prefix in ["reviewer one", "reviewer two"] {
            assert_eq!(
                book.threads()[0]
                    .messages
                    .iter()
                    .filter(|message| message.text.starts_with(prefix))
                    .count(),
                5
            );
        }
        let failed = store.update_threads(&"change".into(), |book| {
            book.post(Post::reply(thread.clone(), "Discard on failure".into()))?;
            Err::<(), _>("could not finish posting".into())
        });
        assert!(failed.is_err());
        assert_eq!(store.load_threads(&"change".into()).unwrap(), book);
    }

    #[test]
    fn threads_and_final_replies_survive_reopening_after_the_source_file_disappears() {
        let temporary = tempfile::TempDir::new().unwrap();
        let repository = temporary.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        let file = repository.join("gone.rs");
        std::fs::write(&file, "original").unwrap();
        let store = ReviewStore::open(temporary.path().join("state"), &repository).unwrap();
        let mut book = ReviewThreads::new("change".into());
        let post = Post::start(
            DiffRangeAnchor {
                source_checkpoint: "original".into(),
                old_path: None,
                new_path: Some("gone.rs".into()),
                old_lines: None,
                new_lines: Some(0..1),
                target_kind: GuideAnchorKind::Lines,
                source_hunk_count: 1,
                old_content: None,
                new_content: Some(vec![b'x'; 2_000_000]),
                diff_hash: String::new(),
            },
            "+original".into(),
            "Keep this".into(),
        );
        let thread = post.thread_id().clone();
        book.post(post).unwrap();
        book.post(Post::agent_reply(
            thread.clone(),
            MessageId::parse("bf9d0fca-e70c-46c0-b039-f7385dfc45d2").unwrap(),
            "Final answer".into(),
        ))
        .unwrap();
        book.set_resolution(&thread, review_threads::Resolution::Resolved)
            .unwrap();
        book.mark_read(&thread, book.sequence()).unwrap();
        store.save_threads(&book).unwrap();
        std::fs::remove_file(file).unwrap();
        let reopened = ReviewStore::open(temporary.path().join("state"), &repository).unwrap();
        assert_eq!(reopened.load_threads(&book.review_unit).unwrap(), book);
        assert_eq!(
            reopened
                .load_threads(&"another-change".into())
                .unwrap()
                .threads()
                .len(),
            0
        );
    }

    #[test]
    fn legacy_recovery_only_delivers_comments_still_waiting_for_an_answer() {
        for late_before_reply in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let store =
                ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
            let mut book = ReviewThreads::new("change".into());
            let post = Post::start(
                DiffRangeAnchor {
                    source_checkpoint: "original".into(),
                    old_path: None,
                    new_path: Some("file.rs".into()),
                    old_lines: None,
                    new_lines: Some(0..1),
                    target_kind: GuideAnchorKind::Lines,
                    source_hunk_count: 1,
                    old_content: None,
                    new_content: Some(b"retained source".to_vec()),
                    diff_hash: String::new(),
                },
                "+retained source".into(),
                "Already answered".into(),
            );
            let thread = post.thread_id().clone();
            book.post(post).unwrap();
            if late_before_reply {
                book.post(Post::reply(thread.clone(), "Fetched but unanswered".into()))
                    .unwrap();
            }
            book.post(Post::agent_reply(
                thread.clone(),
                MessageId::parse("1f27f5d0-0e04-4a13-aa1a-ef8dbf96366c").unwrap(),
                "Existing answer".into(),
            ))
            .unwrap();
            if !late_before_reply {
                book.post(Post::reply(thread.clone(), "Fetched but unanswered".into()))
                    .unwrap();
            }
            let mut legacy = serde_json::to_value(&book).unwrap();
            legacy["readers"] = serde_json::json!({"agent": {serde_json::to_value(&thread).unwrap().as_str().unwrap(): book.sequence()}});
            let path = store.threads_path(&book.review_unit).unwrap();
            store
                .atomic_compressed_json(
                    &path,
                    &serde_json::json!({"version": 2, "conversations": legacy}),
                    "write fixture",
                )
                .unwrap();
            let recovered = store.load_threads(&book.review_unit).unwrap();
            assert_eq!(recovered.threads(), book.threads());
            assert_eq!(
                recovered.new_messages().len(),
                usize::from(!late_before_reply)
            );
            assert_eq!(recovered.has_new_messages(), !late_before_reply);
            if late_before_reply {
                // Without an exact boundary, legacy replies cover preceding
                // comments, just as the thread's visible Waiting status does.
                assert!(!recovered.thread(&thread).unwrap().is_waiting());
                continue;
            }
            let comment = recovered
                .thread(&thread)
                .unwrap()
                .last_comment()
                .unwrap()
                .id
                .clone();
            store
                .update_threads(&book.review_unit, |book| {
                    book.answer(Post::answer(
                        thread.clone(),
                        MessageId::parse("b5c06df5-6b11-4134-9fb7-d18b4c310097").unwrap(),
                        "Recovered answer".into(),
                        comment,
                    ))
                })
                .unwrap();
            let saved = store.load_threads(&book.review_unit).unwrap();
            assert_eq!(saved.threads()[0].messages.len(), 4);
            assert_eq!(saved.threads()[0].anchor, book.threads()[0].anchor);
            assert!(!saved.has_new_messages());
            let stored: serde_json::Value = serde_json::from_slice(
                &zstd::stream::decode_all(std::fs::read(path).unwrap().as_slice()).unwrap(),
            )
            .unwrap();
            assert_eq!(stored["version"], 4);
        }
    }

    #[test]
    fn corrupt_history_is_reported_instead_of_replaced_with_empty_threads() {
        let temporary = tempfile::TempDir::new().unwrap();
        let store = ReviewStore::open(temporary.path().join("state"), temporary.path()).unwrap();
        let book = ReviewThreads::new("change".into());
        store.save_threads(&book).unwrap();
        let path = store.threads_path(&book.review_unit).unwrap();
        std::fs::write(&path, "invalid compressed history").unwrap();
        assert!(store.load_threads(&book.review_unit).is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "invalid compressed history"
        );
    }
}
