//! 生产事件流的 append-only jsonl 落盘适配器（`JsonlSegmentEventStore`）。
//!
//! 对应设计「生产事件流与 append-only 历史」§4.1：
//!
//! - 路径：`memory/{project_key}/events/{yyyy-mm-dd}.jsonl`（UTC 日切，
//!   segment 均经 [`SafePathSegmentData`]）；
//! - 写入：`ensure_dir` → `create_or_open`（append+read）→ 一行一条完整
//!   `MemoryEvent` JSON + `\n`；进程内 store 级互斥保证行完整；
//! - 保留：`gc_expired` 按日切文件名删除超窗 segment，`retention_days == 0`
//!   表示**禁用 GC**（从不删除），NEVER 表示关闭事件写入；
//! - 失败语义：错误映射为不携带记忆正文的 [`EventAppendError`]；
//! - IO 只出现在本适配器模块（domain 保持零 IO）。
//!
//! 读回（`read_all_for_test`）仅测试构建存在，**NEVER** 经 `memory::api` 暴露。

use std::io::Write;
#[cfg(test)]
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use storage::{
    SafeOpenOptions, SafePathSegmentData, SafeStorageDir, SafeStorageFileType, SafeStorageRoot,
    StorageErrorKind, StorageNamespaceData,
};

#[cfg(test)]
use crate::constants::DEFAULT_EVENT_RETENTION_DAYS;
use crate::constants::{EVENTS_JSONL_SUFFIX, EVENTS_SEGMENT};
use crate::domain::event::MemoryEvent;
use crate::domain::ProjectMemoryKey;
use crate::ports::{EventAppendError, MemoryEventAppendPort};

/// 与其余 wire 实现体一致：composition 构造经 crate 根 `wire_memory_event_store`，
/// `new` 收窄 `pub(crate)`。
#[derive(Clone)]
pub(crate) struct JsonlSegmentEventStore {
    root: SafeStorageRoot,
    /// `memory` + `{project_key}` + `events` 三段目录（`new` 时预构造）。
    segments: Vec<SafePathSegmentData>,
    /// GC 配置天数（`0` = 禁用 GC）；`gc` 免参入口读取，显式入口见
    /// [`JsonlSegmentEventStore::gc_expired`]。
    retention_days: u32,
    /// 进程内 store 级互斥：跨文件序列化 append/GC，保证单行完整不被交错。
    store_lock: Arc<Mutex<()>>,
}

