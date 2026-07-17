use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest, Sha256};

use super::{CommitWarning, SafePathSegment, StorageError, StorageErrorKind, StorageNamespace};

const REVISION_DOMAIN: &[u8] = b"aemeath.storage.dataset.revision.v1\0";
const MEMBER_BYTES_DOMAIN: &[u8] = b"aemeath.storage.dataset.member.bytes.v1\0";

/// 计算单个成员字节参与修订号运算的领域摘要（`MEMBER_BYTES_DOMAIN`）。
///
/// 与 `DatasetRevision::from_canonical_members` 内联的成员摘要算法严格一致：
/// `SHA256(MEMBER_BYTES_DOMAIN || len_le64 || bytes)`。adapter 将其持久化进事务
/// journal，使得恢复时不需要原始字节即可精确重算 `DatasetRevision`。
pub(crate) fn revision_member_digest(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(MEMBER_BYTES_DOMAIN);
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    digest.finalize().into()
}

/// The adapter-independent logical location of an atomic dataset.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DatasetKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

impl DatasetKey {
    pub fn new(
        namespace: StorageNamespace,
        segments: Vec<SafePathSegment>,
    ) -> Result<Self, StorageError> {
        if segments.is_empty() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "数据集键至少需要一个路径段",
            ));
        }

        Ok(Self {
            namespace,
            segments,
        })
    }

    pub fn namespace(&self) -> StorageNamespace {
        self.namespace
    }

    pub fn segments(&self) -> &[SafePathSegment] {
        &self.segments
    }
}

/// One named byte value supplied to a dataset commit or returned by a read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetMember {
    name: SafePathSegment,
    bytes: Vec<u8>,
}

impl DatasetMember {
    pub fn new(name: SafePathSegment, bytes: Vec<u8>) -> Self {
        Self { name, bytes }
    }

    pub fn name(&self) -> &SafePathSegment {
        &self.name
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A Storage-generated opaque fingerprint of a complete dataset generation.
///
/// The fingerprint is deliberately redacted from `Debug` so that raw
/// generation bytes never leak into logs, panics, or receipts.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct DatasetRevision([u8; 32]);

impl fmt::Debug for DatasetRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatasetRevision(<redacted>)")
    }
}

impl DatasetRevision {
    /// 供 adapter 将修订号持久化到私有 schema（十六进制）后再复原。
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 供 adapter 从持久化的权威 manifest 复原修订号。仅可用于同一完整代先前生成的字节。
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn from_canonical_members(members: &[DatasetMember]) -> Self {
        let evidence: Vec<(&str, u64, [u8; 32])> = members
            .iter()
            .map(|member| {
                (
                    member.name.as_str(),
                    member.bytes.len() as u64,
                    revision_member_digest(&member.bytes),
                )
            })
            .collect();
        Self::from_member_digests(&evidence)
    }

    /// 从每个成员的 canonical 名称、字节数与 `revision_member_digest` 精确重算修订号，
    /// 无需原始字节。恢复时据此校验事务 journal 记录的新修订号是否自洽。
    ///
    /// 输入无需预排序：内部按名称升序 canonicalize，与 `from_canonical_members`
    /// 的 canonical 成员顺序一致。
    pub(crate) fn from_member_digests(members: &[(&str, u64, [u8; 32])]) -> Self {
        let mut ordered: Vec<&(&str, u64, [u8; 32])> = members.iter().collect();
        ordered.sort_by(|left, right| left.0.cmp(right.0));

        let mut revision = Sha256::new();
        revision.update(REVISION_DOMAIN);
        revision.update((ordered.len() as u64).to_le_bytes());

        for (name, byte_len, member_digest) in ordered {
            let name_bytes = name.as_bytes();
            revision.update((name_bytes.len() as u64).to_le_bytes());
            revision.update(name_bytes);
            revision.update(byte_len.to_le_bytes());
            revision.update(member_digest);
        }

        Self(revision.finalize().into())
    }
}

