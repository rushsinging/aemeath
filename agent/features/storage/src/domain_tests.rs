use std::str::FromStr;

use crate::domain::{
    decide_blob_recovery, decide_orphan_previous, CorruptTransactionError, CorruptionReason,
    DeleteOptions, DigestObservation, Durability, Generation, JournalPhase, PreviousPolicy,
    QuarantineDisposition, QuarantineOutcome, QuarantineReason, RecoveryDecision, SafePathSegment,
    StorageErrorKind, StorageKey, StorageNamespace, TransactionDigest, TransactionScope,
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

    assert_eq!(error.kind(), &crate::domain::StorageErrorKind::InvalidKey);
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
fn prepared_recovery_decision_covers_new_old_absent_and_corrupt() {
    assert_eq!(
        decide_blob_recovery(JournalPhase::Prepared, DigestObservation::New),
        RecoveryDecision::RollForward
    );
    assert_eq!(
        decide_blob_recovery(JournalPhase::Prepared, DigestObservation::Old),
        RecoveryDecision::RollBack
    );
    assert_eq!(
        decide_blob_recovery(JournalPhase::Prepared, DigestObservation::Absent),
        RecoveryDecision::RollBack
    );
    assert_eq!(
        decide_blob_recovery(JournalPhase::Prepared, DigestObservation::Other),
        RecoveryDecision::Corrupt(CorruptionReason::PrimaryDigestMatchesNeitherGeneration)
    );
}

#[test]
fn committed_recovery_requires_new_digest() {
    assert_eq!(
        decide_blob_recovery(JournalPhase::Committed, DigestObservation::New),
        RecoveryDecision::RollForward
    );
    for observation in [
        DigestObservation::Old,
        DigestObservation::Absent,
        DigestObservation::Other,
    ] {
        assert_eq!(
            decide_blob_recovery(JournalPhase::Committed, observation),
            RecoveryDecision::Corrupt(CorruptionReason::CommittedDigestMismatch)
        );
    }
}

#[test]
fn orphan_previous_next_is_only_cleaned_when_it_matches_primary() {
    assert_eq!(decide_orphan_previous(true), RecoveryDecision::CleanOrphan);
    assert_eq!(
        decide_orphan_previous(false),
        RecoveryDecision::Corrupt(CorruptionReason::OrphanPreviousDigestMismatch)
    );
}

#[test]
fn transaction_digest_is_domain_separated_and_distinguishes_absent() {
    assert_ne!(
        TransactionDigest::blob_bytes(b"value"),
        TransactionDigest::dataset_bytes(b"value")
    );
    assert_ne!(
        TransactionDigest::blob_bytes(b""),
        TransactionDigest::absent_blob()
    );
    assert_eq!(
        TransactionDigest::blob_bytes(b"value"),
        TransactionDigest::blob_bytes(b"value")
    );
}

#[test]
fn corrupt_transaction_error_preserves_typed_facts_without_paths() {
    let corruption = CorruptTransactionError::new(
        TransactionScope::Blob,
        CorruptionReason::CommittedDigestMismatch,
        QuarantineDisposition::EvidenceQuarantined,
    );
    let kind = StorageErrorKind::CorruptTransaction(corruption.clone());

    assert_eq!(corruption.scope(), TransactionScope::Blob);
    assert_eq!(
        corruption.reason(),
        CorruptionReason::CommittedDigestMismatch
    );
    assert_eq!(
        corruption.quarantine_disposition(),
        QuarantineDisposition::EvidenceQuarantined
    );
    assert_eq!(kind, StorageErrorKind::CorruptTransaction(corruption));
    assert!(!format!("{kind:?}").contains('/'));
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
