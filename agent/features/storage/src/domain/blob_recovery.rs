// JournalPhase / DigestObservation / RecoveryDecision、decide_* 与 TransactionDigest 仅测试引用：
// 生产恢复路径在 adapters（`blob_protocol` 侧副本 + 内联 match）实现，逐项收窄到 test 目标（#1995）；
// CorruptionReason / QuarantineDisposition / CorruptTransactionError 仍为生产载荷，保持原样。
#[cfg(test)]
use sha2::{Digest, Sha256};

use super::TransactionScopeData;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalPhase {
    Prepared,
    Committed,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigestObservation {
    New,
    Old,
    Absent,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorruptionReason {
    PrimaryDigestMatchesNeitherGeneration,
    CommittedDigestMismatch,
    OrphanPreviousDigestMismatch,
    InvalidJournal,
    /// A dataset member's staged or published bytes match neither the old nor
    /// the new generation digest recorded in the durable journal.
    DatasetMemberDigestMismatch,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDecision {
    RollForward,
    RollBack,
    CleanOrphan,
    Corrupt(CorruptionReason),
}

#[cfg(test)]
pub fn decide_blob_recovery(
    phase: JournalPhase,
    observation: DigestObservation,
) -> RecoveryDecision {
    match (phase, observation) {
        (JournalPhase::Prepared, DigestObservation::New) => RecoveryDecision::RollForward,
        (JournalPhase::Prepared, DigestObservation::Old | DigestObservation::Absent) => {
            RecoveryDecision::RollBack
        }
        (JournalPhase::Prepared, DigestObservation::Other) => {
            RecoveryDecision::Corrupt(CorruptionReason::PrimaryDigestMatchesNeitherGeneration)
        }
        (JournalPhase::Committed, DigestObservation::New) => RecoveryDecision::RollForward,
        (JournalPhase::Committed, _) => {
            RecoveryDecision::Corrupt(CorruptionReason::CommittedDigestMismatch)
        }
    }
}

#[cfg(test)]
pub fn decide_orphan_previous(matches_primary: bool) -> RecoveryDecision {
    if matches_primary {
        RecoveryDecision::CleanOrphan
    } else {
        RecoveryDecision::Corrupt(CorruptionReason::OrphanPreviousDigestMismatch)
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionDigest([u8; 32]);

#[cfg(test)]
impl TransactionDigest {
    pub fn blob_bytes(bytes: &[u8]) -> Self {
        Self::calculate(b"aemeath.storage.blob.bytes.v1\0", bytes)
    }

    pub fn dataset_bytes(bytes: &[u8]) -> Self {
        Self::calculate(b"aemeath.storage.dataset.bytes.v1\0", bytes)
    }

    pub fn absent_blob() -> Self {
        Self::calculate(b"aemeath.storage.blob.absent.v1\0", &[])
    }

    fn calculate(domain: &[u8], bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineDisposition {
    EvidenceQuarantined,
    QuarantineFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorruptTransactionError {
    scope: TransactionScopeData,
    reason: CorruptionReason,
    quarantine_disposition: QuarantineDisposition,
}

impl CorruptTransactionError {
    pub fn new(
        scope: TransactionScopeData,
        reason: CorruptionReason,
        quarantine_disposition: QuarantineDisposition,
    ) -> Self {
        Self {
            scope,
            reason,
            quarantine_disposition,
        }
    }

    pub fn scope(&self) -> TransactionScopeData {
        self.scope
    }

    pub fn reason(&self) -> CorruptionReason {
        self.reason
    }

    pub fn quarantine_disposition(&self) -> QuarantineDisposition {
        self.quarantine_disposition
    }
}
