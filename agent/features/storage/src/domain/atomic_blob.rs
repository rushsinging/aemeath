use super::Durability;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Generation {
    Primary,
    Previous,
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
