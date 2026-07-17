use super::{BatchId, TaskId, TaskRevision, TaskSnapshot, TaskSnapshotCodecError};

const EMPTY_V2: &[u8] = br#"{
  "schema_version": 2,
  "revision": "0",
  "tasks": [],
  "next_task_id": "1",
  "next_batch_id": "1",
  "current_batch": null,
  "batches": []
}"#;

fn decode(bytes: &[u8]) -> TaskSnapshot {
    TaskSnapshot::decode(bytes).expect("fixture must decode")
}

fn encoded_text(snapshot: &TaskSnapshot) -> String {
    String::from_utf8(snapshot.encode().expect("snapshot must encode"))
        .expect("snapshot JSON must be UTF-8")
}

#[test]
fn snapshot_empty_v2_decodes_to_canonical_empty_snapshot() {
    let snapshot = decode(EMPTY_V2);

    assert_eq!(snapshot.revision(), TaskRevision::new(0));
    assert!(snapshot.tasks().is_empty());
    assert!(snapshot.batches().is_empty());
    assert_eq!(snapshot.next_task_id(), TaskId::new(1));
    assert_eq!(snapshot.next_batch_id(), BatchId::new(1));
    assert_eq!(snapshot.current_batch(), None);
}

#[test]
fn snapshot_empty_encodes_as_v2_with_string_ids_and_round_trips() {
    let snapshot = TaskSnapshot::empty();
    let bytes = snapshot.encode().expect("empty snapshot must encode");
    let json = std::str::from_utf8(&bytes).unwrap();

    assert!(json.contains(r#""schema_version":2"#) || json.contains(r#""schema_version": 2"#));
    for field in ["revision", "next_task_id", "next_batch_id"] {
        assert!(
            json.contains(&format!(r#""{field}":"#)) || json.contains(&format!(r#""{field}": "#)),
            "missing canonical field {field}: {json}"
        );
    }
    assert!(json.contains(r#""revision":"0"#) || json.contains(r#""revision": "0"#));
    assert!(json.contains(r#""next_task_id":"1"#) || json.contains(r#""next_task_id": "1"#));
    assert!(json.contains(r#""next_batch_id":"1"#) || json.contains(r#""next_batch_id": "1"#));
    assert_eq!(decode(&bytes), snapshot);
}

#[test]
fn snapshot_v1_upgrades_zero_current_batch_and_derives_missing_next_batch_id() {
    let legacy = br#"{
      "tasks": [],
      "next_id": 4,
      "current_batch": 0,
      "batches": [{
        "id": 7,
        "summary": "legacy",
        "status": "archived",
        "created_at": 10,
        "last_active_turn": 11,
        "silence_turns": 2
      }]
    }"#;

    let snapshot = decode(legacy);

    assert_eq!(snapshot.revision(), TaskRevision::new(0));
    assert_eq!(snapshot.next_task_id(), TaskId::new(4));
    assert_eq!(snapshot.next_batch_id(), BatchId::new(8));
    assert_eq!(snapshot.current_batch(), None);
    assert_eq!(snapshot.batches().len(), 1);

    let upgraded = encoded_text(&snapshot);
    assert!(
        upgraded.contains(r#""schema_version":2"#) || upgraded.contains(r#""schema_version": 2"#)
    );
    assert!(
        upgraded.contains(r#""next_batch_id":"8"#) || upgraded.contains(r#""next_batch_id": "8"#)
    );
    assert!(!upgraded.contains("next_id"));
}

#[test]
fn snapshot_v2_encoding_is_deterministic_and_sorts_all_repeated_values() {
    let unordered = br#"{
      "schema_version": 2,
      "revision": "9",
      "tasks": [
        {"id":"3","batch":"2","subject":"third","description":"","active_form":null,"session_id":null,"tags":["z","a"],"blocked_by":["2","1"],"status":"pending","priority":"normal","created_at":3,"updated_at":3,"started_at":null,"completed_at":null},
        {"id":"2","batch":"2","subject":"second","description":"","active_form":null,"session_id":null,"tags":["y","b"],"blocked_by":[],"status":"completed","priority":"high","created_at":2,"updated_at":2,"started_at":2,"completed_at":2},
        {"id":"1","batch":"2","subject":"first","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":[],"status":"completed","priority":"normal","created_at":1,"updated_at":1,"started_at":1,"completed_at":1}
      ],
      "next_task_id": "4",
      "next_batch_id": "3",
      "current_batch": "2",
      "batches": [
        {"id":"2","summary":"active","status":"active","created_at":2,"last_active_turn":4,"silence_turns":0},
        {"id":"1","summary":"old","status":"archived","created_at":1,"last_active_turn":3,"silence_turns":1}
      ]
    }"#;

    let first = decode(unordered).encode().unwrap();
    let second = decode(unordered).encode().unwrap();
    assert_eq!(
        first, second,
        "the same logical snapshot must produce identical bytes"
    );

    let json = std::str::from_utf8(&first).unwrap();
    let task_one = json.find(r#""subject":"first"#).unwrap();
    let task_two = json.find(r#""subject":"second"#).unwrap();
    let task_three = json.find(r#""subject":"third"#).unwrap();
    assert!(task_one < task_two && task_two < task_three);
    let batch_one = json.find(r#""summary":"old"#).unwrap();
    let batch_two = json.find(r#""summary":"active"#).unwrap();
    assert!(batch_one < batch_two);
    assert!(
        json.contains(r#""tags":["a","z"]"#)
            || json.contains("\"tags\": [\n        \"a\",\n        \"z\"")
    );
    assert!(
        json.contains(r#""blocked_by":["1","2"]"#)
            || json.contains("\"blocked_by\": [\n        \"1\",\n        \"2\"")
    );
}

#[test]
fn snapshot_future_version_is_rejected_without_falling_back_to_v1() {
    let bytes = br#"{"schema_version":3,"revision":"0","tasks":[],"next_task_id":"1","next_batch_id":"1","current_batch":null,"batches":[]}"#;

    assert!(matches!(
        TaskSnapshot::decode(bytes),
        Err(TaskSnapshotCodecError::UnsupportedFutureVersion { version: 3 })
    ));
}

#[test]
fn snapshot_malformed_json_returns_typed_codec_error() {
    assert!(matches!(
        TaskSnapshot::decode(br#"{"schema_version":2,"tasks":["#),
        Err(TaskSnapshotCodecError::InvalidJson(_))
    ));
}

#[test]
fn snapshot_v2_rejects_numeric_mixed_and_zero_id_representations() {
    let cases: &[(&str, &[u8])] = &[
        (
            "numeric IDs",
            br#"{"schema_version":2,"revision":0,"tasks":[],"next_task_id":1,"next_batch_id":1,"current_batch":null,"batches":[]}"#,
        ),
        (
            "mixed string and numeric IDs",
            br#"{"schema_version":2,"revision":"0","tasks":[],"next_task_id":"1","next_batch_id":2,"current_batch":null,"batches":[]}"#,
        ),
        (
            "zero next task ID",
            br#"{"schema_version":2,"revision":"0","tasks":[],"next_task_id":"0","next_batch_id":"1","current_batch":null,"batches":[]}"#,
        ),
        (
            "zero current batch ID",
            br#"{"schema_version":2,"revision":"0","tasks":[],"next_task_id":"1","next_batch_id":"1","current_batch":"0","batches":[]}"#,
        ),
    ];

    for (case, bytes) in cases {
        assert!(
            matches!(
                TaskSnapshot::decode(bytes),
                Err(TaskSnapshotCodecError::InvalidIdRepresentation { .. })
            ),
            "V2 must reject {case}"
        );
    }
}

/// A present `schema_version` must be an unsigned integer. Only an absent
/// field selects the legacy V1 decoder; malformed representations fail closed
/// with the dedicated typed codec error.
fn legacy_v1_body_with_schema_version(schema_version_json: &str) -> Vec<u8> {
    // A fully well-formed, minimal legacy V1 envelope. With `schema_version`
    // removed entirely this decodes successfully as V1 (`next_id` is V1's
    // only required field), so failures specifically exercise version parsing.
    format!(r#"{{"schema_version":{schema_version_json},"tasks":[],"next_id":1,"current_batch":0,"batches":[]}}"#)
        .into_bytes()
}

fn assert_invalid_schema_version(schema_version_json: &str) {
    assert!(matches!(
        TaskSnapshot::decode(&legacy_v1_body_with_schema_version(schema_version_json)),
        Err(TaskSnapshotCodecError::InvalidSchemaVersionRepresentation { .. })
    ));
}

#[test]
fn snapshot_negative_schema_version_must_not_fall_back_to_v1() {
    assert_invalid_schema_version("-1");
}

#[test]
fn snapshot_string_schema_version_must_not_fall_back_to_v1() {
    assert_invalid_schema_version(r#""2""#);
}

#[test]
fn snapshot_fractional_schema_version_must_not_fall_back_to_v1() {
    assert_invalid_schema_version("2.5");
}

#[test]
fn snapshot_u64_overflow_schema_version_must_not_fall_back_to_v1() {
    assert_invalid_schema_version("18446744073709551616");
}

#[test]
fn snapshot_v2_ignores_unknown_envelope_and_entity_fields() {
    let with_unknown_fields = br#"{
      "schema_version":2,
      "revision":"1",
      "tasks":[],
      "next_task_id":"1",
      "next_batch_id":"2",
      "current_batch":null,
      "batches":[{"id":"1","summary":"old","status":"archived","created_at":1,"last_active_turn":1,"silence_turns":0,"future_batch_field":{"x":1}}],
      "future_envelope_field":[1,2,3]
    }"#;

    let snapshot = decode(with_unknown_fields);
    assert_eq!(snapshot.batches().len(), 1);

    let canonical = encoded_text(&snapshot);
    assert!(!canonical.contains("future_envelope_field"));
    assert!(!canonical.contains("future_batch_field"));
    assert_eq!(decode(canonical.as_bytes()), snapshot);
}
