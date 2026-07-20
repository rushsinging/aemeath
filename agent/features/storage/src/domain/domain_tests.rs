use std::str::FromStr;

use crate::domain::{
    decide_blob_recovery, decide_orphan_previous, CorruptTransactionError, CorruptionReason,
    DatasetKey, DatasetManifest, DatasetMember, DeleteOptions, DigestObservation, Durability,
    Generation, JournalPhase, PreviousPolicy, QuarantineDisposition, QuarantineOutcome,
    QuarantineReason, RecoveryDecision, SafePathSegment, StorageErrorKind, StorageKey,
    StorageNamespace, TransactionDigest, TransactionScope,
};

#[test]
fn safe_path_segment_accepts_plain_component() {
    for value in ["a", "SESSION_01", "会话-01"] {
        let segment = SafePathSegment::from_str(value).expect("plain component should be valid");
        assert_eq!(segment.as_str(), value);
        assert_eq!(segment.to_string(), value);
    }
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

// --- #983 AtomicDataset published-language (L1) ---------------------------------

fn dataset_member(name: &str, bytes: &[u8]) -> DatasetMember {
    DatasetMember::new(
        SafePathSegment::from_str(name).expect("member name should be a safe path segment"),
        bytes.to_vec(),
    )
}

#[test]
fn dataset_key_with_empty_segments_is_rejected() {
    let error = DatasetKey::new(StorageNamespace::Memory, Vec::new())
        .expect_err("dataset keys with no segments must be rejected");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn dataset_manifest_orders_members_canonically_by_name() {
    let manifest = DatasetManifest::new(vec![
        dataset_member("payload", b"p"),
        dataset_member("active", b"a"),
        dataset_member("index", b"i"),
    ])
    .expect("distinct member names should be accepted");

    let names: Vec<&str> = manifest
        .members()
        .iter()
        .map(|member| member.as_str())
        .collect();

    assert_eq!(names, ["active", "index", "payload"]);
}

#[test]
fn dataset_manifest_with_duplicate_member_names_is_rejected() {
    let error = DatasetManifest::new(vec![
        dataset_member("index", b"first"),
        dataset_member("index", b"second"),
    ])
    .expect_err("duplicate member names must be rejected");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn empty_dataset_manifest_has_stable_revision() {
    let first = DatasetManifest::new(Vec::new()).expect("empty manifest is valid");
    let second = DatasetManifest::new(Vec::new()).expect("empty manifest is valid");

    assert_eq!(first.revision(), second.revision());
}

#[test]
fn dataset_revision_is_independent_of_member_input_order() {
    let ordered = DatasetManifest::new(vec![
        dataset_member("active", b"a"),
        dataset_member("archive", b"z"),
    ])
    .expect("distinct member names should be accepted");
    let shuffled = DatasetManifest::new(vec![
        dataset_member("archive", b"z"),
        dataset_member("active", b"a"),
    ])
    .expect("distinct member names should be accepted");

    assert_eq!(ordered.revision(), shuffled.revision());
}

#[test]
fn dataset_revision_changes_when_member_name_changes() {
    let base = DatasetManifest::new(vec![dataset_member("active", b"a")])
        .expect("distinct member names should be accepted");
    let renamed = DatasetManifest::new(vec![dataset_member("archive", b"a")])
        .expect("distinct member names should be accepted");

    assert_ne!(base.revision(), renamed.revision());
}

#[test]
fn dataset_revision_changes_when_member_bytes_change() {
    let base = DatasetManifest::new(vec![dataset_member("active", b"a")])
        .expect("distinct member names should be accepted");
    let mutated = DatasetManifest::new(vec![dataset_member("active", b"b")])
        .expect("distinct member names should be accepted");

    assert_ne!(base.revision(), mutated.revision());
}

#[test]
fn omitted_members_are_old_names_absent_from_replacement() {
    let current = DatasetManifest::new(vec![
        dataset_member("active", b"a"),
        dataset_member("archive", b"z"),
        dataset_member("index", b"i"),
    ])
    .expect("distinct member names should be accepted");
    let replacement = DatasetManifest::new(vec![
        dataset_member("active", b"a2"),
        dataset_member("index", b"i2"),
    ])
    .expect("distinct member names should be accepted");

    let omitted: Vec<&str> = current
        .omitted_members(&replacement)
        .iter()
        .map(|name| name.as_str())
        .collect();

    assert_eq!(omitted, ["archive"]);
}
