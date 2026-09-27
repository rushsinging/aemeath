use super::DurabilityData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationData {
    Primary,
    Previous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionScopeData {
    Blob,
    Dataset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuarantineReason {
    DigestMismatch,
    DecoderRejected,
    PromoteFromCorrupt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeleteOptionsData {
    include_quarantine: bool,
}

impl DeleteOptionsData {
    pub fn new(include_quarantine: bool) -> Self {
        Self { include_quarantine }
    }

    pub fn include_quarantine(self) -> bool {
        self.include_quarantine
    }
}

impl Default for DeleteOptionsData {
    fn default() -> Self {
        Self {
            include_quarantine: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeleteOutcomeData {
    deleted_primary: bool,
    deleted_previous: bool,
    deleted_quarantine: bool,
}

impl DeleteOutcomeData {
    pub fn new(deleted_primary: bool, deleted_previous: bool, deleted_quarantine: bool) -> Self {
        Self {
            deleted_primary,
            deleted_previous,
            deleted_quarantine,
        }
    }

    pub fn deleted_primary(self) -> bool {
        self.deleted_primary
    }

    pub fn deleted_previous(self) -> bool {
        self.deleted_previous
    }

    pub fn deleted_quarantine(self) -> bool {
        self.deleted_quarantine
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuarantineReceiptData {
    id: super::SafePathSegmentData,
    generation: GenerationData,
    scope: TransactionScopeData,
    reason: QuarantineReason,
}

impl QuarantineReceiptData {
    pub fn new(
        id: super::SafePathSegmentData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Self {
        Self {
            id,
            generation,
            scope,
            reason,
        }
    }

    pub fn id(&self) -> &super::SafePathSegmentData {
        &self.id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuarantineOutcomeData {
    Moved(QuarantineReceiptData),
    AlreadyAbsent {
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    },
}

impl QuarantineOutcomeData {
    pub fn already_absent(
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Self {
        Self::AlreadyAbsent {
            generation,
            scope,
            reason,
        }
    }

    pub fn generation(&self) -> GenerationData {
        match self {
            Self::Moved(receipt) => receipt.generation,
            Self::AlreadyAbsent { generation, .. } => *generation,
        }
    }

    pub fn scope(&self) -> TransactionScopeData {
        match self {
            Self::Moved(receipt) => receipt.scope,
            Self::AlreadyAbsent { scope, .. } => *scope,
        }
    }

    pub fn reason(&self) -> QuarantineReason {
        match self {
            Self::Moved(receipt) => receipt.reason,
            Self::AlreadyAbsent { reason, .. } => *reason,
        }
    }

    pub fn moved(&self) -> bool {
        matches!(self, Self::Moved(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromoteOutcomeData {
    Promoted(WriteReceiptData),
    AlreadyPromoted,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobReadData {
    generation: GenerationData,
    bytes: Vec<u8>,
}

impl BlobReadData {
    pub fn new(generation: GenerationData, bytes: Vec<u8>) -> Self {
        Self { generation, bytes }
    }

    pub fn generation(&self) -> GenerationData {
        self.generation
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadOutcomeData {
    Found(BlobReadData),
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageEntryData {
    key: super::StorageKeyData,
    size_bytes: usize,
}

impl StorageEntryData {
    pub fn new(key: super::StorageKeyData, size_bytes: usize) -> Self {
        Self { key, size_bytes }
    }

    pub fn key(&self) -> &super::StorageKeyData {
        &self.key
    }

    pub fn size_bytes(&self) -> usize {
        self.size_bytes
    }

    pub fn generation(&self) -> GenerationData {
        GenerationData::Primary
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteOptionsData {
    durability: DurabilityData,
}

impl WriteOptionsData {
    pub fn new(durability: DurabilityData) -> Self {
        Self { durability }
    }

    pub fn durability(self) -> DurabilityData {
        self.durability
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitWarningData {
    PreviousPromotionPending,
    JournalCleanupPending,
    /// The dataset is committed, but one or more members still require
    /// mechanical roll-forward before the generation becomes visible.
    MemberPublishRecoveryPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceiptData {
    warning: Option<CommitWarningData>,
}

impl WriteReceiptData {
    pub fn committed(warning: Option<CommitWarningData>) -> Self {
        Self { warning }
    }

    pub fn warning(self) -> Option<CommitWarningData> {
        self.warning
    }
}
