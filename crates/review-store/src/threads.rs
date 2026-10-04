use std::sync::Arc;

use review_threads::{ReviewThreads, SavedDrafts, ThreadSource};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::source_references::{SourceForm, SourceSlot, WithSources};
use super::{Error, Result, ReviewStore};

mod legacy;

#[cfg(test)]
#[path = "thread_storage.tests.rs"]
mod storage_tests;

#[cfg(test)]
#[path = "thread_formats.tests.rs"]
mod format_tests;

/// Version 4 stays the written version: builds that predate the separate draft store
/// still load this document, and simply see no drafts in it.
const VERSION: u8 = 4;

/// The thread document: the conversations are borrowed to write it, and stay JSON until
/// they are migrated and their sources restored when reading it.
#[derive(Deserialize, Serialize)]
struct StoredThreads<Conversations> {
    version: u8,
    conversations: Conversations,
}

/// A thread document version this build reads.
#[derive(Clone, Copy)]
enum StoredVersion {
    /// Versions 2 and 3 carry sources inline and track delivery per recipient.
    Legacy(u8),
    Current,
}

impl StoredVersion {
    fn parse(version: u8) -> Result<Self> {
        match version {
            VERSION => Ok(Self::Current),
            2 | 3 => Ok(Self::Legacy(version)),
            _ => Err(Error::InvalidStateKey {
                field: "thread storage version or review unit",
            }),
        }
    }

    fn source_form(self) -> SourceForm {
        match self {
            Self::Legacy(_) => SourceForm::Inline,
            Self::Current => SourceForm::Referenced,
        }
    }

    fn foreign_review_unit(self) -> Error {
        Error::InvalidStateKey {
            field: match self {
                Self::Legacy(_) => "thread storage version or review unit",
                Self::Current => "thread review unit",
            },
        }
    }
}

/// The review a stored thread document belongs to.
#[derive(Deserialize)]
struct Owner {
    review_unit: ReviewUnit,
}

impl WithSources for ReviewThreads {
    const SLOT: SourceSlot = SourceSlot::Merged;

    fn sources_mut(&mut self) -> impl Iterator<Item = &mut Arc<ThreadSource>> {
        ReviewThreads::sources_mut(self)
    }

    fn records(json: &mut Value) -> Vec<&mut Value> {
        json.get_mut("threads")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .collect()
    }
}

/// A decoded thread document, with any drafts it still carries from earlier builds.
struct ThreadDocument {
    threads: ReviewThreads,
    embedded_drafts: SavedDrafts,
}

impl ReviewStore {
    pub(super) fn atomic_compressed_bytes(
        &self,
        target: &std::path::Path,
        json: &[u8],
        operation: &'static str,
    ) -> Result<()> {
        let bytes = zstd::stream::encode_all(json, 3).map_err(|source| Error::StateIo {
            operation,
            path: target.to_owned(),
            source,
        })?;
        self.atomic_write(target, &bytes, operation)
    }

    /// Apply a change to the latest history under a lock shared by reviewer processes.
    pub fn update_threads<T>(
        &self,
        review_unit: &ReviewUnit,
        update: impl FnOnce(&mut ReviewThreads) -> std::result::Result<T, String>,
    ) -> Result<(T, ReviewThreads)> {
        let path = self.threads_path(review_unit)?.with_extension("lock");
        let _lock = self.exclusive_lock(&path, "lock conversation")?;
        let document = self.read_thread_document(review_unit)?;
        let migrating = !document.embedded_drafts.is_empty();
        if migrating {
            // The draft store is written first: an interruption before the thread document
            // is rewritten leaves the drafts in both places, and the next load merges the
            // same drafts again.
            self.adopt_embedded_drafts(review_unit, document.embedded_drafts)?;
        }
        let mut book = document.threads;
        let original = book.clone();
        let result = update(&mut book).map_err(Error::ThreadUpdate)?;
        if migrating || book != original {
            self.save_threads(&book)?;
        }
        Ok((result, book))
    }

    /// Persist all conversations for a logical review independently of its current diff.
    fn save_threads(&self, conversations: &ReviewThreads) -> Result<()> {
        let path = self.threads_path(&conversations.review_unit)?;
        let detached = self.detach(conversations)?;
        self.atomic_referencing_json(
            &path,
            &detached,
            &StoredThreads {
                version: VERSION,
                conversations: &detached.value,
            },
            "write review threads",
        )
    }

    /// Restore conversations even when their original paths no longer exist. The first
    /// load of a document that still carries drafts moves them to the draft store.
    pub fn load_threads(&self, review_unit: &ReviewUnit) -> Result<ReviewThreads> {
        let document = self.read_thread_document(review_unit)?;
        if document.embedded_drafts.is_empty() {
            return Ok(document.threads);
        }
        // A draft store this build cannot read must not hide the threads. The drafts stay
        // in the thread document, and changing the threads reports the problem instead.
        Ok(self
            .update_threads(review_unit, |_| Ok(()))
            .map_or(document.threads, |((), threads)| threads))
    }

    fn read_thread_document(&self, review_unit: &ReviewUnit) -> Result<ThreadDocument> {
        const OPERATION: &str = "decode review threads";
        let path = self.threads_path(review_unit)?;
        let Some(bytes) = Self::read_bytes(&path, "read review threads", None)? else {
            return Ok(ThreadDocument {
                threads: ReviewThreads::new(review_unit.clone()),
                embedded_drafts: SavedDrafts::default(),
            });
        };
        let json = Self::decode_thread_json(&path, &bytes)?;
        let decode = Error::json(OPERATION, &path);
        let stored: StoredThreads<Value> = serde_json::from_slice(&json).map_err(&decode)?;
        let version = StoredVersion::parse(stored.version)?;
        let mut conversations = stored.conversations;
        let owner = Owner::deserialize(&conversations).map_err(&decode)?;
        if &owner.review_unit != review_unit {
            return Err(version.foreign_review_unit());
        }
        // Earlier builds kept drafts inside the thread document, in the same source form.
        let embedded = conversations
            .as_object_mut()
            .and_then(|fields| fields.remove("drafts"));
        if let StoredVersion::Legacy(number) = version {
            legacy::migrate(&mut conversations, number).map_err(&decode)?;
        }
        let form = version.source_form();
        Ok(ThreadDocument {
            threads: self.attach(conversations, form, &path, OPERATION)?,
            embedded_drafts: match embedded {
                Some(drafts) => self.attach(drafts, form, &path, OPERATION)?,
                None => SavedDrafts::default(),
            },
        })
    }

    fn threads_path(&self, review_unit: &ReviewUnit) -> Result<std::path::PathBuf> {
        self.review_record_path("conversations", review_unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use review_source::{AnchorKind, DiffRangeAnchor};
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
                target_kind: AnchorKind::Lines,
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
                target_kind: AnchorKind::Lines,
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
