//! Store checkpoint patches compactly so a later working-copy edit cannot rewrite Explore evidence.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serializer, de::Error, ser::Error as _};

pub(super) fn serialize<S>(patches: &Vec<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut bytes = Vec::new();
    for patch in patches {
        let length = u64::try_from(patch.len()).map_err(S::Error::custom)?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(patch);
    }
    let packed = zstd::stream::encode_all(bytes.as_slice(), 3).map_err(S::Error::custom)?;
    serializer.serialize_str(&STANDARD.encode(packed))
}

pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let encoded = String::deserialize(deserializer)?;
    let packed = STANDARD.decode(encoded).map_err(D::Error::custom)?;
    let bytes = zstd::stream::decode_all(packed.as_slice()).map_err(D::Error::custom)?;
    let mut cursor = bytes.as_slice();
    let mut patches = Vec::new();
    while !cursor.is_empty() {
        let length = cursor
            .get(..8)
            .ok_or_else(|| D::Error::custom("truncated patch length"))?;
        let length = usize::try_from(u64::from_le_bytes(length.try_into().expect("eight bytes")))
            .map_err(D::Error::custom)?;
        cursor = &cursor[8..];
        let patch = cursor
            .get(..length)
            .ok_or_else(|| D::Error::custom("truncated patch"))?;
        patches.push(patch.to_vec());
        cursor = &cursor[length..];
    }
    Ok(patches)
}
