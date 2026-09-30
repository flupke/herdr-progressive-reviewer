//! Drafts live in a small document of their own, apart from the posted thread document.
//! Saving one never rewrites posted messages.

use std::path::PathBuf;

use review_threads::{ReviewThreads, SavedDrafts};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

use super::thread_sources::StoredSource;
use super::{Error, Result, ReviewStore};

#[cfg(test)]
#[path = "drafts.tests.rs"]
mod tests;

const VERSION: u8 = 1;

#[derive(Deserialize, Serialize)]
#[serde(bound(deserialize = "S: Deserialize<'de>", serialize = "S: Serialize"))]
struct StoredDrafts<S> {
    version: u8,
    review_unit: ReviewUnit,
    drafts: SavedDrafts<S>,
}

impl ReviewStore {
    /// The drafts saved for a review. A draft whose post reached the threads is never
    /// returned, including when posting was interrupted before it discarded the draft.
    pub fn load_drafts(&self, review_unit: &ReviewUnit) -> Result<SavedDrafts> {
        let threads = self.load_threads(review_unit)?;
        let mut drafts = self.read_drafts(review_unit)?;
        drafts.forget_posted(&threads);
        Ok(drafts)
    }

    /// Apply a change to the saved drafts under a lock shared by reviewer processes.
    /// The threads are read to check the change and are never written.
    pub fn update_drafts<T>(
        &self,
        review_unit: &ReviewUnit,
        update: impl FnOnce(&mut SavedDrafts, &ReviewThreads) -> std::result::Result<T, String>,
    ) -> Result<(T, SavedDrafts)> {
        // Loading the threads can migrate drafts, which takes the draft lock itself.
        let threads = self.load_threads(review_unit)?;
        let _lock = self.drafts_lock(review_unit)?;
        let mut drafts = self.read_drafts(review_unit)?;
        let original = drafts.clone();
        drafts.forget_posted(&threads);
        let result = update(&mut drafts, &threads).map_err(Error::ThreadUpdate)?;
        if drafts != original {
            self.save_drafts(review_unit, &drafts)?;
        }
        Ok((result, drafts))
    }

    /// Move drafts that an earlier build saved inside the thread document into the draft
    /// store. Only such a build writes them there, so they replace the stored draft of the
    /// same thread. Adopting the same drafts again changes nothing.
    pub(super) fn adopt_embedded_drafts(
        &self,
        review_unit: &ReviewUnit,
        embedded: SavedDrafts,
    ) -> Result<()> {
        let _lock = self.drafts_lock(review_unit)?;
        let mut drafts = self.read_drafts(review_unit)?;
        let original = drafts.clone();
        for draft in embedded {
            drafts.keep(draft);
        }
        if drafts != original {
            self.save_drafts(review_unit, &drafts)?;
        }
        Ok(())
    }

    fn read_drafts(&self, review_unit: &ReviewUnit) -> Result<SavedDrafts> {
        let path = self.drafts_path(review_unit)?;
        let Some(bytes) = Self::read_bytes(&path, "read drafts", None)? else {
            return Ok(SavedDrafts::default());
        };
        let json = Self::decode_thread_json(&path, &bytes)?;
        let stored: StoredDrafts<StoredSource> =
            serde_json::from_slice(&json).map_err(|source| Error::StateJson {
                operation: "decode drafts",
                path,
                source,
            })?;
        if stored.version != VERSION || &stored.review_unit != review_unit {
            return Err(Error::InvalidStateKey {
                field: "draft storage version or review unit",
            });
        }
        stored
            .drafts
            .try_map_sources(|source| self.load_thread_source(source))
    }

    fn save_drafts(&self, review_unit: &ReviewUnit, drafts: &SavedDrafts) -> Result<()> {
        self.atomic_compressed_json(
            &self.drafts_path(review_unit)?,
            &StoredDrafts {
                version: VERSION,
                review_unit: review_unit.clone(),
                drafts: drafts
                    .clone()
                    .try_map_sources(|source| self.save_thread_source(&source))?,
            },
            "write drafts",
        )
    }

    fn drafts_lock(&self, review_unit: &ReviewUnit) -> Result<std::fs::File> {
        self.exclusive_lock(
            &self.drafts_path(review_unit)?.with_extension("lock"),
            "lock drafts",
        )
    }

    fn drafts_path(&self, review_unit: &ReviewUnit) -> Result<PathBuf> {
        self.review_record_path("drafts", review_unit)
    }
}
