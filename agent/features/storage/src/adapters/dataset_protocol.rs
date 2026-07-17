//! #983 数据集事务的私有磁盘协议。
//!
//! 本模块只负责 manifest 记录与事务 journal 的 serde 编解码，以及若干受
//! cap-std 约束的文件读写工具。协议本身独立于 blob 协议，不复用 AtomicBlobPort。
//!
//! 磁盘布局（每个 DatasetKey 一个目录）：
//! - `dataset.lock`            每 DatasetKey 的 OS 排他锁文件
//! - `primary/manifest.json`   当前代权威 manifest（成员名的完整集合）
//! - `primary/blobs/<成员>`     当前代各成员字节
//! - `previous/...`            完整保留的上一代
//! - `journal.json`            事务 journal（阶段：已准备 → 已提交）
//! - `.stage-<随机数>/...`      新代暂存区
//!
//! manifest 是权威的完整成员集合；一致性读取按 manifest 取回成员字节。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{SafePathSegment, StorageError, StorageErrorKind};

pub(super) const LOCK_FILE: &str = "dataset.lock";
pub(super) const JOURNAL_FILE: &str = "journal.json";
pub(super) const MANIFEST_FILE: &str = "manifest.json";
pub(super) const BLOBS_DIR: &str = "blobs";
pub(super) const PRIMARY_DIR: &str = "primary";
pub(super) const PREVIOUS_DIR: &str = "previous";
pub(super) const PREVIOUS_NEXT_DIR: &str = "previous.next";
/// 持久损坏标记：一旦无法隔离被篡改的 primary 代即落盘此文件。恢复入口据此持续
/// fail-closed，绝不再打开仍在原位的矛盾数据；清除只经显式 quarantine。
pub(super) const CORRUPTION_MARKER: &str = "corruption.marker";

/// 事务阶段：跨越「已提交」即为逻辑提交点。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum JournalPhase {
    #[serde(rename = "已准备")]
    Prepared,
    #[serde(rename = "已提交")]
    Committed,
}

/// 事务种类，供未来 crash recovery 区分前滚语义。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum JournalKind {
    #[serde(rename = "完整替换提交")]
    Commit,
    #[serde(rename = "提升上一代")]
    PromotePrevious,
}

/// 事务 journal 中记录的单个成员：名称 + 新字节的证据摘要 + 字节数 + 修订摘要。
///
/// - `摘要`：`digest_bytes` 领域摘要，恢复时判定 stage / 已发布成员字节是否与已提交
///   的新代一致（物理证据校验）。
/// - `字节数` 与 `修订摘要`：`revision_member_digest`（同 atomic_dataset
///   `MEMBER_BYTES_DOMAIN`），使得恢复时无需原始字节即可用「canonical 名称 + 字节数 +
///   成员摘要」精确重算 `DatasetRevision`，据此语义校验 journal 记录的修订号是否自洽。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct JournalMember {
    #[serde(rename = "名称")]
    pub 名称: String,
    #[serde(rename = "摘要")]
    pub 摘要: String,
    #[serde(rename = "字节数")]
    pub 字节数: u64,
    #[serde(rename = "修订摘要")]
    pub 修订摘要: String,
}

/// 事务 journal 的私有持久化 schema（字段为中文）。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DatasetJournal {
    #[serde(rename = "随机数")]
    pub 随机数: String,
    #[serde(rename = "操作")]
    pub 操作: JournalKind,
    #[serde(rename = "旧修订号")]
    pub 旧修订号: String,
    #[serde(rename = "新修订号")]
    pub 新修订号: String,
    #[serde(rename = "成员集合")]
    pub 成员集合: Vec<JournalMember>,
    #[serde(rename = "阶段")]
    pub 阶段: JournalPhase,
}

/// manifest 的私有持久化 schema（字段为中文）。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DatasetManifestRecord {
    #[serde(rename = "修订号")]
    pub 修订号: String,
    #[serde(rename = "成员集合")]
    pub 成员集合: Vec<String>,
}

/// 将 32 字节修订号编码为十六进制字符串。
pub(super) fn encode_revision(bytes: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 将十六进制字符串复原为 32 字节修订号。
pub(super) fn decode_revision(hex: &str) -> Result<[u8; 32], StorageError> {
    if hex.len() != 64 {
        return Err(StorageError::new(
            StorageErrorKind::Io,
            "manifest 修订号长度非法",
        ));
    }
    let mut bytes = [0u8; 32];
    for (index, slot) in bytes.iter_mut().enumerate() {
        let start = index * 2;
        *slot = u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|_| StorageError::new(StorageErrorKind::Io, "manifest 修订号编码损坏"))?;
    }
    Ok(bytes)
}

