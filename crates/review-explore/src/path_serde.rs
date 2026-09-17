//! Normal citations use strings; byte arrays preserve non-UTF-8 Unix paths.
use review_repository::repository::RepoPath;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum PathInput {
    Text(String),
    Bytes(Vec<u8>),
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<RepoPath, D::Error> {
    Ok(RepoPath::from_bytes(match PathInput::deserialize(input)? {
        PathInput::Text(text) => text.into_bytes(),
        PathInput::Bytes(bytes) => bytes,
    }))
}

pub(crate) fn serialize<S: Serializer>(path: &RepoPath, output: S) -> Result<S::Ok, S::Error> {
    match std::str::from_utf8(path.as_bytes()) {
        Ok(text) => text.serialize(output),
        Err(_) => path.as_bytes().serialize(output),
    }
}
