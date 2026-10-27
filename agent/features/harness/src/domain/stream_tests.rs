//! 证据流模型契约测试（#2049/#2057）：run 级状态、步长、session 边界
//! 优先分块、增量不漂移与自动清理。

use crate::domain::stream::{
    load_stream_state, plan_evidence_stream, prune_stream_state, save_stream_state,
    EvidenceStreamState, SessionProgress, SessionStreamEntry, StreamPlanParams,
};
use std::collections::BTreeMap;

fn entry(key: &str, at: u64) -> SessionStreamEntry {
    SessionStreamEntry {
        session_key: key.to_string(),
        updated_at_secs: at,
    }
}

fn params() -> StreamPlanParams {
    StreamPlanParams {
        initial_window_days: 30,
        max_runs_per_cycle: 5,
        block_run_limit: 2,
        component_tree_fingerprint: "fp".to_string(),
    }
}

fn state_with(session: &str, runs: &[&str], complete: bool) -> EvidenceStreamState {
    let mut state = EvidenceStreamState::default();
    state.sessions.insert(
        session.to_string(),
        SessionProgress {
            processed_run_ids: runs.iter().map(|r| r.to_string()).collect(),
            session_complete: complete,
        },
    );
    state
}

fn runs_of(blocks: &[crate::domain::stream::EvidenceBlockPlan]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(|block| block.runs.iter().map(|r| r.run_id.clone()))
        .collect()
}

/// 首次：窗口过滤 + session 升序展开全部 run。
#[test]
fn first_cycle_expands_all_runs_within_window() {
    let entries = vec![
        entry("s2.dataset", 1_000_000_000),
        entry("old.dataset", 1_000_000_000 - 40 * 86_400),
        entry("s1.dataset", 1_000_000_000 - 100),
    ];
    let session_runs = BTreeMap::from([
        (
            "s1.dataset".to_string(),
            vec!["r1".to_string(), "r2".to_string()],
        ),
        ("s2.dataset".to_string(), vec!["r9".to_string()]),
        ("old.dataset".to_string(), vec!["r0".to_string()]),
    ]);
    let blocks = plan_evidence_stream(entries, &Default::default(), &session_runs, &params());
    let all = runs_of(&blocks);
    assert_eq!(all, vec!["r1", "r2", "r9"], "窗口外排除、session 升序展开");
}

/// 增量：session 新增 run 只处理新 run（更新不漂移不漏读）。
#[test]
fn updated_session_processes_only_new_runs() {
    let entries = vec![entry("s1.dataset", 200)];
    let session_runs = BTreeMap::from([(
        "s1.dataset".to_string(),
        vec!["r1", "r2", "r3-new"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
    )]);
    // r1/r2 已处理且完整，但 mtime 变了（有 r3-new）→ 只处理新 run
    let state = state_with("s1.dataset", &["r1", "r2"], true);
    let blocks = plan_evidence_stream(entries, &state, &session_runs, &params());
    assert_eq!(runs_of(&blocks), vec!["r3-new"], "旧 run NEVER 重读");
}

/// 完整且无新 run：跳过（不重扫）。
#[test]
fn complete_untouched_session_skipped() {
    let entries = vec![entry("s1.dataset", 100)];
    let session_runs = BTreeMap::from([(
        "s1.dataset".to_string(),
        vec!["r1".to_string(), "r2".to_string()],
    )]);
    let state = state_with("s1.dataset", &["r1", "r2"], true);
    let blocks = plan_evidence_stream(entries, &state, &session_runs, &params());
    assert!(blocks.is_empty());
}

/// run 级步长：超出截断留待下周期。
#[test]
fn run_level_step_cap_truncates() {
    let entries = vec![entry("s1.dataset", 100)];
    let session_runs = BTreeMap::from([(
        "s1.dataset".to_string(),
        (0..8).map(|i| format!("r{i}")).collect::<Vec<_>>(),
    )]);
    let blocks = plan_evidence_stream(entries, &Default::default(), &session_runs, &params());
    assert_eq!(runs_of(&blocks).len(), 5, "max_runs_per_cycle=5");
}

/// session 边界优先：多 session 连续 run 不在中间开新块。
#[test]
fn blocks_prefer_session_boundaries() {
    let entries = vec![entry("s1.dataset", 100), entry("s2.dataset", 200)];
    let session_runs = BTreeMap::from([
        (
            "s1.dataset".to_string(),
            vec!["a1", "a2"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
        (
            "s2.dataset".to_string(),
            vec!["b1", "b2"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
    ]);
    let blocks = plan_evidence_stream(entries, &Default::default(), &session_runs, &params());
    // block_run_limit=2 → 两块，各含单一 session 的两个 run
    assert_eq!(blocks.len(), 2);
    assert!(blocks[0].runs.iter().all(|r| r.session_key == "s1.dataset"));
    assert!(blocks[1].runs.iter().all(|r| r.session_key == "s2.dataset"));
}

/// 单 session 超块上限：切块（session_complete=false 语义由装配层记录）。
#[test]
fn oversized_session_split_across_blocks() {
    let entries = vec![entry("s1.dataset", 100)];
    let session_runs = BTreeMap::from([(
        "s1.dataset".to_string(),
        (0..5).map(|i| format!("r{i}")).collect::<Vec<_>>(),
    )]);
    let blocks = plan_evidence_stream(entries, &Default::default(), &session_runs, &params());
    assert_eq!(blocks.len(), 3, "5 run / 每块 2");
    assert_eq!(blocks[2].runs.len(), 1);
}

/// checkpoint 往返 + 清理：盘上消失删除、完整超限裁剪、未完成保留。
#[test]
fn state_roundtrip_and_auto_prune() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_stream_state(dir.path()).unwrap(), Default::default());

    let mut state = EvidenceStreamState::default();
    for index in 0..60 {
        state.sessions.insert(
            format!("done-{index}.dataset"),
            SessionProgress {
                processed_run_ids: vec!["r".to_string()],
                session_complete: true,
            },
        );
    }
    state.sessions.insert(
        "active.dataset".to_string(),
        SessionProgress {
            processed_run_ids: vec!["r1".to_string()],
            session_complete: false,
        },
    );
    let live: std::collections::BTreeSet<String> = state
        .sessions
        .keys()
        .cloned()
        .chain([("vanished.dataset".to_string())])
        .collect();
    save_stream_state(dir.path(), &state, &live).unwrap();

    let loaded = load_stream_state(dir.path()).unwrap();
    assert_eq!(loaded.sessions.len(), 51, "50 完整 + 1 未完成");
    assert!(
        loaded.sessions.contains_key("active.dataset"),
        "未完成 NEVER 清"
    );
    assert!(
        !loaded.sessions.contains_key("done-0.dataset"),
        "最旧完整被裁"
    );
    assert!(
        loaded.sessions.contains_key("done-59.dataset"),
        "最新完整保留"
    );

    // 纯 prune：盘上消失即时删
    let mut with_vanished = EvidenceStreamState::default();
    with_vanished.sessions.insert(
        "gone.dataset".to_string(),
        SessionProgress {
            processed_run_ids: vec![],
            session_complete: false,
        },
    );
    let pruned = prune_stream_state(&with_vanished, &Default::default());
    assert!(pruned.sessions.is_empty());
}
