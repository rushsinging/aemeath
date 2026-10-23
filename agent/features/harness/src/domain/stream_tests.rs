//! 证据流模型契约测试（#2049）：窗口/游标/步长/分块 + checkpoint 持久化。

use crate::domain::stream::{
    load_cursor, plan_evidence_stream, save_cursor, EvidenceStreamCursor, SessionStreamEntry,
    StreamPlanParams,
};

fn entry(key: &str, at: u64) -> SessionStreamEntry {
    SessionStreamEntry {
        session_key: key.to_string(),
        updated_at_secs: at,
    }
}

fn params() -> StreamPlanParams {
    StreamPlanParams {
        initial_window_days: 30,
        max_sessions_per_cycle: 5,
        block_session_limit: 2,
        component_tree_fingerprint: "fp-1".to_string(),
    }
}

fn cursor(key: &str, at: u64) -> EvidenceStreamCursor {
    EvidenceStreamCursor {
        last_session_key: key.to_string(),
        last_session_updated_at_secs: at,
        completed_at_secs: at + 1,
        component_tree_fingerprint: "fp-1".to_string(),
    }
}

/// 首次：最近 30 天窗口过滤 + mtime 升序。
#[test]
fn first_cycle_uses_window_and_ascending_order() {
    let entries = vec![
        entry("new.dataset", 1_000_000_000),
        entry("old.dataset", 1_000_000_000 - 40 * 86_400),
        entry("mid.dataset", 1_000_000_000 - 10 * 86_400),
    ];
    let blocks = plan_evidence_stream(entries, None, &params());
    let keys: Vec<&str> = blocks
        .iter()
        .flat_map(|block| block.sessions.iter().map(|e| e.session_key.as_str()))
        .collect();
    assert_eq!(keys, vec!["mid.dataset", "new.dataset"], "窗口外排除、升序");
}

/// 增量：只取游标 (mtime, key) 全序之后的。
#[test]
fn cursor_filters_to_strictly_after() {
    let entries = vec![
        entry("a.dataset", 100),
        entry("b.dataset", 100),
        entry("c.dataset", 200),
    ];
    let blocks = plan_evidence_stream(entries, Some(&cursor("b.dataset", 100)), &params());
    let keys: Vec<&str> = blocks
        .iter()
        .flat_map(|block| block.sessions.iter().map(|e| e.session_key.as_str()))
        .collect();
    assert_eq!(keys, vec!["c.dataset"]);
}

/// 步长截断：超出 max_sessions_per_cycle 留待下周期。
#[test]
fn step_cap_truncates_leaving_remainder() {
    let entries: Vec<_> = (0..8)
        .map(|i| entry(&format!("s{i}.dataset"), 100 + i))
        .collect();
    let blocks = plan_evidence_stream(entries, Some(&cursor("s-1.dataset", 0)), &params());
    let total: usize = blocks.iter().map(|block| block.sessions.len()).sum();
    assert_eq!(total, 5, "步长上限 5");
}

/// 分块：block_session_limit 切块。
#[test]
fn blocks_chunked_by_session_limit() {
    let entries: Vec<_> = (0..5)
        .map(|i| entry(&format!("s{i}.dataset"), 100 + i))
        .collect();
    let blocks = plan_evidence_stream(entries, Some(&cursor("x.dataset", 0)), &params());
    assert_eq!(blocks.len(), 3, "5 session / 每块 2 → 3 块");
    assert_eq!(blocks[0].sessions.len(), 2);
    assert_eq!(blocks[2].sessions.len(), 1);
}

/// checkpoint 往返：save → load 一致；缺失返回 None。
#[test]
fn checkpoint_roundtrip_and_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_cursor(dir.path()).unwrap().is_none());

    let saved = cursor("k.dataset", 42);
    save_cursor(dir.path(), &saved).unwrap();
    let loaded = load_cursor(dir.path()).unwrap().expect("游标必须存在");
    assert_eq!(loaded, saved);
    assert!(
        !dir.path().join("checkpoint.json.tmp").exists(),
        "临时文件不残留"
    );
}
