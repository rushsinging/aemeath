use std::str::FromStr;

use crate::domain::{Durability, SafePathSegment, StorageKey, StorageNamespace};

#[test]
fn safe_path_segment_accepts_plain_component() {
    let segment = SafePathSegment::from_str("session-01").expect("plain component should be valid");

    assert_eq!(segment.as_str(), "session-01");
}

#[test]
fn safe_path_segment_rejects_unsafe_components() {
    for value in ["", ".", "..", ".hidden", "/tmp", "a/b", "a\\b", "a\0b"] {
        assert!(
            SafePathSegment::from_str(value).is_err(),
            "unsafe segment must be rejected: {value:?}"
        );
    }
}

#[test]
fn storage_key_requires_at_least_one_segment() {
    let error = StorageKey::new(StorageNamespace::Session, Vec::new())
        .expect_err("empty keys must be rejected");

    assert_eq!(error.kind(), crate::domain::StorageErrorKind::InvalidKey);
}

#[test]
fn namespace_minimum_durability_cannot_be_lowered() {
    assert_eq!(
        StorageNamespace::Session.minimum_durability(),
        Durability::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespace::ToolResult.effective_durability(Durability::BestEffort),
        Durability::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespace::AuditUsage.effective_durability(Durability::BestEffort),
        Durability::BestEffort
    );
}
