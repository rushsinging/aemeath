use super::constants::SCHEMA_VERSION;
use crate::domain::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MemberLocation {
    Active,
    Archive,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberEnvelope {
    schema_version: u32,
    location: MemberLocation,
    entries: Vec<MemoryEntry>,
}

pub(crate) fn encode_dataset(dataset: &MemoryDataset) -> Result<(Vec<u8>, Vec<u8>), MemoryError> {
    let active = encode_member(MemberLocation::Active, dataset.active())?;
    let archive = encode_member(MemberLocation::Archive, dataset.archive())?;
    Ok((active, archive))
}

pub(crate) fn decode_dataset(
    layer: MemoryLayer,
    active: &[u8],
    archive: &[u8],
) -> Result<MemoryDataset, MemoryOpenError> {
    let active = decode_member(MemberLocation::Active, active)?;
    let archive = decode_member(MemberLocation::Archive, archive)?;
    MemoryDataset::new(layer, active, archive)
}

pub(crate) fn decode_legacy_dataset(
    layer: MemoryLayer,
    active: Option<&[u8]>,
    archive: Option<&[u8]>,
) -> Result<MemoryDataset, MemoryOpenError> {
    fn decode(bytes: Option<&[u8]>) -> Result<Vec<MemoryEntry>, MemoryOpenError> {
        match bytes {
            None => Ok(Vec::new()),
            Some(bytes) => {
                serde_json::from_slice(bytes).map_err(|_| MemoryOpenError::CorruptDataset {
                    message: "legacy memory cannot be decoded".to_string(),
                })
            }
        }
    }

    MemoryDataset::new(layer, decode(active)?, decode(archive)?)
}

fn encode_member(
    location: MemberLocation,
    entries: &[MemoryEntry],
) -> Result<Vec<u8>, MemoryError> {
    let mut entries = entries.to_vec();
    entries.sort_by_key(|entry| entry.id);
    for entry in &mut entries {
        entry.tags.sort();
        entry.tags.dedup();
    }
    serde_json::to_vec(&MemberEnvelope {
        schema_version: SCHEMA_VERSION,
        location,
        entries,
    })
    .map_err(|_| MemoryError::Storage {
        kind: MemoryStorageErrorKind::Serialization,
    })
}

fn decode_member(
    expected_location: MemberLocation,
    bytes: &[u8],
) -> Result<Vec<MemoryEntry>, MemoryOpenError> {
    let envelope: MemberEnvelope =
        serde_json::from_slice(bytes).map_err(|_| MemoryOpenError::CorruptDataset {
            message: "记忆数据无法解码".to_string(),
        })?;
    if envelope.schema_version != SCHEMA_VERSION {
        return Err(MemoryOpenError::UnsupportedSchema {
            version: envelope.schema_version,
        });
    }
    if envelope.location != expected_location {
        return Err(MemoryOpenError::CorruptDataset {
            message: "记忆成员位置不匹配".to_string(),
        });
    }
    Ok(envelope.entries)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
