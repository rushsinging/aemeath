//! reflection-history 真 append-only jsonl 落盘适配器（`JsonlReflectionHistoryStore`）。
//!
//! 对应设计「生产事件流与 append-only 历史」§4.2：
//!
//! - 路径：`memory/{project_key}/reflection-history/{yyyy-mm-dd}.jsonl`（UTC 日切，
//!   segment 均经 [`SafePathSegmentData`]），每行一条完整 `ReflectionRecord` JSON；
//! - **`append` 与 `upsert` 均只追加一行**：原地替换已取消，同 id 再写入即再追加，
//!   latest-wins 由读侧折叠实现；
//! - **`list` / `list_with_content`**：扫描现有日切 segment（新→旧、文件内后写在前），
//!   按 `id` 折叠为最新一条后截断 `limit`，以 newest-first 投影 Safe 摘要；
//! - **legacy 迁移**：首个读写若发现旧 AtomicDataset member `records` 且尚无 jsonl
//!   segment，一次性按 `record.timestamp`（UTC 秒）分日导出，随后 legacy dataset
//!   不再被读写（等价只读/可删）；迁移任何失败 **fail-open**（`log::warn` 后继续
//!   空 / jsonl 视图）；
//! - **保留**：与事件流共用默认 30 天（`retention_days == 0` 禁用 GC），过期日切
//!   segment 删除后折叠视图自然变短；
//! - 失败语义：当前 jsonl 行损坏 = 数据损坏 → fail closed（`Serialization`）；
//!   IO 只出现在本适配器模块（domain 保持零 IO）。

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use storage::{
    AtomicDatasetPort, DatasetKeyData, DatasetMemberData, DatasetReadOutcomeData, SafeOpenOptions,
    SafePathSegmentData, SafeStorageDir, SafeStorageEntryData, SafeStorageFileType,
    SafeStorageRoot, StorageNamespaceData,
};

use crate::adapters::map_storage_error;
use crate::constants::{
    LOG_TARGET, REFLECTION_HISTORY_JSONL_SUFFIX, REFLECTION_HISTORY_SEGMENT,
    REFLECTION_RECORDS_MEMBER,
};
use crate::domain::{
    MemoryError, MemoryStorageErrorKind, ProjectMemoryKey, ReflectionRecord, ReflectionSafeSummary,
};
use crate::ports::{ReflectionHistoryQuery, ReflectionHistoryStore};

/// 旧 AtomicDataset `records` member 的一次性导出源（dataset key
/// `memory/{project}/reflection-history`，与 jsonl 目录同路径共存）。
#[derive(Clone)]
struct LegacyHistoryDataset {
    storage: Arc<dyn AtomicDatasetPort>,
    dataset: DatasetKeyData,
}

#[derive(Clone)]
struct HistoryShared {
    root: SafeStorageRoot,
    /// `memory` + `{project_key}` + `reflection-history` 三段目录（`new` 时预构造）。
    segments: Vec<SafePathSegmentData>,
    /// GC 配置天数（`0` = 禁用 GC）；`gc` 免参入口读取，显式入口见
    /// [`JsonlReflectionHistoryStore::gc_expired`]。
    retention_days: u32,
    /// legacy 迁移导出源（`None` = 无 legacy 需要迁移）。
    legacy: Option<LegacyHistoryDataset>,
    /// legacy 迁移一次性闸门：每 store 实例至多尝试一次，成败均置位（fail-open）。
    legacy_checked: Arc<AtomicBool>,
    /// legacy 迁移互斥：并发首个读写不重复导出。
    legacy_lock: Arc<tokio::sync::Mutex<()>>,
    /// 进程内 store 级互斥：跨文件序列化 append/GC/fold，保证单行完整不被交错。
    store_lock: Arc<Mutex<()>>,
}

/// 与其余 wire 实现体一致：composition 构造经 crate 根
/// `wire_reflection_history_store`，`new` 收窄 `pub(crate)`。
#[derive(Clone)]
pub(crate) struct JsonlReflectionHistoryStore {
    shared: Arc<HistoryShared>,
}

