// 恢复决策单一真相源（#1865 A）：`decide_blob_recovery` / `decide_orphan_previous`
// 是本 crate 唯一的恢复策略表，adapter（blob_filesystem::recover_sync /
// recover_orphan_previous）只负责采集证据（journal phase × digest 观测）并执行
// 决策动作，NEVER 内联重写判定。CorruptionReason / QuarantineDisposition /
// CorruptTransactionError 为生产载荷。

use super::TransactionScopeData;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum JournalPhase {
    Prepared,
    Committed,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDecision {
    RollForward,
    RollBack,
    CleanOrphan,
    Corrupt(CorruptionReason),
}

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

pub fn decide_orphan_previous(matches_primary: bool) -> RecoveryDecision {
    if matches_primary {
        RecoveryDecision::CleanOrphan
    } else {
        RecoveryDecision::Corrupt(CorruptionReason::OrphanPreviousDigestMismatch)
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
