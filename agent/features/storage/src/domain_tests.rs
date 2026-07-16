use std::str::FromStr;

use crate::domain::{
    DeleteOptions, Durability, Generation, PreviousPolicy, QuarantineOutcome, QuarantineReason,
    SafePathSegment, StorageKey, StorageNamespace, TransactionScope,
};

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

#[test]
fn namespace_previous_policy_is_explicit() {
    for namespace in [
        StorageNamespace::Session,
        StorageNamespace::Memory,
        StorageNamespace::Task,
        StorageNamespace::History,
        StorageNamespace::ToolResult,
        StorageNamespace::Config,
        StorageNamespace::Workspace,
        StorageNamespace::Cost,
    ] {
        assert_eq!(namespace.previous_policy(), PreviousPolicy::Retain);
    }
    assert_eq!(
        StorageNamespace::AuditUsage.previous_policy(),
        PreviousPolicy::Discard
    );
}

#[test]
fn delete_options_default_includes_quarantine() {
    assert!(DeleteOptions::default().include_quarantine());
}

#[test]
fn quarantine_already_absent_preserves_requested_facts() {
    let outcome = QuarantineOutcome::already_absent(
        Generation::Previous,
        TransactionScope::Blob,
        QuarantineReason::DecoderRejected,
    );

    assert_eq!(outcome.generation(), Generation::Previous);
    assert_eq!(outcome.scope(), TransactionScope::Blob);
    assert_eq!(outcome.reason(), QuarantineReason::DecoderRejected);
    assert!(!outcome.moved());
}
