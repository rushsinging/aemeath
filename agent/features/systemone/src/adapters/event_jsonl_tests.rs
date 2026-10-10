//! JsonlSegmentScoringEventStore 行为测试：日切 append + legacy 迁移 + 保留 GC。
//!
//! 全部基于 tempfile 真实文件系统断言（设计 §4.1/§4.2 落盘契约）。

use super::*;
use crate::domain::ScoringEvent;

/// 构造指定 RFC3339 时刻的最小事件（其余字段空缺即可满足落盘契约）。
fn event_at(timestamp: &str) -> ScoringEvent {
    ScoringEvent {
        schema_version: ScoringEvent::SCHEMA_VERSION,
        event_id: ScoringEvent::generate_event_id(),
        ts_unix_ms: 0,
        timestamp: timestamp.to_string(),
        scenario: "memory_rerank".to_string(),
        engine_revision: "kev-r3".to_string(),
        prompt_sha256: "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b"
            .to_string(),
        question_count: 0,
        latency_ms: 42,
        outcome: "ok".to_string(),
        unavailable_kind: None,
        state_text: String::new(),
        questions: vec![],
        answers: vec![],
        ranking: None,
        // PR1 关联字段全缺省（序列化为 null，不 skip）。
        correlation_id: None,
        session_id: None,
        run_ordinal: None,
        step_ordinal: None,
        tool_call_id: None,
    }
}

#[test]
fn append_same_day_writes_two_lines_to_one_segment() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = JsonlSegmentScoringEventStore::new(temp.path().join("scoring"), 30);
    let first = event_at("2026-10-10T01:00:00Z");
    let second = event_at("2026-10-10T23:30:00Z");

    store.append(&first).expect("首次 append 应成功");
    store.append(&second).expect("二次 append 应成功");

    let segment_path = temp
        .path()
        .join("scoring")
        .join("events")
        .join("2026-10-10.jsonl");
    let source = std::fs::read_to_string(&segment_path).expect("日切 segment 应存在");
    let lines: Vec<&str> = source.lines().collect();
    assert_eq!(lines.len(), 2, "同日两次 append 应各占一行：{source}");

    let first_round_trip: ScoringEvent =
        serde_json::from_str(lines[0]).expect("第一行应反序列化回 ScoringEvent");
    let second_round_trip: ScoringEvent =
        serde_json::from_str(lines[1]).expect("第二行应反序列化回 ScoringEvent");
    assert_eq!(first_round_trip, first);
    assert_eq!(second_round_trip, second);
}

#[test]
fn append_across_days_writes_two_segment_files() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = JsonlSegmentScoringEventStore::new(temp.path().join("scoring"), 30);

    store
        .append(&event_at("2026-10-09T23:59:00Z"))
        .expect("第一日 append 应成功");
    store
        .append(&event_at("2026-10-10T00:01:00Z"))
        .expect("第二日 append 应成功");

    let events_dir = temp.path().join("scoring").join("events");
    let first_segment = events_dir.join("2026-10-09.jsonl");
    let second_segment = events_dir.join("2026-10-10.jsonl");
    assert!(first_segment.exists(), "跨日事件应写入第一日 segment");
    assert!(second_segment.exists(), "跨日事件应写入第二日 segment");
    assert_eq!(
        std::fs::read_to_string(&first_segment)
            .expect("第一日 segment 可读")
            .lines()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(&second_segment)
            .expect("第二日 segment 可读")
            .lines()
            .count(),
        1
    );
}

#[test]
fn append_invalid_timestamp_returns_err_without_writing() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = JsonlSegmentScoringEventStore::new(temp.path().join("scoring"), 30);

    let error = store
        .append(&event_at("not-a-timestamp"))
        .expect_err("timestamp 解析失败 MUST 返回 Err，NEVER 静默丢弃");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        !temp.path().join("scoring").join("events").exists(),
        "解析失败时不应产生落盘文件"
    );
}