impl JsonlReflectionHistoryStore {
    pub(crate) fn new(
        root: SafeStorageRoot,
        project: ProjectMemoryKey,
        retention_days: u32,
        legacy: Option<Arc<dyn AtomicDatasetPort>>,
    ) -> Self {
        let memory = SafePathSegmentData::from_str(StorageNamespaceData::Memory.as_str())
            .expect("Memory namespace is a safe Storage path segment");
        let project = SafePathSegmentData::from_str(project.as_str())
            .expect("derived project Memory key is a safe Storage path segment");
        let history = SafePathSegmentData::from_str(REFLECTION_HISTORY_SEGMENT)
            .expect("fixed reflection-history segment is a safe Storage path segment");
        let legacy = legacy.map(|storage| {
            let dataset = DatasetKeyData::new(
                StorageNamespaceData::Memory,
                vec![project.clone(), history.clone()],
            )
            .expect("Reflection history segments form a valid dataset key");
            LegacyHistoryDataset { storage, dataset }
        });
        Self {
            shared: Arc::new(HistoryShared {
                root,
                segments: vec![memory, project, history],
                retention_days,
                legacy,
                legacy_checked: Arc::new(AtomicBool::new(false)),
                legacy_lock: Arc::new(tokio::sync::Mutex::new(())),
                store_lock: Arc::new(Mutex::new(())),
            }),
        }
    }

    // ---------- 错误与序列化 ----------

    fn storage_failure(error: storage::StorageError) -> MemoryError {
        MemoryError::Storage {
            kind: map_storage_error(&error),
        }
    }

    fn storage_kind(kind: MemoryStorageErrorKind) -> MemoryError {
        MemoryError::Storage { kind }
    }

    fn lock_failure() -> MemoryError {
        Self::storage_kind(MemoryStorageErrorKind::Io)
    }

