//! Storage adapters 层共享生产常量（#1146 双轨归位）。

pub(crate) const LOCK_FILE: &str = "dataset.lock";
pub(crate) const JOURNAL_FILE: &str = "journal.json";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";
pub(crate) const BLOBS_DIR: &str = "blobs";
pub(crate) const MEMBERS_DIR: &str = "members";
pub(crate) const PRIMARY_DIR: &str = "primary";
pub(crate) const PREVIOUS_DIR: &str = "previous";
pub(crate) const PREVIOUS_NEXT_DIR: &str = "previous.next";
/// 持久损坏标记：一旦无法隔离被篡改的 primary 代即落盘此文件。恢复入口据此持续
/// fail-closed，绝不再打开仍在原位的矛盾数据；清除只经显式 quarantine。
pub(crate) const CORRUPTION_MARKER: &str = "corruption.marker";
#[cfg(any(test, feature = "test-fault-injection"))]
#[cfg(any(test, feature = "test-fault-injection"))]
pub(crate) const FAULT_ABORT_ENV: &str = "AEMEATH_STORAGE_FAULT_ABORT";

#[cfg(test)]
pub(crate) const FAULT_POINT_ENV: &str = "AEMEATH_STORAGE_FAULT_POINT";

#[cfg(any(test, feature = "test-fault-injection"))]
#[cfg(any(test, feature = "test-fault-injection"))]
pub(crate) const DATASET_FAULT_ABORT_ENV: &str = "AEMEATH_STORAGE_DATASET_FAULT_ABORT";

#[cfg(any(test, feature = "test-fault-injection"))]
#[cfg_attr(not(any(test, feature = "test-fault-injection")), allow(dead_code))]
pub(crate) const DATASET_FAULT_POINT_ENV: &str = "AEMEATH_STORAGE_DATASET_FAULT_POINT";
