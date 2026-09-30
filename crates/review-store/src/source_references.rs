//! Stored documents reference each thread source by its context file instead of carrying
//! it. The domain types serialize their sources inline, so the store swaps each source for
//! a stand-in before it serializes a value, then writes a reference where each stand-in
//! was. Loading reverses the swap, and equal sources keep sharing one loaded copy.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::ThreadSource;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::thread_sources::StoredSource;
use super::{Error, Result, ReviewStore};

/// Where a stored record keeps its source: merged into the record's own fields, as a
/// thread does, or under a field of its own, as a draft does.
#[derive(Clone, Copy)]
pub(super) enum SourceSlot {
    Merged,
    Field(&'static str),
}

/// Whether a stored document carries each source inline, as thread documents before
/// version 4 did, or a reference to the source's context file.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum SourceForm {
    Inline,
    Referenced,
}

/// A domain value whose sources the store keeps in context files.
pub(super) trait WithSources: Clone + Serialize + DeserializeOwned {
    const SLOT: SourceSlot;

    /// Every source the value serializes, in the order it serializes them.
    fn sources_mut(&mut self) -> impl Iterator<Item = &mut Arc<ThreadSource>>;

    /// The records that hold one source each, within the value's JSON.
    fn records(json: &mut Value) -> Option<&mut Vec<Value>>;
}

/// Takes a source's place while a document is converted. Stored documents never contain
/// it: the conversion replaces it with a reference, or a loaded source replaces it.
struct StandIn {
    source: Arc<ThreadSource>,
    fields: Map<String, Value>,
    json: Vec<u8>,
}

static STAND_IN: LazyLock<StandIn> = LazyLock::new(|| {
    let source = Arc::new(ThreadSource {
        anchor: DiffRangeAnchor {
            source_checkpoint: "stand-in for a stored source reference".into(),
            old_path: None,
            new_path: None,
            old_lines: None,
            new_lines: None,
            target_kind: AnchorKind::default(),
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: String::new(),
        },
        excerpt: String::new(),
    });
    let Ok(Value::Object(fields)) = serde_json::to_value(&*source) else {
        unreachable!("a thread source serializes as a JSON object");
    };
    let json = serde_json::to_vec(&*source).expect("a thread source serializes");
    StandIn {
        source,
        fields,
        json,
    }
});

impl SourceSlot {
    /// The bytes a record's source, serialized as the object `object`, occupies in it.
    fn bytes(self, object: &[u8]) -> &[u8] {
        match self {
            // A merged source contributes its fields, without its own braces.
            Self::Merged => &object[1..object.len() - 1],
            Self::Field(_) => object,
        }
    }

    /// Replace the reference that `record` holds with the stand-in.
    fn take_reference(self, record: &mut Value) -> serde_json::Result<StoredSource> {
        let invalid = || serde::de::Error::custom("a stored record is not an object");
        match self {
            Self::Merged => {
                let reference = StoredSource::deserialize(&*record)?;
                let object = record.as_object_mut().ok_or_else(invalid)?;
                if let Value::Object(fields) = serde_json::to_value(&reference)? {
                    for key in fields.keys() {
                        object.remove(key);
                    }
                }
                object.extend(STAND_IN.fields.clone());
                Ok(reference)
            }
            Self::Field(name) => {
                let field = record
                    .as_object_mut()
                    .ok_or_else(invalid)?
                    .get_mut(name)
                    .ok_or_else(|| serde::de::Error::missing_field(name))?;
                let reference = StoredSource::deserialize(&*field)?;
                *field = Value::Object(STAND_IN.fields.clone());
                Ok(reference)
            }
        }
    }
}

/// A copy of a value with each source swapped for the stand-in, and the reference to each
/// swapped source's context file, in source order.
pub(super) struct Detached<T> {
    pub(super) value: T,
    references: Vec<StoredSource>,
}

impl<T: WithSources> Detached<T> {
    /// The JSON of `document`, which serializes this value once, with each stand-in
    /// replaced by the reference to the source it took the place of.
    fn json(&self, document: &impl Serialize) -> serde_json::Result<Vec<u8>> {
        let json = serde_json::to_vec(document)?;
        let stand_in = T::SLOT.bytes(&STAND_IN.json);
        let mut references = self.references.iter();
        let mut written = Vec::with_capacity(json.len());
        let mut rest = json.as_slice();
        // The stand-in cannot occur inside a JSON string, where every quote is escaped.
        while let Some(at) = rest
            .windows(stand_in.len())
            .position(|window| window == stand_in)
        {
            let reference = references.next().ok_or_else(Self::mismatch)?;
            written.extend_from_slice(&rest[..at]);
            written.extend_from_slice(T::SLOT.bytes(&serde_json::to_vec(reference)?));
            rest = &rest[at + stand_in.len()..];
        }
        if references.next().is_some() {
            return Err(Self::mismatch());
        }
        written.extend_from_slice(rest);
        Ok(written)
    }

    fn mismatch() -> serde_json::Error {
        serde::ser::Error::custom("the document does not hold each detached source once")
    }
}

impl ReviewStore {
    /// Copy `value` with its sources swapped for the stand-in, saving each source's
    /// context file.
    pub(super) fn detach<T: WithSources>(&self, value: &T) -> Result<Detached<T>> {
        let mut value = value.clone();
        let references = value
            .sources_mut()
            .map(|source| {
                let reference = self.save_thread_source(source)?;
                *source = STAND_IN.source.clone();
                Ok(reference)
            })
            .collect::<Result<_>>()?;
        Ok(Detached { value, references })
    }

    /// Atomically write `document`, which serializes `detached.value`, with a reference in
    /// place of each detached source.
    pub(super) fn atomic_referencing_json<T: WithSources>(
        &self,
        target: &Path,
        detached: &Detached<T>,
        document: &impl Serialize,
        operation: &'static str,
    ) -> Result<()> {
        let json = detached
            .json(document)
            .map_err(Error::json(operation, target))?;
        self.atomic_compressed_bytes(target, &json, operation)
    }

    /// Decode a value from its stored JSON, restoring every source it references.
    pub(super) fn attach<T: WithSources>(
        &self,
        mut json: Value,
        form: SourceForm,
        path: &Path,
        operation: &'static str,
    ) -> Result<T> {
        let decode = Error::json(operation, path);
        let references = match (form, T::records(&mut json)) {
            (SourceForm::Referenced, Some(records)) => records
                .iter_mut()
                .map(|record| T::SLOT.take_reference(record))
                .collect::<serde_json::Result<Vec<_>>>()
                .map_err(&decode)?,
            _ => Vec::new(),
        };
        let mut value: T = serde_json::from_value(json).map_err(&decode)?;
        if form == SourceForm::Referenced {
            let mut references = references.into_iter();
            for source in value.sources_mut() {
                let reference = references.next().ok_or_else(|| {
                    decode(serde::de::Error::custom("a stored source has no reference"))
                })?;
                *source = self.load_thread_source(reference)?;
            }
        }
        Ok(value)
    }
}
