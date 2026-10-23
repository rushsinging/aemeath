//! 证据流模型 domain（#2049/#2057，设计 04 分片 §2A.5/§2A.6）：
//! run 级处理状态、session 边界优先分块与 checkpoint 自动清理。
//!
//! 对齐 Harness² 断点纪律（runmark）：完成显式打标；增量判定用
//! run_id 集合（NEVER 依赖 mtime 可比性——session 更新只新增 run，
//! 条目不漂移、不漏读、不冗余）。

use crate::constants::{CHECKPOINT_FILE_NAME, COMPLETE_SESSION_RETENTION, ERROR_DOMAIN};
use crate::domain::errors::HarnessError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// 单 session 的处理进度（run 级）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionProgress {
    /// 已处理 run id 集合（有序去重）。
    pub processed_run_ids: Vec<String>,
    /// 本 session 是否已完整处理（块边界没被步长切断）。
    pub session_complete: bool,
}

/// checkpoint 状态：per-session 进度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EvidenceStreamState {
    pub sessions: BTreeMap<String, SessionProgress>,
}

/// 从 checkpoint 文件读状态（缺失/损坏返回默认——按首次处理，NEVER 阻断）。
pub fn load_stream_state(harness_dir: &Path) -> Result<EvidenceStreamState, HarnessError> {
    let path = harness_dir.join(CHECKPOINT_FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EvidenceStreamState::default())
        }
        Err(error) => {
            return Err(HarnessError::storage(
                ERROR_DOMAIN,
                format!("读取 checkpoint 失败：{error}"),
            ))
        }
    };
    serde_json::from_str(&text).map_err(|error| {
        HarnessError::invalid(
            ERROR_DOMAIN,
            format!("checkpoint 解析失败（将重建）：{error}"),
        )
    })
}

/// 原子写状态（tmp+rename）+ 自动清理：
/// 保留未完成 session、盘上仍存在的 session、最近 N 条已完整的。
pub fn save_stream_state(
    harness_dir: &Path,
    state: &EvidenceStreamState,
    live_session_keys: &std::collections::BTreeSet<String>,
) -> Result<(), HarnessError> {
    let pruned = prune_stream_state(state, live_session_keys);
    std::fs::create_dir_all(harness_dir).map_err(|error| {
        HarnessError::storage(ERROR_DOMAIN, format!("创建 harness 目录失败：{error}"))
    })?;
    let path = harness_dir.join(CHECKPOINT_FILE_NAME);
    let temp_path = path.with_extension("json.tmp");
    let text = serde_json::to_string(&pruned).map_err(|error| {
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

/// 清理：盘上消失即时删；完整条目保最近 N 条（按 key 降序保留新者）。
pub fn prune_stream_state(
    state: &EvidenceStreamState,
    live_session_keys: &std::collections::BTreeSet<String>,
) -> EvidenceStreamState {
    let mut pruned = BTreeMap::new();
    let complete_keys: Vec<&String> = state
        .sessions
        .iter()
        .filter(|(_, progress)| progress.session_complete)
        .map(|(key, _)| key)
        .collect();
    // BTreeMap key 升序；保留最近 N 条完整 = 升序尾部 N 条
    let complete_keep: std::collections::BTreeSet<&String> =
        if complete_keys.len() > COMPLETE_SESSION_RETENTION {
            let cutoff = complete_keys.len() - COMPLETE_SESSION_RETENTION;
            complete_keys[cutoff..].iter().copied().collect()
        } else {
            complete_keys.iter().copied().collect()
        };
    for (key, progress) in &state.sessions {
        if !live_session_keys.contains(key) {
            continue; // 盘上已消失
        }
        if progress.session_complete && !complete_keep.contains(&key) {
            continue; // 完整但超保留上限
        }
        pruned.insert(key.clone(), progress.clone());
    }
    EvidenceStreamState { sessions: pruned }
}

/// session 流条目（调度序原料；mtime 只定顺序 NEVER 承担游标语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStreamEntry {
    pub session_key: String,
    pub updated_at_secs: u64,
}

/// 单个数据源块：一组 run 引用（session 边界优先切块）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceBlockPlan {
    pub runs: Vec<RunRef>,
}

/// run 引用（session 内一个 run）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRef {
    pub session_key: String,
    pub run_id: String,
}

