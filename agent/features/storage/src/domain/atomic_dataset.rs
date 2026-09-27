use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest, Sha256};

use super::{
    CommitWarningData, SafePathSegmentData, StorageError, StorageErrorKind, StorageNamespaceData,
};

const REVISION_DOMAIN: &[u8] = b"aemeath.storage.dataset.revision.v1\0";
const MEMBER_BYTES_DOMAIN: &[u8] = b"aemeath.storage.dataset.member.bytes.v1\0";

/// 计算单个成员字节参与修订号运算的领域摘要（`MEMBER_BYTES_DOMAIN`）。
///
/// 与 `DatasetRevisionData::from_canonical_members` 内联的成员摘要算法严格一致：
/// `SHA256(MEMBER_BYTES_DOMAIN || len_le64 || bytes)`。adapter 将其持久化进事务
/// journal，使得恢复时不需要原始字节即可精确重算 `DatasetRevisionData`。
pub(crate) fn revision_member_digest(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(MEMBER_BYTES_DOMAIN);
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    digest.finalize().into()
}

/// The adapter-independent logical location of an atomic dataset.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DatasetKeyData {
    namespace: StorageNamespaceData,
    segments: Vec<SafePathSegmentData>,
}

impl DatasetKeyData {
    pub fn new(
        namespace: StorageNamespaceData,
        segments: Vec<SafePathSegmentData>,
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

    pub fn namespace(&self) -> StorageNamespaceData {
        self.namespace
    }

    pub fn segments(&self) -> &[SafePathSegmentData] {
        &self.segments
    }
}

/// One named byte value supplied to a dataset commit or returned by a read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetMemberData {
    name: SafePathSegmentData,
    bytes: Vec<u8>,
}

impl DatasetMemberData {
    pub fn new(name: SafePathSegmentData, bytes: Vec<u8>) -> Self {
        Self { name, bytes }
    }

    pub fn name(&self) -> &SafePathSegmentData {
        &self.name
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// An immutable member that an incremental generation reuses from the expected primary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetMemberReferenceData {
    source_revision: DatasetRevisionData,
    name: SafePathSegmentData,
    byte_len: u64,
    member_digest: [u8; 32],
}

impl DatasetMemberReferenceData {
    pub fn from_manifest_member(
        source_revision: DatasetRevisionData,
        name: SafePathSegmentData,
        byte_len: u64,
        member_digest: [u8; 32],
    ) -> Self {
        Self {
            source_revision,
            name,
            byte_len,
            member_digest,
        }
    }

    pub fn source_revision(&self) -> &DatasetRevisionData {
        &self.source_revision
    }

    pub fn name(&self) -> &SafePathSegmentData {
        &self.name
    }

    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub fn member_digest(&self) -> &[u8; 32] {
        &self.member_digest
    }

    pub fn matches_bytes(&self, bytes: &[u8]) -> bool {
        self.byte_len == bytes.len() as u64 && self.member_digest == revision_member_digest(bytes)
    }
}

/// A byte-bearing member change for an incremental dataset generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetMemberChangeData {
    Replace(DatasetMemberData),
}

impl DatasetMemberChangeData {
    pub fn member(&self) -> &DatasetMemberData {
        match self {
            Self::Replace(member) => member,
        }
    }
}

/// A complete target-generation description that carries bytes only for changed members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetChangeSetData {
    expected_revision: DatasetRevisionData,
    new_members: Vec<DatasetMemberChangeData>,
    reused_members: Vec<DatasetMemberReferenceData>,
    removed_members: Vec<SafePathSegmentData>,
}

impl DatasetChangeSetData {
    pub fn new(
        expected_revision: DatasetRevisionData,
        mut new_members: Vec<DatasetMemberChangeData>,
        mut reused_members: Vec<DatasetMemberReferenceData>,
    ) -> Result<Self, StorageError> {
        new_members.sort_by(|left, right| left.member().name().cmp(right.member().name()));
        reused_members.sort_by(|left, right| left.name.cmp(&right.name));
        let mut names = new_members
            .iter()
            .map(|change| change.member().name())
            .chain(reused_members.iter().map(|member| member.name()))
            .collect::<Vec<_>>();
        names.sort();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(duplicate_member_error());
        }
        if reused_members
            .iter()
            .any(|member| member.source_revision != expected_revision)
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "复用成员必须来自期望的数据集修订号",
            ));
        }
        Ok(Self {
            expected_revision,
            new_members,
            reused_members,
            removed_members: Vec::new(),
        })
    }

    pub fn with_removed_members(
        mut self,
        mut removed_members: Vec<SafePathSegmentData>,
    ) -> Result<Self, StorageError> {
        removed_members.sort();
        reject_duplicate_names(&removed_members)?;
        let target_names = self
            .new_members
            .iter()
            .map(|change| change.member().name())
            .chain(self.reused_members.iter().map(|member| member.name()))
            .collect::<BTreeSet<_>>();
        if removed_members
            .iter()
            .any(|name| target_names.contains(name))
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "同一数据集成员不能同时保留和删除",
            ));
        }
        self.removed_members = removed_members;
        Ok(self)
    }

    pub fn expected_revision(&self) -> &DatasetRevisionData {
        &self.expected_revision
    }

    pub fn new_members(&self) -> &[DatasetMemberChangeData] {
        &self.new_members
    }

    pub fn reused_members(&self) -> &[DatasetMemberReferenceData] {
        &self.reused_members
    }

    pub fn removed_members(&self) -> &[SafePathSegmentData] {
        &self.removed_members
    }
}

