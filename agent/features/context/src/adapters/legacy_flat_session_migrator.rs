//! 存量平铺 session 的后台一次性迁移。
//!
//! 历史布局把全部项目的 session 平铺在 blob 的 session namespace 根下
//! （当前主形态为裸 `<id>` 文件名，更早版本为 `<id>.json`），列表查询被迫
//! 全量加载才能判定归属（实测 363 个文件 / 4.1GB：78 秒阻塞 + 数百 MB
//! 峰值内存）。本迁移器把平铺 session 搬入 `<project-dir>/<id>` 布局：
//!
//! - 逐个加载（内存峰值 = 单个 session，绝不全量驻留）；
//! - 加载产物经 `LegacySessionDecoder` 的 workspace ACL 升级，project
//!   identity 可靠后决定目标目录；
//! - 写入新位置（原子写协议）成功后删除旧文件；
//! - 幂等：删除失败或部分失败时下次重跑自愈。

use std::sync::Arc;

use storage::{AtomicBlobPort, StorageNamespace};

use crate::adapters::{AtomicBlobSessionStore, LegacySessionDecoder};
use crate::application::SessionPersistenceService;
use crate::domain::session::session_project_dir;

/// 迁移结果摘要。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FlatSessionMigrationReport {
    /// 成功搬入 project 目录的 session 数。
    pub migrated: usize,
    /// 跳过（无法解码 / 无归属 identity / 写入失败）的数量。
    pub skipped: usize,
}

/// 扫描平铺 `<id>.json` session 并迁移到 project 目录段布局。
/// 串行逐个处理；单个失败只计入 skipped，不中断整体迁移。
pub async fn migrate_flat_sessions_to_project_dirs(
    blob: Arc<dyn AtomicBlobPort>,
) -> FlatSessionMigrationReport {
    let entries = match blob.list_primary(StorageNamespace::Session).await {
        Ok(entries) => entries,
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "flat_session_migration list_failed error={error}"
            );
            return FlatSessionMigrationReport::default();
        }
    };
    let mut report = FlatSessionMigrationReport::default();
    for entry in entries {
        let segments = entry.key().segments();
        if segments.len() != 1 {
            continue;
        }
        let flat_segment = segments[0].as_str();
        // 兼容两种历史文件名：裸 `<id>`（当前主形态）与 `<id>.json`（更早版本）。
        let session_id = flat_segment.strip_suffix(".json").unwrap_or(flat_segment);
        if migrate_one(&blob, session_id, flat_segment).await {
            report.migrated += 1;
        } else {
            report.skipped += 1;
        }
    }
    if report.migrated > 0 || report.skipped > 0 {
        log::info!(
            target: crate::LOG_TARGET,
            "flat_session_migration done migrated={} skipped={}",
            report.migrated,
            report.skipped
        );
    }
    report
}

/// 迁移单个平铺 session：`<id>.json` → `<project-dir>/<id>`。
async fn migrate_one(blob: &Arc<dyn AtomicBlobPort>, session_id: &str, flat_segment: &str) -> bool {
    let flat_store = match AtomicBlobSessionStore::from_key_segments(
        Arc::clone(blob),
        vec![flat_segment.to_string()],
    ) {
        Ok(store) => store,
        Err(_) => return false,
    };
    let session =
        match SessionPersistenceService::new(Arc::new(flat_store), Arc::new(LegacySessionDecoder))
            .load()
            .await
        {
            Ok(session) => session,
            Err(_) => return false,
        };
    let Some(project_dir) = session_project_dir(&session) else {
        return false;
    };
    let scoped_store =
        match AtomicBlobSessionStore::new_scoped(Arc::clone(blob), &project_dir, session_id) {
            Ok(store) => store,
            Err(_) => return false,
        };
    if SessionPersistenceService::new(Arc::new(scoped_store), Arc::new(LegacySessionDecoder))
        .save(&session)
        .await
        .is_err()
    {
        return false;
    }
    // 新位置写入成功后清理平铺旧文件；删除失败由下次迁移自愈。
    match AtomicBlobSessionStore::from_key_segments(
        Arc::clone(blob),
        vec![flat_segment.to_string()],
    ) {
        Ok(flat_store) => flat_store.delete_all().await.is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "legacy_flat_session_migrator_tests.rs"]
mod tests;