    /// 序列化记录为单行 payload：一条完整 JSON + 行尾 `\n`；行内出现第二个
    /// `\n` 即拒绝（`ReflectionRecord` 不携带原始正文，出现即编码损坏）。
    fn encode_line(record: &ReflectionRecord) -> Result<Vec<u8>, MemoryError> {
        let mut bytes = serde_json::to_vec(record)
            .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Serialization))?;
        bytes.push(b'\n');
        let Some((last, body)) = bytes.split_last() else {
            return Err(Self::storage_kind(MemoryStorageErrorKind::Serialization));
        };
        if *last != b'\n' || body.contains(&b'\n') {
            return Err(Self::storage_kind(MemoryStorageErrorKind::Serialization));
        }
        Ok(bytes)
    }

    /// `ReflectionRecord::timestamp`（UTC 秒）所在 UTC 日（`yyyy-mm-dd`）。
    fn day_of_timestamp(timestamp_secs: u64) -> Result<String, MemoryError> {
        let millis = i64::try_from(timestamp_secs)
            .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Serialization))?
            .saturating_mul(1000);
        chrono::DateTime::from_timestamp_millis(millis)
            .map(|ts| ts.format("%Y-%m-%d").to_string())
            .ok_or_else(|| Self::storage_kind(MemoryStorageErrorKind::Serialization))
    }

    /// 当前 UTC 日（供 GC 以挂钟时间计算超期）。
    fn today_of(now_unix_ms: u64) -> Result<chrono::NaiveDate, MemoryError> {
        chrono::DateTime::from_timestamp_millis(i64::try_from(now_unix_ms).unwrap_or(i64::MAX))
            .map(|ts| ts.date_naive())
            .ok_or_else(|| Self::storage_kind(MemoryStorageErrorKind::Serialization))
    }

    // ---------- 目录与文件 ----------

    fn history_dir(&self) -> Result<SafeStorageDir, MemoryError> {
        self.shared
            .root
            .ensure_dir(&self.shared.segments)
            .map_err(Self::storage_failure)
    }

    fn segment_file_name(day: &str) -> Result<SafePathSegmentData, MemoryError> {
        SafePathSegmentData::from_str(&format!("{day}{REFLECTION_HISTORY_JSONL_SUFFIX}"))
            .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Serialization))
    }

    /// 可解析的日切 jsonl segment（文件名降序 = 新→旧）；无法按
    /// `{yyyy-mm-dd}.jsonl` 解析的条目一律跳过（含 legacy dataset 的
    /// `primary/` 等目录与事务工件）。
    fn ordered_segments(dir: &SafeStorageDir) -> Result<Vec<SafeStorageEntryData>, MemoryError> {
        let mut entries: Vec<SafeStorageEntryData> = dir
            .entries()
            .map_err(Self::storage_failure)?
            .into_iter()
            .filter(|entry| {
                entry.file_type() == SafeStorageFileType::RegularFile
                    && entry
                        .name()
                        .as_str()
                        .strip_suffix(REFLECTION_HISTORY_JSONL_SUFFIX)
                        .is_some_and(|stem| {
                            chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d").is_ok()
                        })
            })
            .collect();
        entries.sort_by(|a, b| b.name().as_str().cmp(a.name().as_str()));
        Ok(entries)
    }

    /// 同步追加：持 store 互斥 → ensure_dir → create_or_open(append+read)
    /// → `write_all`。返回错误不携带正文。
    fn append_line(&self, day: &str, bytes: &[u8]) -> Result<(), MemoryError> {
        let _guard = self
            .shared
            .store_lock
            .lock()
            .map_err(|_| Self::lock_failure())?;
        let dir = self.history_dir()?;
        let name = Self::segment_file_name(day)?;
        let mut file = dir
            .create_or_open(
                &name,
                SafeOpenOptions {
                    read: true,
                    append: true,
                },
            )
            .map_err(Self::storage_failure)?;
        file.write_all(bytes)
            .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Io))
    }

    /// 是否已存在可解析的日切 jsonl segment（legacy 迁移的重复导出闸门）。
    fn has_segments(&self) -> Result<bool, MemoryError> {
        let _guard = self
            .shared
            .store_lock
            .lock()
            .map_err(|_| Self::lock_failure())?;
        let dir = self.history_dir()?;
        Ok(!Self::ordered_segments(&dir)?.is_empty())
    }

    // ---------- 读侧折叠 ----------

    /// 读侧折叠：现有 segment 新→旧、文件内后写在前地扫描，按 `id` **先见者胜**
    /// （先见 = 位置更新），得到 newest-append-first 的每 id 最新视图，截断 `limit`
    /// 后投影 Safe 摘要。当前 jsonl 行损坏 → `Serialization`（fail closed）。
    fn fold_latest(
        &self,
        limit: usize,
        with_content: bool,
    ) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        let _guard = self
            .shared
            .store_lock
            .lock()
            .map_err(|_| Self::lock_failure())?;
        let dir = self.history_dir()?;
        let mut folded: Vec<ReflectionRecord> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for entry in Self::ordered_segments(&dir)? {
            let file = dir
                .open_existing(
                    entry.name(),
                    SafeOpenOptions {
                        read: true,
                        append: false,
                    },
                )
                .map_err(Self::storage_failure)?;
            let mut lines = Vec::new();
            for line in BufReader::new(file).lines() {
                lines.push(line.map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Io))?);
            }
            for line in lines.iter().rev() {
                if line.is_empty() {
                    continue;
                }
                let record: ReflectionRecord = serde_json::from_str(line)
                    .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Serialization))?;
                if seen.insert(record.id.clone()) {
                    folded.push(record);
                }
                if folded.len() >= limit && limit > 0 {
                    break;
                }
            }
            if folded.len() >= limit && limit > 0 {
                break;
            }
        }
        folded.truncate(limit);
        Ok(folded
            .into_iter()
            .map(|record| {
                if with_content {
                    record.safe_summary_with_content()
                } else {
                    record.safe_summary()
                }
            })
            .collect())
    }

    // ---------- legacy 一次性迁移（fail-open） ----------

    /// 首个读写的迁移闸门（双重检查 + 实例内一次性）：成功或失败均置位，
    /// 失败仅 `log::warn`（无正文）后放行当前操作。
    async fn ensure_legacy_migrated(&self) -> Result<(), MemoryError> {
        if self.shared.legacy_checked.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(legacy) = self.shared.legacy.clone() else {
            self.shared.legacy_checked.store(true, Ordering::Release);
            return Ok(());
        };
        let _gate = self.shared.legacy_lock.lock().await;
        if self.shared.legacy_checked.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Err(error) = self.export_legacy(&legacy).await {
            log::warn!(
                target: LOG_TARGET,
                "reflection_history_legacy_migration_failed error={error}"
            );
        }
        self.shared.legacy_checked.store(true, Ordering::Release);
        Ok(())
    }

    /// 一次性导出：仅当 legacy dataset 存在 member `records` 且 jsonl 尚无
    /// segment 时执行；导出完成后 legacy dataset 不再被读写（只读/可删语义）。
    /// 任何读取/序列化/落盘失败向上返回，由调用方 fail-open。
    async fn export_legacy(&self, legacy: &LegacyHistoryDataset) -> Result<(), MemoryError> {
        let store = self.clone();
        let has_jsonl = tokio::task::spawn_blocking(move || store.has_segments())
            .await
            .map_err(|_| Self::lock_failure())??;
        if has_jsonl {
            // jsonl 已有内容 ⇒ 视为已迁移，绝不重复导出。
            return Ok(());
        }
        let manifest = legacy
            .storage
            .read_manifest(&legacy.dataset)
            .await
            .map_err(Self::storage_failure)?;
        if manifest.members().is_empty() {
            return Ok(());
        }
        let records_member = SafePathSegmentData::from_str(REFLECTION_RECORDS_MEMBER)
            .expect("fixed records member name is safe");
        if manifest.members() != std::slice::from_ref(&records_member) {
            return Err(Self::storage_kind(
                MemoryStorageErrorKind::CorruptTransaction,
            ));
        }
        let revision = manifest.revision().clone();
        let read = legacy
            .storage
            .read_consistent(&legacy.dataset, std::slice::from_ref(&records_member))
            .await
            .map_err(Self::storage_failure)?;
        let DatasetReadOutcomeData::Found(read) = read else {
            return Err(Self::storage_kind(
                MemoryStorageErrorKind::CorruptTransaction,
            ));
        };
        if read.revision() != &revision {
            return Err(Self::storage_kind(MemoryStorageErrorKind::ConcurrentWrite));
        }
        let bytes = read
            .members()
            .first()
            .filter(|member| member.name() == &records_member)
            .map(DatasetMemberData::bytes)
            .ok_or_else(|| Self::storage_kind(MemoryStorageErrorKind::CorruptTransaction))?;
        let records: Vec<ReflectionRecord> = serde_json::from_slice(bytes)
            .map_err(|_| Self::storage_kind(MemoryStorageErrorKind::Serialization))?;
        if records.is_empty() {
            return Ok(());
        }
        let lines = records
            .iter()
            .map(|record| {
                let day = Self::day_of_timestamp(record.timestamp)?;
                let bytes = Self::encode_line(record)?;
                Ok::<_, MemoryError>((day, bytes))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let migrated = lines.len();
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            for (day, bytes) in lines {
                store.append_line(&day, &bytes)?;
            }
            Ok::<(), MemoryError>(())
        })
        .await
        .map_err(|_| Self::lock_failure())??;
        log::warn!(
            target: LOG_TARGET,
            "reflection_history_legacy_migrated records={migrated}"
        );
        Ok(())
    }

    // ---------- GC ----------

    /// GC 日切 segment：删除文件名超龄（`(today - stem).days >= retention_days`）
    /// 的 `.jsonl` 文件，返回删除数。`retention_days == 0` 表示**禁用 GC**
    /// （从不删除，也不阻断写入）；无法按 `yyyy-mm-dd` 解析的文件名一律跳过。
    pub(crate) fn gc_expired(
        &self,
        now_unix_ms: u64,
        retention_days: u32,
    ) -> Result<usize, MemoryError> {
        if retention_days == 0 {
            return Ok(0);
        }
        let today = Self::today_of(now_unix_ms)?;
        let _guard = self
            .shared
            .store_lock
            .lock()
            .map_err(|_| Self::lock_failure())?;
        let dir = self.history_dir()?;
        let mut deleted = 0usize;
        for entry in Self::ordered_segments(&dir)? {
            let stem = entry
                .name()
                .as_str()
                .strip_suffix(REFLECTION_HISTORY_JSONL_SUFFIX)
                .expect("ordered_segments yields suffixed names");
            let Ok(stem_date) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                continue;
            };
            if (today - stem_date).num_days() >= i64::from(retention_days) {
                dir.remove_file(entry.name())
                    .map_err(Self::storage_failure)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// 以构造时配置的 `retention_days` 执行 GC 的免参入口（composition /
    /// 周期触发点使用；配置接线落地前仅测试引用，放行 dead_code）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn gc(&self, now_unix_ms: u64) -> Result<usize, MemoryError> {
        self.gc_expired(now_unix_ms, self.shared.retention_days)
    }

    // ---------- 读写公共入口 ----------

    async fn list_projecting(
        &self,
        limit: usize,
        with_content: bool,
    ) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        self.ensure_legacy_migrated().await?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.fold_latest(limit, with_content))
            .await
            .map_err(|_| Self::lock_failure())?
    }
}

#[async_trait]
impl ReflectionHistoryStore for JsonlReflectionHistoryStore {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        self.ensure_legacy_migrated().await?;
        let bytes = Self::encode_line(record)?;
        let day = Self::day_of_timestamp(record.timestamp)?;
        // 阻塞 IO 走 blocking 池（对齐 JsonlSegmentEventStore 先例）。
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.append_line(&day, &bytes))
            .await
            .map_err(|_| Self::lock_failure())?
    }

    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        // 原地替换已取消：同 id 再写入 = 再 append 一行，latest-wins 由读侧折叠实现。
        self.append(record).await
    }
}

#[async_trait]
impl ReflectionHistoryQuery for JsonlReflectionHistoryStore {
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        self.list_projecting(limit, false).await
    }

    async fn list_with_content(
        &self,
        limit: usize,
    ) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        self.list_projecting(limit, true).await
    }
}

#[cfg(test)]
#[path = "reflection_history_jsonl_tests.rs"]
mod tests;