impl JsonlSegmentEventStore {
    pub(crate) fn new(
        root: SafeStorageRoot,
        project: ProjectMemoryKey,
        retention_days: u32,
    ) -> Self {
        let memory = SafePathSegmentData::from_str(StorageNamespaceData::Memory.as_str())
            .expect("Memory namespace is a safe Storage path segment");
        let project = SafePathSegmentData::from_str(project.as_str())
            .expect("derived project Memory key is a safe Storage path segment");
        let events = SafePathSegmentData::from_str(EVENTS_SEGMENT)
            .expect("fixed events segment is a safe Storage path segment");
        Self {
            root,
            segments: vec![memory, project, events],
            retention_days,
            store_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 事件行校验（与 Audit append 同口径）：非空、以 `\n` 结尾、行内无第二个
    /// `\n`。返回错误的 Display 固定文案，绝不携带 payload。
    fn validate_line(bytes: &[u8]) -> Result<(), EventAppendError> {
        let Some((last, body)) = bytes.split_last() else {
            return Err(EventAppendError::Rejected);
        };
        if *last != b'\n' || body.contains(&b'\n') {
            return Err(EventAppendError::Rejected);
        }
        Ok(())
    }

    /// 序列化事件为单行 payload：一条完整 JSON + 行尾 `\n`，随后按
    /// [`Self::validate_line`] 校验。
    fn encode_line(event: &MemoryEvent) -> Result<Vec<u8>, EventAppendError> {
        let mut bytes = serde_json::to_vec(event).map_err(|_| EventAppendError::Serialization)?;
        bytes.push(b'\n');
        Self::validate_line(&bytes)?;
        Ok(bytes)
    }

    /// 事件 `ts_unix_ms` 所在 UTC 日（`yyyy-mm-dd`）。
    fn day_of(ts_unix_ms: u64) -> Result<String, EventAppendError> {
        chrono::DateTime::from_timestamp_millis(ts_unix_ms as i64)
            .map(|ts| ts.format("%Y-%m-%d").to_string())
            .ok_or(EventAppendError::Rejected)
    }

    fn map_storage_error(error: storage::StorageError) -> EventAppendError {
        match error.kind() {
            StorageErrorKind::InvalidKey => EventAppendError::Rejected,
            _ => EventAppendError::Io,
        }
    }

    fn events_dir(&self) -> Result<SafeStorageDir, EventAppendError> {
        self.root
            .ensure_dir(&self.segments)
            .map_err(Self::map_storage_error)
    }

    fn segment_file_name(day: &str) -> Result<SafePathSegmentData, EventAppendError> {
        SafePathSegmentData::from_str(&format!("{day}{EVENTS_JSONL_SUFFIX}"))
            .map_err(|_| EventAppendError::Rejected)
    }

    /// 同步追加：持 store 互斥 → ensure_dir → create_or_open(append+read)
    /// → `write_all`。返回错误不携带正文。
    fn append_line(&self, day: &str, bytes: &[u8]) -> Result<(), EventAppendError> {
        let _guard = self
            .store_lock
            .lock()
            .map_err(|_| EventAppendError::Unavailable)?;
        let dir = self.events_dir()?;
        let name = Self::segment_file_name(day)?;
        let mut file = dir
            .create_or_open(
                &name,
                SafeOpenOptions {
                    read: true,
                    append: true,
                },
            )
            .map_err(Self::map_storage_error)?;
        file.write_all(bytes).map_err(|_| EventAppendError::Io)
    }

    /// GC 事件 segment：删除日切文件名超龄（`(today - stem).days >= retention_days`）
    /// 的 `.jsonl` 文件，返回删除数。`retention_days == 0` 表示**禁用 GC**
    /// （从不删除，也不阻断写入）；无法按 `yyyy-mm-dd` 解析的文件名一律跳过。
    pub(crate) fn gc_expired(
        &self,
        now_unix_ms: u64,
        retention_days: u32,
    ) -> Result<usize, EventAppendError> {
        if retention_days == 0 {
            return Ok(0);
        }
        let today = Self::day_of(now_unix_ms)?;
        let today = chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")
            .map_err(|_| EventAppendError::Rejected)?;
        let _guard = self
            .store_lock
            .lock()
            .map_err(|_| EventAppendError::Unavailable)?;
        let dir = self.events_dir()?;
        let mut deleted = 0usize;
        for entry in dir.entries().map_err(Self::map_storage_error)? {
            if entry.file_type() != SafeStorageFileType::RegularFile {
                continue;
            }
            let name = entry.name().as_str();
            let Some(stem) = name.strip_suffix(EVENTS_JSONL_SUFFIX) else {
                continue;
            };
            let Ok(stem_date) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
                continue;
            };
            if (today - stem_date).num_days() >= i64::from(retention_days) {
                dir.remove_file(entry.name())
                    .map_err(Self::map_storage_error)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// 以构造时配置的 `retention_days` 执行 GC 的免参入口（composition /
    /// 周期触发点使用；配置接线落地前仅测试引用，放行 dead_code）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn gc(&self, now_unix_ms: u64) -> Result<usize, EventAppendError> {
        self.gc_expired(now_unix_ms, self.retention_days)
    }

    /// 测试读回：按文件名（日切升序）读取全部事件 segment 并逐行反序列化。
    /// **仅测试构建存在**，NEVER 经 `memory::api` 暴露。
    #[cfg(test)]
    pub(crate) fn read_all_for_test(&self) -> Result<Vec<MemoryEvent>, EventAppendError> {
        let _guard = self
            .store_lock
            .lock()
            .map_err(|_| EventAppendError::Unavailable)?;
        let dir = self.events_dir()?;
        let mut events = Vec::new();
        for entry in dir.entries().map_err(Self::map_storage_error)? {
            if entry.file_type() != SafeStorageFileType::RegularFile
                || !entry.name().as_str().ends_with(EVENTS_JSONL_SUFFIX)
            {
                continue;
            }
            let file = dir
                .open_existing(
                    entry.name(),
                    SafeOpenOptions {
                        read: true,
                        append: false,
                    },
                )
                .map_err(Self::map_storage_error)?;
            for line in BufReader::new(file).lines() {
                let line = line.map_err(|_| EventAppendError::Io)?;
                if line.is_empty() {
                    continue;
                }
                events.push(
                    serde_json::from_str(&line).map_err(|_| EventAppendError::Serialization)?,
                );
            }
        }
        Ok(events)
    }
}

#[async_trait]
impl MemoryEventAppendPort for JsonlSegmentEventStore {
    async fn append(&self, event: &MemoryEvent) -> Result<(), EventAppendError> {
        let bytes = Self::encode_line(event)?;
        let day = Self::day_of(event.ts_unix_ms)?;
        // 阻塞 IO 走 blocking 池（对齐 Audit append 先例），store 克隆共享同一把锁。
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.append_line(&day, &bytes))
            .await
            .map_err(|_| EventAppendError::Unavailable)?
    }
}

#[cfg(test)]
#[path = "event_jsonl_tests.rs"]
mod tests;