/// 是否为恰好 64 位十六进制字符（大小写皆可）。
pub(super) fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 在任何路径拼接之前，对已解码的 journal 做纯结构语义校验。
///
/// 任一不合法即返回 `Err(())`，调用方据此触发 `InvalidJournal` 隔离，且不得先
/// 用 journal 中的任何字段拼接/解析磁盘路径。校验项：
/// - 随机数：安全路径段（无 `/`、`\`、`..`、前导点、`\0`）；
/// - 旧/新修订号：恰好 64 位十六进制；
/// - 成员集合：名称为安全路径段、摘要 64 位十六进制、按名称严格升序且无重复。
pub(super) fn validate_journal_structure(journal: &DatasetJournal) -> Result<(), ()> {
    if SafePathSegment::from_str(&journal.随机数).is_err() {
        return Err(());
    }
    if !is_hex64(&journal.旧修订号) || !is_hex64(&journal.新修订号) {
        return Err(());
    }
    let mut previous: Option<&str> = None;
    for member in &journal.成员集合 {
        if SafePathSegment::from_str(&member.名称).is_err() {
            return Err(());
        }
        if !is_hex64(&member.摘要) || !is_hex64(&member.修订摘要) {
            return Err(());
        }
        if let Some(prev) = previous {
            if prev >= member.名称.as_str() {
                // 非严格升序或存在重复名称。
                return Err(());
            }
        }
        previous = Some(member.名称.as_str());
    }
    Ok(())
}

/// fail-closed：拒绝任何符号链接目标。
///
/// `symlink_metadata` 的错误绝不吞掉：仅 `NotFound`（目标尚不存在）视为可继续，
/// 其余（含权限、I/O）一律上抛，保证中间组件 fail-closed。
pub(super) fn reject_symlink(dir: &Dir, path: &Path) -> Result<(), StorageError> {
    match dir.symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "数据集协议目标是符号链接",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}

/// 目标是否存在（非符号链接才允许）。
pub(super) fn exists(dir: &Dir, path: &Path) -> Result<bool, StorageError> {
    match dir.symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "数据集协议目标是符号链接",
        )),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(map_io(error)),
    }
}

/// 以 create_new 写入一个全新文件（拒绝符号链接）。
pub(super) fn write_new_file(dir: &Dir, path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    reject_symlink(dir, path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = dir.open_with(path, &options).map_err(map_io)?;
    file.write_all(bytes).map_err(map_io)?;
    file.sync_all().map_err(map_sync)?;
    Ok(())
}

/// 读取整个文件字节（拒绝符号链接）。
pub(super) fn read_file(dir: &Dir, path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    reject_symlink(dir, path)?;
    let mut file = match dir.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(error)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(map_io)?;
    Ok(Some(bytes))
}

/// 原子写入 manifest：暂存文件 + rename。
pub(super) fn write_manifest(
    dir: &Dir,
    gen_dir: &Path,
    record: &DatasetManifestRecord,
    nonce: &str,
) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(record).map_err(|error| {
        StorageError::new(StorageErrorKind::Io, format!("manifest 编码失败：{error}"))
    })?;
    let stage = gen_dir.join(format!(".manifest-{nonce}"));
    let target = gen_dir.join(MANIFEST_FILE);
    let _ = dir.remove_file(&stage);
    write_new_file(dir, &stage, &bytes)?;
    reject_symlink(dir, &target)?;
    dir.rename(&stage, dir, &target).map_err(map_io)?;
    Ok(())
}

/// 读取某代 manifest 记录。
pub(super) fn read_manifest(
    dir: &Dir,
    gen_dir: &Path,
) -> Result<Option<DatasetManifestRecord>, StorageError> {
    let path = gen_dir.join(MANIFEST_FILE);
    let Some(bytes) = read_file(dir, &path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| StorageError::new(StorageErrorKind::Io, "manifest 损坏且无法解码"))
}

/// 写入事务 journal：暂存文件 + rename。
pub(super) fn write_journal(dir: &Dir, journal: &DatasetJournal) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(journal).map_err(|error| {
        StorageError::new(StorageErrorKind::Io, format!("事务日志编码失败：{error}"))
    })?;
    let stage = PathBuf::from(format!(".journal-{}", journal.随机数));
    let target = PathBuf::from(JOURNAL_FILE);
    let _ = dir.remove_file(&stage);
    write_new_file(dir, &stage, &bytes)?;
    reject_symlink(dir, &target)?;
    dir.rename(&stage, dir, &target).map_err(map_io)?;
    Ok(())
}