#[test]
fn migrate_copies_legacy_lines_verbatim_then_renames_original() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    std::fs::create_dir_all(&scoring_dir).expect("创建 scoring 目录");
    let legacy_source =
        "{\"timestamp\":\"2026-01-02T03:04:05Z\",\"outcome\":\"ok\"}\nnot json at all\n";
    let legacy_path = scoring_dir.join("audit.jsonl");
    std::fs::write(&legacy_path, legacy_source).expect("预置 legacy 审计文件");

    let store = JsonlSegmentScoringEventStore::new(scoring_dir.clone(), 30);
    store.migrate_legacy_audit().expect("迁移应成功");

    let archive_path = scoring_dir.join("events").join("legacy-audit.jsonl");
    let archive = std::fs::read_to_string(&archive_path).expect("归档应存在");
    let archived_lines: Vec<&str> = archive.lines().collect();
    assert_eq!(
        archived_lines,
        vec![
            "{\"timestamp\":\"2026-01-02T03:04:05Z\",\"outcome\":\"ok\"}",
            "not json at all",
        ],
        "合法行与垃圾行 MUST 原样抄入，NEVER 丢弃"
    );

    assert!(!legacy_path.exists(), "迁移后原文件应已改名");
    let migrated_path = scoring_dir.join("audit.jsonl.migrated");
    assert_eq!(
        std::fs::read_to_string(&migrated_path).expect("migrated 文件应存在"),
        legacy_source,
        "原文件内容应完整保留在 .migrated"
    );

    // 再次调用（无原文件）：Ok 空操作，归档不重复追加。
    store
        .migrate_legacy_audit()
        .expect("原文件不存在时应 Ok 空操作");
    assert_eq!(
        std::fs::read_to_string(&archive_path)
            .expect("归档仍应存在")
            .lines()
            .count(),
        2,
        "重复迁移不应再追加任何行"
    );
}

#[test]
fn migrate_without_legacy_file_is_noop() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    std::fs::create_dir_all(&scoring_dir).expect("创建 scoring 目录");

    let store = JsonlSegmentScoringEventStore::new(scoring_dir.clone(), 30);
    store.migrate_legacy_audit().expect("legacy 不存在时应 Ok");

    assert!(
        !scoring_dir.join("events").exists(),
        "空操作 MUST 不产生任何文件"
    );
    assert!(!scoring_dir.join("audit.jsonl.migrated").exists());
}

#[test]
fn migrate_unreadable_legacy_file_returns_err_and_keeps_original() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    std::fs::create_dir_all(&scoring_dir).expect("创建 scoring 目录");
    // 用同名目录占位模拟「存在但不可读」：NEVER 吞错为空操作。
    std::fs::create_dir_all(scoring_dir.join("audit.jsonl")).expect("占位目录");

    let store = JsonlSegmentScoringEventStore::new(scoring_dir.clone(), 30);
    let migrate_result = store.migrate_legacy_audit();

    assert!(
        migrate_result.is_err(),
        "读失败 MUST 返回 Err，仅 NotFound 才是空操作"
    );
    assert!(
        !scoring_dir.join("audit.jsonl.migrated").exists(),
        "失败后原路径 MUST 保持不动"
    );
}

#[test]
fn retain_segments_deletes_expired_keeps_recent_and_unparsable_names() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let events_dir = scoring_dir.join("events");
    std::fs::create_dir_all(&events_dir).expect("创建 events 目录");
    for file_name in ["2025-01-01.jsonl", "2026-10-10.jsonl", "notadate.jsonl"] {
        std::fs::write(events_dir.join(file_name), "[]\n").expect("预置 segment 文件");
    }

    let store = JsonlSegmentScoringEventStore::new(scoring_dir, 30);
    store.retain_segments("2026-10-10");

    assert!(
        !events_dir.join("2025-01-01.jsonl").exists(),
        "过期 segment MUST 被删除"
    );
    assert!(
        events_dir.join("2026-10-10.jsonl").exists(),
        "保留窗口内的 segment MUST 保留"
    );
    assert!(
        events_dir.join("notadate.jsonl").exists(),
        "文件名日期无法解析的文件 MUST 跳过不删"
    );
}

#[test]
fn retain_segments_disabled_keeps_every_segment() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("scoring");
    let events_dir = scoring_dir.join("events");
    std::fs::create_dir_all(&events_dir).expect("创建 events 目录");
    for file_name in ["2025-01-01.jsonl", "2026-10-10.jsonl", "notadate.jsonl"] {
        std::fs::write(events_dir.join(file_name), "[]\n").expect("预置 segment 文件");
    }

    let store = JsonlSegmentScoringEventStore::new(scoring_dir, 0);
    store.retain_segments("2026-10-10");

    for file_name in ["2025-01-01.jsonl", "2026-10-10.jsonl", "notadate.jsonl"] {
        assert!(
            events_dir.join(file_name).exists(),
            "retention_days=0 时 MUST 全部保留：{file_name}"
        );
    }
}

#[test]
fn append_creates_missing_events_directory() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let scoring_dir = temp.path().join("nested").join("scoring");
    let store = JsonlSegmentScoringEventStore::new(scoring_dir.clone(), 30);

    store
        .append(&event_at("2026-10-10T12:00:00Z"))
        .expect("events 目录不存在时 append 应自动创建");

    assert!(
        scoring_dir.join("events").join("2026-10-10.jsonl").exists(),
        "append MUST create_dir_all 幂等创建 events 目录"
    );
}
