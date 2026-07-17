use super::Durability;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Generation {
    Primary,
    Previous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionScope {
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
pub struct DeleteOptions {
    include_quarantine: bool,
}

impl DeleteOptions {
    pub fn new(include_quarantine: bool) -> Self {
        Self { include_quarantine }
    }

    pub fn include_quarantine(self) -> bool {
        self.include_quarantine
    }
}

impl Default for DeleteOptions {
    fn default() -> Self {
        Self {
            include_quarantine: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeleteOutcome {
    deleted_primary: bool,
    deleted_previous: bool,
    deleted_quarantine: bool,
}

impl DeleteOutcome {
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
pub struct QuarantineReceipt {
    id: super::SafePathSegment,
    generation: Generation,
    scope: TransactionScope,
    reason: QuarantineReason,
}

impl QuarantineReceipt {
    pub fn new(
        id: super::SafePathSegment,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Self {
        Self {
            id,
            generation,
            scope,
            reason,
        }
    }

    pub fn id(&self) -> &super::SafePathSegment {
        &self.id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuarantineOutcome {
    Moved(QuarantineReceipt),
    AlreadyAbsent {
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    },
}

impl QuarantineOutcome {
    pub fn already_absent(
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Self {
        Self::AlreadyAbsent {
            generation,
            scope,
            reason,
        }
    }

    pub fn generation(&self) -> Generation {
        match self {
            Self::Moved(receipt) => receipt.generation,
            Self::AlreadyAbsent { generation, .. } => *generation,
        }
    }

    pub fn scope(&self) -> TransactionScope {
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
pub enum PromoteOutcome {
    Promoted(WriteReceipt),
    AlreadyPromoted,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobRead {
    generation: Generation,
    bytes: Vec<u8>,
}

impl BlobRead {
    pub fn new(generation: Generation, bytes: Vec<u8>) -> Self {
        Self { generation, bytes }
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadOutcome {
    Found(BlobRead),
    NotFound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteOptions {
    durability: Durability,
}

impl WriteOptions {
    pub fn new(durability: Durability) -> Self {
        Self { durability }
    }

    pub fn durability(self) -> Durability {
        self.durability
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitWarning {
    PreviousPromotionPending,
    JournalCleanupPending,
    /// The dataset is committed, but one or more members still require
    /// mechanical roll-forward before the generation becomes visible.
    MemberPublishRecoveryPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    warning: Option<CommitWarning>,
}

impl WriteReceipt {
    pub fn committed(warning: Option<CommitWarning>) -> Self {
        Self { warning }
    }

    pub fn warning(self) -> Option<CommitWarning> {
        self.warning
    }
}