/// 读取事务 journal（若存在）。
///
/// 返回 `Ok(None)` 表示无 journal；`Ok(Some(Ok(_)))` 表示可解析；
/// `Ok(Some(Err(_)))` 表示 journal 存在但无法解码（应触发 InvalidJournal 隔离）。
#[allow(clippy::type_complexity)]
pub(super) fn read_journal_raw(
    dir: &Dir,
) -> Result<Option<Result<DatasetJournal, ()>>, StorageError> {
    let path = PathBuf::from(JOURNAL_FILE);
    let Some(bytes) = read_file(dir, &path)? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_slice(&bytes).map_err(|_| ())))
}

/// 是否存在已隔离的 journal 证据。
///
/// `.corrupt` journal 本身是一次未被显式处置的持久损坏屏障；即使活动 journal 已经
/// 不在原位，恢复也不得把它误判为「无事务」并清理/打开 primary。
pub(super) fn has_corrupt_journal(dir: &Dir) -> Result<bool, StorageError> {
    let prefix = format!("{JOURNAL_FILE}.corrupt.");
    for entry in dir.entries().map_err(map_io)? {
        let name = entry.map_err(map_io)?.file_name();
        if name.to_string_lossy().starts_with(&prefix) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 计算数据集成员字节的十六进制摘要（与 blob 协议独立的领域分隔符）。
pub(super) fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aemeath.storage.dataset.member.digest.v1\0");
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 读取文件并计算摘要；文件缺失返回 `Ok(None)`。
pub(super) fn digest_file(dir: &Dir, path: &Path) -> Result<Option<String>, StorageError> {
    match read_file(dir, path)? {
        Some(bytes) => Ok(Some(digest_bytes(&bytes))),
        None => Ok(None),
    }
}

/// 计算成员字节参与 `DatasetRevision` 运算的领域摘要（十六进制），与
/// atomic_dataset `MEMBER_BYTES_DOMAIN` 算法一致。持久化进 journal 后，恢复时无需
/// 原始字节即可精确重算修订号。
pub(super) fn revision_member_digest_hex(bytes: &[u8]) -> String {
    encode_revision(&crate::domain::revision_member_digest(bytes))
}

/// 删除事务 journal。
pub(super) fn remove_journal(dir: &Dir) -> Result<(), StorageError> {
    match dir.remove_file(JOURNAL_FILE) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}

/// sync 数据集目录，落实 rename 等元数据变更。
pub(super) fn sync_dir(dir: &Dir) -> Result<(), StorageError> {
    let mut options = OpenOptions::new();
    options.read(true);
    dir.open_with(".", &options)
        .and_then(|handle| handle.sync_all())
        .map_err(map_sync)
}

/// no-follow 打开子目录并 `sync_all`，落实其内部条目的元数据变更。
///
/// 先 `reject_symlink` 保证不跟随符号链接；子目录缺失（`NotFound`）视为无需 sync。
pub(super) fn sync_subdir(dir: &Dir, path: &Path) -> Result<(), StorageError> {
    reject_symlink(dir, path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    match dir.open_with(path, &options) {
        Ok(handle) => handle.sync_all().map_err(map_sync),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}

/// 自底向上同步一个完整代目录：`<代>/blobs` → `<代>` → 顶层。
///
/// 保证代内 blob 文件、blobs 目录条目、代目录条目与顶层条目全部落盘，
/// 是「跨越 Prepared 前证明 stage/previous 完整 durable」的基础原语。
pub(super) fn sync_generation(dir: &Dir, gen_dir: &Path) -> Result<(), StorageError> {
    sync_subdir(dir, &gen_dir.join(BLOBS_DIR))?;
    sync_subdir(dir, gen_dir)?;
    sync_dir(dir)
}

pub(super) fn map_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageErrorKind::PermissionDenied
    } else {
        StorageErrorKind::Io
    };
    StorageError::new(kind, format!("数据集 I/O 失败：{error}"))
}

pub(super) fn map_lock_io(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::ConcurrentWrite,
        format!("数据集锁获取失败：{error}"),
    )
}

pub(super) fn map_sync(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::UnsupportedDurability,
        format!("当前平台无法兑现数据集持久性：{error}"),
    )
}
