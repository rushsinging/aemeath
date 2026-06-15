use super::*;
use uuid::{NoContext, Timestamp, Uuid};

#[test]
fn test_session_creation() {
    let session = InternalSession::new(Path::new("/tmp"));
    assert!(!session.id.is_empty());
    assert_eq!(Uuid::parse_str(&session.id).unwrap().get_version_num(), 7);
    assert_eq!(session.cwd, "/tmp");
}

#[test]
fn test_new_session_id_happy_path_is_uuidv7() {
    let id = new_session_id();
    let uuid = Uuid::parse_str(&id).unwrap();

    assert_eq!(id.len(), 36);
    assert_eq!(uuid.get_version_num(), 7);
    assert!(validate_session_id(&id).is_ok());
}

#[test]
fn test_new_session_id_boundary_same_timestamp_still_unique() {
    let timestamp = Timestamp::from_unix(NoContext, 1_700_000_000, 123_000_000);

    let first = new_session_id_with_timestamp(timestamp);
    let second = new_session_id_with_timestamp(timestamp);

    assert_ne!(first, second);
    assert_eq!(Uuid::parse_str(&first).unwrap().get_version_num(), 7);
    assert_eq!(Uuid::parse_str(&second).unwrap().get_version_num(), 7);
}

#[test]
fn test_new_session_id_error_path_rejects_malformed_uuid_like_id() {
    let malformed = "018f2d4e-9c7a-7b12-9a34-8f0c1d2e3f45/evil";

    assert!(validate_session_id(malformed).is_err());
}

#[test]
fn test_validate_session_id_accepts_legacy_hex_id() {
    assert!(validate_session_id("0000019dc93bab86dfd7032f").is_ok());
}
