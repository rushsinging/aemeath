use std::str::FromStr;

use crate::domain::{
    decide_blob_recovery, decide_orphan_previous, CorruptTransactionError, CorruptionReason,
    DatasetChangeSetData, DatasetKeyData, DatasetManifestData, DatasetMemberChangeData,
    DatasetMemberData, DatasetMemberReferenceData, DeleteOptionsData, DigestObservation,
    DurabilityData, GenerationData, JournalPhase, PreviousPolicy, QuarantineDisposition,
    QuarantineOutcomeData, QuarantineReason, RecoveryDecision, SafePathSegmentData,
    StorageErrorKind, StorageKeyData, StorageNamespaceData, TransactionDigest,
    TransactionScopeData,
};

#[test]
fn safe_path_segment_accepts_plain_component() {
    for value in ["a", "SESSION_01", "会话-01"] {
        let segment =
            SafePathSegmentData::from_str(value).expect("plain component should be valid");
        assert_eq!(segment.as_str(), value);
        assert_eq!(segment.to_string(), value);
    }
}

#[test]
fn safe_path_segment_rejects_unsafe_components() {
    for value in ["", ".", "..", ".hidden", "/tmp", "a/b", "a\\b", "a\0b"] {
        assert!(
            SafePathSegmentData::from_str(value).is_err(),
            "unsafe segment must be rejected: {value:?}"
        );
    }
}

#[test]
fn storage_key_requires_at_least_one_segment() {
    let error = StorageKeyData::new(StorageNamespaceData::Session, Vec::new())
        .expect_err("empty keys must be rejected");

    assert_eq!(error.kind(), &crate::domain::StorageErrorKind::InvalidKey);
}

#[test]
fn namespace_minimum_durability_cannot_be_lowered() {
    assert_eq!(
        StorageNamespaceData::Session.minimum_durability(),
        DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespaceData::ToolResult.effective_durability(DurabilityData::BestEffort),
        DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespaceData::AuditUsage.effective_durability(DurabilityData::BestEffort),
        DurabilityData::BestEffort
    );
}

#[test]
fn namespace_previous_policy_is_explicit() {
    for namespace in [
        StorageNamespaceData::Session,
        StorageNamespaceData::Memory,
        StorageNamespaceData::TaskData,
        StorageNamespaceData::History,
        StorageNamespaceData::ToolResult,
        StorageNamespaceData::Config,
        StorageNamespaceData::Workspace,
    ] {
        assert_eq!(namespace.previous_policy(), PreviousPolicy::Retain);
    }
    assert_eq!(
        StorageNamespaceData::AuditUsage.previous_policy(),
        PreviousPolicy::Discard
    );
}

#[test]
fn delete_options_default_includes_quarantine() {
    assert!(DeleteOptionsData::default().include_quarantine());
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
        TransactionScopeData::Blob,
        CorruptionReason::CommittedDigestMismatch,
        QuarantineDisposition::EvidenceQuarantined,
    );
    let kind = StorageErrorKind::CorruptTransaction(corruption.clone());

    assert_eq!(corruption.scope(), TransactionScopeData::Blob);
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
    let outcome = QuarantineOutcomeData::already_absent(
        GenerationData::Previous,
        TransactionScopeData::Blob,
        QuarantineReason::DecoderRejected,
    );

    assert_eq!(outcome.generation(), GenerationData::Previous);
    assert_eq!(outcome.scope(), TransactionScopeData::Blob);
    assert_eq!(outcome.reason(), QuarantineReason::DecoderRejected);
    assert!(!outcome.moved());
}

// --- #983 AtomicDataset published-language (L1) ---------------------------------

fn dataset_member(name: &str, bytes: &[u8]) -> DatasetMemberData {
    DatasetMemberData::new(
        SafePathSegmentData::from_str(name).expect("member name should be a safe path segment"),
        bytes.to_vec(),
    )
}