/// A Storage-generated opaque fingerprint of a complete dataset generation.
///
/// The fingerprint is deliberately redacted from `Debug` so that raw
/// generation bytes never leak into logs, panics, or receipts.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct DatasetRevisionData([u8; 32]);

impl fmt::Debug for DatasetRevisionData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatasetRevisionData(<redacted>)")
    }
}

impl DatasetRevisionData {
    /// 供 adapter 将修订号持久化到私有 schema（十六进制）后再复原。
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 供 adapter 从持久化的权威 manifest 复原修订号。仅可用于同一完整代先前生成的字节。
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn from_canonical_members(members: &[DatasetMemberData]) -> Self {
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

/// Storage's authoritative member manifest for one complete generation.
///
/// Member bytes intentionally are not exposed by this discovery value. Each
/// member instead carries Storage-verified reuse evidence that can be passed
/// directly to an incremental commit for the reported revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetManifestData {
    revision: DatasetRevisionData,
    members: Vec<SafePathSegmentData>,
    member_evidence: Vec<DatasetMemberReferenceData>,
}

impl DatasetManifestData {
    /// Freezes a complete generation into canonical member-name order.
    pub(crate) fn new(mut members: Vec<DatasetMemberData>) -> Result<Self, StorageError> {
        canonicalize_members(&mut members)?;
        let revision = DatasetRevisionData::from_canonical_members(&members);
        let member_evidence = members
            .iter()
            .map(|member| {
                DatasetMemberReferenceData::from_manifest_member(
                    revision.clone(),
                    member.name.clone(),
                    member.bytes.len() as u64,
                    revision_member_digest(&member.bytes),
                )
            })
            .collect();
        let members = members.into_iter().map(|member| member.name).collect();
        Ok(Self {
            revision,
            members,
            member_evidence,
        })
    }

    /// Reconstitutes a manifest from Storage-owned persisted facts.
    ///
    /// Adapters must only use evidence verified under the same dataset lock as
    /// the complete generation represented by `revision`.
    pub(crate) fn from_verified_members(
        revision: DatasetRevisionData,
        mut member_evidence: Vec<DatasetMemberReferenceData>,
    ) -> Result<Self, StorageError> {
        member_evidence.sort_by(|left, right| left.name.cmp(&right.name));
        let members = member_evidence
            .iter()
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        reject_duplicate_names(&members)?;
        if member_evidence
            .iter()
            .any(|member| member.source_revision != revision)
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "manifest 成员证据必须来自同一数据集修订号",
            ));
        }
        Ok(Self {
            revision,
            members,
            member_evidence,
        })
    }

    pub fn revision(&self) -> &DatasetRevisionData {
        &self.revision
    }

    pub fn members(&self) -> &[SafePathSegmentData] {
        &self.members
    }

    pub fn member_evidence(
        &self,
        name: &SafePathSegmentData,
    ) -> Option<&DatasetMemberReferenceData> {
        self.member_evidence
            .binary_search_by(|member| member.name.cmp(name))
            .ok()
            .map(|index| &self.member_evidence[index])
    }

    /// Returns names present in this manifest but absent from its replacement.
    pub fn omitted_members<'a>(&'a self, replacement: &Self) -> Vec<&'a SafePathSegmentData> {
        let replacement_names: BTreeSet<&SafePathSegmentData> =
            replacement.members.iter().collect();
        self.members
            .iter()
            .filter(|name| !replacement_names.contains(name))
            .collect()
    }
}

/// A revision and requested member bytes read under one dataset lock.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetReadData {
    revision: DatasetRevisionData,
    members: Vec<DatasetMemberData>,
}

impl DatasetReadData {
    pub(crate) fn new(
        revision: DatasetRevisionData,
        mut members: Vec<DatasetMemberData>,
    ) -> Result<Self, StorageError> {
        canonicalize_members(&mut members)?;
        Ok(Self { revision, members })
    }

    pub fn revision(&self) -> &DatasetRevisionData {
        &self.revision
    }

    pub fn members(&self) -> &[DatasetMemberData] {
        &self.members
    }
}

/// Result of reading a requested member set from one explicit generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetReadOutcomeData {
    Found(DatasetReadData),
    NotFound,
}

/// Whether a logically committed generation is already externally visible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetCommitVisibilityData {
    Visible,
    RecoveryPending,
}

/// Proof that a dataset generation crossed its logical commit point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetCommitReceiptData {
    revision: DatasetRevisionData,
    visibility: DatasetCommitVisibilityData,
    warning: Option<CommitWarningData>,
}

impl DatasetCommitReceiptData {
    pub(crate) fn committed(
        revision: DatasetRevisionData,
        visibility: DatasetCommitVisibilityData,
        warning: Option<CommitWarningData>,
    ) -> Self {
        Self {
            revision,
            visibility,
            warning,
        }
    }

    pub fn revision(&self) -> &DatasetRevisionData {
        &self.revision
    }

    pub fn visibility(&self) -> DatasetCommitVisibilityData {
        self.visibility
    }

    pub fn warning(&self) -> Option<CommitWarningData> {
        self.warning
    }
}

fn canonicalize_members(members: &mut [DatasetMemberData]) -> Result<(), StorageError> {
    members.sort_by(|left, right| left.name.cmp(&right.name));
    if members.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(duplicate_member_error());
    }
    Ok(())
}

fn reject_duplicate_names(members: &[SafePathSegmentData]) -> Result<(), StorageError> {
    if members.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(duplicate_member_error());
    }
    Ok(())
}

fn duplicate_member_error() -> StorageError {
    StorageError::new(StorageErrorKind::InvalidKey, "数据集成员名必须唯一")
}