/// Storage's authoritative member-name manifest for one complete generation.
///
/// Member bytes intentionally are not exposed by this discovery value. Use a
/// consistent read to obtain bytes belonging to the reported revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetManifest {
    revision: DatasetRevision,
    members: Vec<SafePathSegment>,
}

impl DatasetManifest {
    /// Freezes a complete generation into canonical member-name order.
    pub(crate) fn new(mut members: Vec<DatasetMember>) -> Result<Self, StorageError> {
        canonicalize_members(&mut members)?;
        let revision = DatasetRevision::from_canonical_members(&members);
        let members = members.into_iter().map(|member| member.name).collect();
        Ok(Self { revision, members })
    }

    /// Reconstitutes a manifest from Storage-owned persisted facts.
    ///
    /// Adapters must only use a revision previously generated for the same
    /// complete generation; callers cannot inspect or construct a revision.
    pub(crate) fn from_revision(
        revision: DatasetRevision,
        mut members: Vec<SafePathSegment>,
    ) -> Result<Self, StorageError> {
        members.sort();
        reject_duplicate_names(&members)?;
        Ok(Self { revision, members })
    }

    pub fn revision(&self) -> &DatasetRevision {
        &self.revision
    }

    pub fn members(&self) -> &[SafePathSegment] {
        &self.members
    }

    /// Returns names present in this manifest but absent from its replacement.
    pub fn omitted_members<'a>(&'a self, replacement: &Self) -> Vec<&'a SafePathSegment> {
        let replacement_names: BTreeSet<&SafePathSegment> = replacement.members.iter().collect();
        self.members
            .iter()
            .filter(|name| !replacement_names.contains(name))
            .collect()
    }
}

/// A revision and requested member bytes read under one dataset lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetRead {
    revision: DatasetRevision,
    members: Vec<DatasetMember>,
}

impl DatasetRead {
    pub(crate) fn new(
        revision: DatasetRevision,
        mut members: Vec<DatasetMember>,
    ) -> Result<Self, StorageError> {
        canonicalize_members(&mut members)?;
        Ok(Self { revision, members })
    }

    pub fn revision(&self) -> &DatasetRevision {
        &self.revision
    }

    pub fn members(&self) -> &[DatasetMember] {
        &self.members
    }
}

/// Result of reading a requested member set from one explicit generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetReadOutcome {
    Found(DatasetRead),
    NotFound,
}

/// Whether a logically committed generation is already externally visible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetCommitVisibility {
    Visible,
    RecoveryPending,
}

/// Proof that a dataset generation crossed its logical commit point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetCommitReceipt {
    revision: DatasetRevision,
    visibility: DatasetCommitVisibility,
    warning: Option<CommitWarning>,
}

impl DatasetCommitReceipt {
    pub(crate) fn committed(
        revision: DatasetRevision,
        visibility: DatasetCommitVisibility,
        warning: Option<CommitWarning>,
    ) -> Self {
        Self {
            revision,
            visibility,
            warning,
        }
    }

    pub fn revision(&self) -> &DatasetRevision {
        &self.revision
    }

    pub fn visibility(&self) -> DatasetCommitVisibility {
        self.visibility
    }

    pub fn warning(&self) -> Option<CommitWarning> {
        self.warning
    }
}

fn canonicalize_members(members: &mut [DatasetMember]) -> Result<(), StorageError> {
    members.sort_by(|left, right| left.name.cmp(&right.name));
    if members.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(duplicate_member_error());
    }
    Ok(())
}

fn reject_duplicate_names(members: &[SafePathSegment]) -> Result<(), StorageError> {
    if members.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(duplicate_member_error());
    }
    Ok(())
}

fn duplicate_member_error() -> StorageError {
    StorageError::new(StorageErrorKind::InvalidKey, "数据集成员名必须唯一")
}

// The pre-existing L1 tests treated manifest entries like DatasetMember. Keep
// that test-only spelling while the published manifest surface exposes names.
#[cfg(test)]
impl SafePathSegment {
    pub(crate) fn name(&self) -> &Self {
        self
    }
}