#[test]
fn dataset_key_with_empty_segments_is_rejected() {
    let error = DatasetKeyData::new(StorageNamespaceData::Memory, Vec::new())
        .expect_err("dataset keys with no segments must be rejected");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn dataset_manifest_orders_members_canonically_by_name() {
    let manifest = DatasetManifestData::new(vec![
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
    let error = DatasetManifestData::new(vec![
        dataset_member("index", b"first"),
        dataset_member("index", b"second"),
    ])
    .expect_err("duplicate member names must be rejected");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn empty_dataset_manifest_has_stable_revision() {
    let first = DatasetManifestData::new(Vec::new()).expect("empty manifest is valid");
    let second = DatasetManifestData::new(Vec::new()).expect("empty manifest is valid");

    assert_eq!(first.revision(), second.revision());
}

#[test]
fn dataset_revision_is_independent_of_member_input_order() {
    let ordered = DatasetManifestData::new(vec![
        dataset_member("active", b"a"),
        dataset_member("archive", b"z"),
    ])
    .expect("distinct member names should be accepted");
    let shuffled = DatasetManifestData::new(vec![
        dataset_member("archive", b"z"),
        dataset_member("active", b"a"),
    ])
    .expect("distinct member names should be accepted");

    assert_eq!(ordered.revision(), shuffled.revision());
}

#[test]
fn dataset_revision_changes_when_member_name_changes() {
    let base = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("distinct member names should be accepted");
    let renamed = DatasetManifestData::new(vec![dataset_member("archive", b"a")])
        .expect("distinct member names should be accepted");

    assert_ne!(base.revision(), renamed.revision());
}

#[test]
fn dataset_revision_changes_when_member_bytes_change() {
    let base = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("distinct member names should be accepted");
    let mutated = DatasetManifestData::new(vec![dataset_member("active", b"b")])
        .expect("distinct member names should be accepted");

    assert_ne!(base.revision(), mutated.revision());
}

#[test]
fn manifest_member_evidence_matches_only_original_bytes() {
    let manifest = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("current generation should be valid");
    let evidence = manifest
        .member_evidence(&SafePathSegmentData::from_str("active").expect("safe member name"))
        .expect("manifest evidence");

    assert!(evidence.matches_bytes(b"a"));
    assert!(!evidence.matches_bytes(b"b"));
    assert!(!evidence.matches_bytes(b"aa"));
}

#[test]
fn incremental_dataset_members_distinguish_new_and_reused_bytes() {
    let current = DatasetManifestData::new(vec![
        dataset_member("active", b"a"),
        dataset_member("archive", b"z"),
    ])
    .expect("current generation should be valid");
    let reused = DatasetMemberReferenceData::from_manifest_member(
        current.revision().clone(),
        SafePathSegmentData::from_str("archive").expect("safe member name"),
        1,
        [0; 32],
    );
    let replacement = DatasetMemberData::new(
        SafePathSegmentData::from_str("active").expect("safe member name"),
        b"a2".to_vec(),
    );
    let change_set = DatasetChangeSetData::new(
        current.revision().clone(),
        vec![DatasetMemberChangeData::Replace(replacement)],
        vec![reused.clone()],
    )
    .expect("incremental member set should be valid");

    assert_eq!(change_set.reused_members(), &[reused]);
    assert_eq!(change_set.new_members().len(), 1);
    assert!(change_set.removed_members().is_empty());
}

#[test]
fn incremental_dataset_rejects_duplicate_new_and_reused_member_names() {
    let revision = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("current generation should be valid")
        .revision()
        .clone();
    let reused = DatasetMemberReferenceData::from_manifest_member(
        revision.clone(),
        SafePathSegmentData::from_str("active").expect("safe member name"),
        1,
        [0; 32],
    );
    let error = DatasetChangeSetData::new(
        revision,
        vec![DatasetMemberChangeData::Replace(dataset_member(
            "active", b"a2",
        ))],
        vec![reused],
    )
    .expect_err("one member cannot be both replaced and reused");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn incremental_dataset_removal_names_are_canonical_and_unique() {
    let revision = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("current generation should be valid")
        .revision()
        .clone();
    let active = SafePathSegmentData::from_str("active").expect("safe member name");
    let archive = SafePathSegmentData::from_str("archive").expect("safe member name");
    let change_set = DatasetChangeSetData::new(revision.clone(), Vec::new(), Vec::new())
        .expect("empty change set should be valid")
        .with_removed_members(vec![archive.clone(), active.clone()])
        .expect("distinct removal names should be valid");

    assert_eq!(change_set.removed_members(), &[active.clone(), archive]);

    let duplicate_error = DatasetChangeSetData::new(revision, Vec::new(), Vec::new())
        .expect("empty change set should be valid")
        .with_removed_members(vec![active.clone(), active])
        .expect_err("duplicate removal names must be rejected");
    assert_eq!(duplicate_error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn incremental_dataset_rejects_reused_member_from_another_revision() {
    let expected_revision = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("current generation should be valid")
        .revision()
        .clone();
    let another_revision = DatasetManifestData::new(vec![dataset_member("active", b"b")])
        .expect("another generation should be valid")
        .revision()
        .clone();
    let reused = DatasetMemberReferenceData::from_manifest_member(
        another_revision,
        SafePathSegmentData::from_str("active").expect("safe member name"),
        1,
        [0; 32],
    );

    let error = DatasetChangeSetData::new(expected_revision, Vec::new(), vec![reused])
        .expect_err("a reused member must belong to the expected generation");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn incremental_dataset_rejects_member_named_as_removed() {
    let revision = DatasetManifestData::new(vec![dataset_member("active", b"a")])
        .expect("current generation should be valid")
        .revision()
        .clone();
    let active = SafePathSegmentData::from_str("active").expect("safe member name");
    let error = DatasetChangeSetData::new(
        revision,
        vec![DatasetMemberChangeData::Replace(dataset_member(
            "active", b"a2",
        ))],
        Vec::new(),
    )
    .expect("replacement should be valid")
    .with_removed_members(vec![active])
    .expect_err("a target member cannot also be removed");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
}

#[test]
fn omitted_members_are_old_names_absent_from_replacement() {
    let current = DatasetManifestData::new(vec![
        dataset_member("active", b"a"),
        dataset_member("archive", b"z"),
        dataset_member("index", b"i"),
    ])
    .expect("distinct member names should be accepted");
    let replacement = DatasetManifestData::new(vec![
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

#[test]
fn background_process_namespace_is_crash_safe_and_retained() {
    // #252：后台进程账本跨进程可见（resume 失效对账与查询）。
    assert_eq!(
        StorageNamespaceData::BackgroundProcess.as_str(),
        "background-process"
    );
    assert_eq!(
        StorageNamespaceData::BackgroundProcess.minimum_durability(),
        DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespaceData::BackgroundProcess.effective_durability(DurabilityData::BestEffort),
        DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        StorageNamespaceData::BackgroundProcess.previous_policy(),
        PreviousPolicy::Retain
    );
}
