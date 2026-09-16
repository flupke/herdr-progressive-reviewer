//! Immutable context is compressed once, independently of mutable conversations.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use review_threads::ThreadSource;
use serde::{Deserialize, Serialize};

use crate::{Error, Result, ReviewStore, StateKey};

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct StoredSource {
    context: String,
}

#[derive(Debug, Default)]
pub(super) struct SourceCache {
    loaded: HashMap<String, Weak<ThreadSource>>,
    saved: HashMap<usize, (Weak<ThreadSource>, String)>,
}

impl SourceCache {
    fn insert(&mut self, key: String, source: &Arc<ThreadSource>) {
        self.loaded.retain(|_, source| source.strong_count() > 0);
        self.saved
            .retain(|_, (source, _)| source.strong_count() > 0);
        let weak = Arc::downgrade(source);
        self.loaded.insert(key.clone(), weak.clone());
        self.saved.insert(Arc::as_ptr(source) as usize, (weak, key));
    }
}

impl ReviewStore {
    pub(super) fn save_thread_source(&self, source: &Arc<ThreadSource>) -> Result<StoredSource> {
        let cached = self
            .thread_sources
            .lock()
            .expect("source cache poisoned")
            .saved
            .get(&(Arc::as_ptr(source) as usize))
            .filter(|(weak, _)| {
                weak.upgrade()
                    .is_some_and(|saved| Arc::ptr_eq(&saved, source))
            })
            .map(|(_, key)| key.clone());
        if let Some(context) = cached {
            return Ok(StoredSource { context });
        }
        let json =
            serde_json::to_vec(source).map_err(|error| Error::ThreadUpdate(error.to_string()))?;
        let context = StateKey::hash(&json).0;
        let path = self.thread_source_path(&context)?;
        if let Some(existing) = Self::read_bytes(&path, "read original thread context", None)? {
            let decoded = Self::decode_thread_json(&path, &existing)?;
            if decoded != json {
                return Err(Error::StateCollision { path });
            }
        } else {
            let bytes =
                zstd::stream::encode_all(json.as_slice(), 3).map_err(|source| Error::StateIo {
                    operation: "compress original thread context",
                    path: path.clone(),
                    source,
                })?;
            self.atomic_write(&path, &bytes, "write original thread context")?;
        }
        self.thread_sources
            .lock()
            .expect("source cache poisoned")
            .insert(context.clone(), source);
        Ok(StoredSource { context })
    }

    pub(super) fn load_thread_source(&self, stored: StoredSource) -> Result<Arc<ThreadSource>> {
        let path = self.thread_source_path(&stored.context)?;
        if let Some(source) = self
            .thread_sources
            .lock()
            .expect("source cache poisoned")
            .loaded
            .get(&stored.context)
            .and_then(Weak::upgrade)
        {
            return Ok(source);
        }
        let bytes =
            Self::read_bytes(&path, "read original thread context", None)?.ok_or_else(|| {
                Error::ThreadUpdate(format!(
                    "Missing original thread context at {}",
                    path.display()
                ))
            })?;
        let json = Self::decode_thread_json(&path, &bytes)?;
        if StateKey::hash(&json).0 != stored.context {
            return Err(Error::StateCollision { path });
        }
        let source =
            Arc::new(
                serde_json::from_slice(&json).map_err(|source| Error::StateJson {
                    operation: "decode original thread context",
                    path,
                    source,
                })?,
            );
        self.thread_sources
            .lock()
            .expect("source cache poisoned")
            .insert(stored.context, &source);
        Ok(source)
    }

    fn thread_source_path(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidStateKey {
                field: "thread context",
            });
        }
        Ok(self
            .repository_dir
            .join("thread-contexts")
            .join(format!("{key}.json.zst")))
    }

    pub(super) fn decode_thread_json(path: &std::path::Path, bytes: &[u8]) -> Result<Vec<u8>> {
        zstd::stream::decode_all(bytes).map_err(|source| Error::StateIo {
            operation: "decode review threads",
            path: path.to_owned(),
            source,
        })
    }
}
