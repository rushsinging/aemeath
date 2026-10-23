//! 证据流模型 domain（#2049，设计 04 分片 §2A.5）：checkpoint 游标、
//! 首次窗口、步长上限与数据源分块。
//!
//! 对齐 Harness² 断点纪律（runmark）：完成显式打标（输出存在≠完成）、
//! 指纹校验复用。分块对象是 session 证据流，NEVER 按组件分块。

use crate::constants::CHECKPOINT_FILE_NAME;
use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 流游标：上次成功处理到的位置（session 序 + 当时的组件树指纹）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceStreamCursor {
    /// 最后成功处理的 session 标识（dataset 目录名，含 .dataset 后缀）。
    pub last_session_key: String,
    /// 游标 session 的修改时间（Unix 秒）——增量判定的全序坐标之一。
    pub last_session_updated_at_secs: u64,
    /// 最后成功块的处理时间戳（Unix 秒）。
    pub completed_at_secs: u64,
    /// 处理时的组件树指纹（外部改动检测：指纹不符当块重跑）。
    pub component_tree_fingerprint: String,
}

/// 从 checkpoint 文件读游标（缺失返回 None——首次周期）。
pub fn load_cursor(harness_dir: &Path) -> Result<Option<EvidenceStreamCursor>, HarnessError> {
    let path = harness_dir.join(CHECKPOINT_FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取 checkpoint 失败：{error}"),
            ))
        }
    };
    serde_json::from_str(&text).map(Some).map_err(|error| {
        HarnessError::invalid(
            ERROR_DOMAIN,
            format!("checkpoint 解析失败（将重建）：{error}"),
        )
    })
}

/// 原子写游标（tmp+rename；只在块成功后调用——输出存在不等于完成）。
pub fn save_cursor(harness_dir: &Path, cursor: &EvidenceStreamCursor) -> Result<(), HarnessError> {
    std::fs::create_dir_all(harness_dir).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("创建 harness 目录失败：{error}"))
    })?;
    let path = harness_dir.join(CHECKPOINT_FILE_NAME);
    let temp_path = harness_path_with_suffix(&path, ".tmp");
    let text = serde_json::to_string(cursor).map_err(|error| {
        HarnessError::invalid(ERROR_DOMAIN, format!("checkpoint 序列化失败：{error}"))
    })?;
    std::fs::write(&temp_path, text).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("写 checkpoint 临时文件失败：{error}"))
    })?;
    std::fs::rename(&temp_path, &path).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("checkpoint 原子替换失败：{error}"))
    })?;
    Ok(())
}

fn harness_path_with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    name.push_str(suffix);
    path.with_file_name(name)
}

/// session 流条目（装配层从 dataset 根枚举：目录名 + mtime）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStreamEntry {
    /// dataset 目录名（含 .dataset 后缀，作稳定 key）。
    pub session_key: String,
    /// 修改时间（Unix 秒）。
    pub updated_at_secs: u64,
}

/// 单个数据源块的处理计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceBlockPlan {
    /// 本块的 session 条目（有序）。
    pub sessions: Vec<SessionStreamEntry>,
}

/// 流规划参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPlanParams {
    /// 首次（无游标）时间窗（天）。
    pub initial_window_days: u64,
    /// 每周期步长上限（session 数）。
    pub max_sessions_per_cycle: usize,
    /// 每块 session 数上限。
    pub block_session_limit: usize,
    /// 当前组件树指纹（游标推进与校验用）。
    pub component_tree_fingerprint: String,
}

/// 规划证据流（纯函数）：
/// 全序（mtime 升序）→ 首次窗口/游标过滤 → 步长截断 → 分块。
pub fn plan_evidence_stream(
    entries: Vec<SessionStreamEntry>,
    cursor: Option<&EvidenceStreamCursor>,
    params: &StreamPlanParams,
) -> Vec<EvidenceBlockPlan> {
    let mut ordered = entries;
    ordered.sort_by_key(|entry| (entry.updated_at_secs, entry.session_key.clone()));

    let eligible: Vec<SessionStreamEntry> = match cursor {
        Some(cursor) => {
            // 增量：(mtime, key) 全序上严格在游标之后的 session
            let cursor_pos = (
                cursor.last_session_updated_at_secs,
                cursor.last_session_key.as_str(),
            );
            ordered
                .into_iter()
                .filter(|entry| (entry.updated_at_secs, entry.session_key.as_str()) > cursor_pos)
                .collect()
        }
        None => {
            // 首次：最近 initial_window_days 天
            let window_secs = params.initial_window_days.saturating_mul(86_400);
            let newest = ordered
                .last()
                .map(|entry| entry.updated_at_secs)
                .unwrap_or(0);
            let cutoff = newest.saturating_sub(window_secs);
            ordered
                .into_iter()
                .filter(|entry| entry.updated_at_secs >= cutoff)
                .collect()
        }
    };

    let truncated: Vec<SessionStreamEntry> = eligible
        .into_iter()
        .take(params.max_sessions_per_cycle)
        .collect();

    truncated
        .chunks(params.block_session_limit.max(1))
        .map(|sessions| EvidenceBlockPlan {
            sessions: sessions.to_vec(),
        })
        .collect()
}