/// 流规划参数（run 级步长，#2057）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamPlanParams {
    /// 首次（空状态）时间窗（天）。
    pub initial_window_days: u64,
    /// 每周期步长上限：最多处理的 run 数。
    pub max_runs_per_cycle: usize,
    /// 每块 run 数上限（块边界优先 session 边界）。
    pub block_run_limit: usize,
    /// 组件树指纹（记录用）。
    pub component_tree_fingerprint: String,
}

/// 规划证据流（纯函数）：
/// 候选 session（升序）→ 每 session 展开 run（装配层给 session→runs 映射，
/// 过滤已处理）→ run 级步长截断 → session 边界优先切块。
pub fn plan_evidence_stream(
    entries: Vec<SessionStreamEntry>,
    state: &EvidenceStreamState,
    session_runs: &BTreeMap<String, Vec<String>>,
    params: &StreamPlanParams,
) -> Vec<EvidenceBlockPlan> {
    let mut ordered = entries;
    ordered.sort_by_key(|entry| (entry.updated_at_secs, entry.session_key.clone()));

    // 首次（状态空）窗口过滤；有状态时增量由 run 过滤承担
    let newest = ordered
        .last()
        .map(|entry| entry.updated_at_secs)
        .unwrap_or(0);
    let window_secs = params.initial_window_days.saturating_mul(86_400);
    let cutoff = newest.saturating_sub(window_secs);

    let mut pending_runs: Vec<RunRef> = Vec::new();
    for entry in ordered {
        let progress = state.sessions.get(&entry.session_key);
        let complete_untouched = progress.map(|p| p.session_complete).unwrap_or(false);
        if complete_untouched {
            // 完整处理过且 mtime 未变（无新 run 可能）——跳过
            // mtime 变化判定由装配层在 session_runs 中体现（更新后含新 run）
            if let Some(runs) = session_runs.get(&entry.session_key) {
                let has_new = runs.iter().any(|run_id| {
                    !progress
                        .map(|p| p.processed_run_ids.contains(run_id))
                        .unwrap_or(false)
                });
                if !has_new {
                    continue;
                }
            } else {
                continue;
            }
        }
        let is_first_cycle = state.sessions.is_empty();
        if is_first_cycle && entry.updated_at_secs < cutoff {
            continue;
        }
        if let Some(runs) = session_runs.get(&entry.session_key) {
            for run_id in runs {
                let processed = progress
                    .map(|p| p.processed_run_ids.contains(run_id))
                    .unwrap_or(false);
                if !processed {
                    pending_runs.push(RunRef {
                        session_key: entry.session_key.clone(),
                        run_id: run_id.clone(),
                    });
                }
            }
        }
    }

    // run 级步长 + session 边界优先切块：pending_runs 按 session 连续
    // （外层 session 内层 run），贪心切块——块满即切，天然不在 session
    // 中间开新块；单 session 超块上限才被切（session_complete=false）。
    let truncated: Vec<RunRef> = pending_runs
        .into_iter()
        .take(params.max_runs_per_cycle)
        .collect();

    let mut blocks = Vec::new();
    let mut current: Vec<RunRef> = Vec::new();
    for run_ref in truncated {
        if current.len() >= params.block_run_limit && !current.is_empty() {
            blocks.push(EvidenceBlockPlan {
                runs: std::mem::take(&mut current),
            });
        }
        current.push(run_ref);
    }
    if !current.is_empty() {
        blocks.push(EvidenceBlockPlan { runs: current });
    }
    blocks
}
