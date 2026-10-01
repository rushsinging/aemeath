use super::*;

fn dataset() -> MemoryDataset {
    let mut entry = MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "codec round trip",
        MemorySource::User,
    )
    .unwrap();
    entry.tags = vec!["z".to_string(), "a".to_string(), "a".to_string()];
    MemoryDataset::new(MemoryLayer::Project, vec![entry], vec![]).unwrap()
}

#[test]
fn codec_round_trip_is_deterministic_and_canonicalizes_tags() {
    let dataset = dataset();
    let first = encode_dataset(&dataset).unwrap();
    let second = encode_dataset(&dataset).unwrap();
    assert_eq!(first, second);

    let decoded = decode_dataset(MemoryLayer::Project, &first.0, &first.1).unwrap();
    assert_eq!(decoded.active()[0].tags, ["a", "z"]);
}

#[test]
fn legacy_access_fields_decode_into_confirmation_semantics() {
    let id = MemoryId::now_v7();
    let active = format!(
        r#"{{"schema_version":1,"location":"active","entries":[{{"id":"{id}","layer":"project","category":"fact","content":"legacy confirmation","source":"user","tags":[],"pinned":false,"created_at":10,"accessed_at":20,"access_count":3,"outdated":false}}]}}"#
    );
    let archive = br#"{"schema_version":1,"location":"archive","entries":[]}"#;

    let decoded = decode_dataset(MemoryLayer::Project, active.as_bytes(), archive).unwrap();

    assert_eq!(decoded.active()[0].last_confirmed_at, 20);
    assert_eq!(decoded.active()[0].confirmation_count, 3);
    let encoded = encode_dataset(&decoded).unwrap().0;
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(encoded.contains("last_confirmed_at"));
    assert!(encoded.contains("confirmation_count"));
    assert!(!encoded.contains("accessed_at"));
    assert!(!encoded.contains("access_count"));
}

#[test]
fn codec_rejects_location_swap_and_unknown_schema() {
    let encoded = encode_dataset(&dataset()).unwrap();
    assert!(decode_dataset(MemoryLayer::Project, &encoded.1, &encoded.0).is_err());

    let unsupported = br#"{"schema_version":99,"location":"active","entries":[]}"#;
    assert!(matches!(
        decode_dataset(MemoryLayer::Project, unsupported, &encoded.1),
        Err(MemoryOpenError::UnsupportedSchema { version: 99 })
    ));
}
