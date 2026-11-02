use super::*;

fn line(ts: &str, session: &str, msg: &str) -> String {
    serde_json::json!({ "ts": ts, "session": session, "msg": msg }).to_string()
}

#[test]
fn legacy_log_pairs_start_and_done_and_infers_duration() {
    let lines = vec![
        line(
            "2026-10-01T01:43:55.000000+08:00",
            "s1",
            "[compact] 开始：messages=313 early=290 early_tokens=215623 previous_summary=0 chars mode=llm",
        ),
        line(
            "2026-10-01T01:43:56.000000+08:00",
            "s1",
            "[compact] map-reduce：26 chunks，3136 messages，1004355 tokens，单块目标 40000，并发上限 5",
        ),
        line(
            "2026-10-01T01:44:20.000000+08:00",
            "s1",
            "[compact] map 完成：total_chunks=26 successful_chunks=24 degraded_chunks=2",
        ),
        line(
            "2026-10-01T01:44:21.000000+08:00",
            "s1",
            "[compact] 完成：summary=48514 chars recent_messages=42",
        ),
    ]
    .into_iter();
    let (records, unmatched) = parse_log(lines, None);
    assert_eq!(unmatched, 0);
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.early_tokens, 215623);
    assert_eq!(record.chunks, Some(26));
    assert_eq!(record.degraded_chunks, Some(2));
    assert_eq!(record.duration_ms(), Some(26_000));
    assert!(record.model.is_none());
    assert!(record.quality.is_none());
}

#[test]
fn new_log_reads_model_quality_and_elapsed() {
    let lines = vec![
        line(
            "2026-10-28T14:01:32.000000+08:00",
            "s2",
            "[compact] 开始：messages=3229 early=3136 early_tokens=1004355 previous_summary=47988 chars mode=llm model=DeepSeek/deepseek-v4.1-flash compact_window=Some(1048576) max_output=20000",
        ),
        line(
            "2026-10-28T14:01:32.100000+08:00",
            "s2",
            "[compact] map-reduce：26 chunks，3136 messages，1004355 tokens，单块目标 40000，并发上限 5，波次 6 model=DeepSeek/deepseek-v4.1-flash",
        ),
        line(
            "2026-10-28T14:08:16.000000+08:00",
            "s2",
            "[compact] map 完成：total_chunks=26 successful_chunks=26 degraded_chunks=0 waves=6 chunk_ms_p50=77000 chunk_ms_p95=90000 chunk_ms_max=120000 elapsed_ms=403800 model=DeepSeek/deepseek-v4.1-flash",
        ),
        line(
            "2026-10-28T14:08:16.200000+08:00",
            "s2",
            "[compact] reduce 合并完成：129017 chars（预算 52428 tokens）facts=128 elapsed_ms=900",
        ),
        line(
            "2026-10-28T14:08:16.300000+08:00",
            "s2",
            "[compact] 完成：summary=129017 chars recent_messages=93 quality=Llm model=DeepSeek/deepseek-v4.1-flash elapsed_ms=404400",
        ),
    ]
    .into_iter();
    let (records, _) = parse_log(lines, None);
    let record = &records[0];
    assert_eq!(
        record.model.as_deref(),
        Some("DeepSeek/deepseek-v4.1-flash")
    );
    assert_eq!(record.quality.as_deref(), Some("Llm"));
    assert_eq!(record.elapsed_ms, Some(404_400));
    assert_eq!(record.duration_ms(), Some(404_400));
    assert_eq!(record.waves, Some(6));
    assert_eq!(record.chunk_ms_p50, Some(77_000));
    assert_eq!(record.map_ms, Some(403_800));
    assert_eq!(record.reduce_ms, Some(900));
}

#[test]
fn since_filter_excludes_older_events() {
    let lines = vec![
        line(
            "2026-09-30T23:00:00.000000+08:00",
            "old",
            "[compact] 开始：messages=10 early=9 early_tokens=100 previous_summary=0 chars mode=llm",
        ),
        line(
            "2026-09-30T23:00:10.000000+08:00",
            "old",
            "[compact] 完成：summary=10 chars recent_messages=2 elapsed_ms=10000",
        ),
        line(
            "2026-10-01T00:00:00.000000+08:00",
            "new",
            "[compact] 开始：messages=10 early=9 early_tokens=200 previous_summary=0 chars mode=llm",
        ),
        line(
            "2026-10-01T00:00:05.000000+08:00",
            "new",
            "[compact] 完成：summary=10 chars recent_messages=2 elapsed_ms=5000",
        ),
    ]
    .into_iter();
    let (records, _) = parse_log(lines, Some("2026-10-01"));
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].early_tokens, 200);
}

#[test]
fn unmatched_sessions_are_reported_not_counted() {
    let lines = vec![line(
        "2026-10-01T00:00:00.000000+08:00",
        "s1",
        "[compact] 开始：messages=10 early=9 early_tokens=100 previous_summary=0 chars mode=llm",
    )]
    .into_iter();
    let (records, unmatched) = parse_log(lines, None);
    assert!(records.is_empty());
    assert_eq!(unmatched, 1);
}

#[test]
fn percentile_handles_empty_single_and_boundaries() {
    assert_eq!(percentile(&[], 0.5), None);
    assert_eq!(percentile(&[42], 0.95), Some(42));
    let values = [10, 20, 30, 40, 50];
    assert_eq!(percentile(&values, 0.0), Some(10));
    assert_eq!(percentile(&values, 0.5), Some(30));
    assert_eq!(percentile(&values, 1.0), Some(50));
}

#[test]
fn summarize_buckets_unknown_model_and_quality_for_legacy_records() {
    let records = vec![
        CompactionRecord {
            started_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+08:00").ok(),
            finished_at: chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:20+08:00").ok(),
            early_tokens: 1000,
            chunks: Some(6),
            degraded_chunks: Some(1),
            ..Default::default()
        },
        CompactionRecord {
            started_at: chrono::DateTime::parse_from_rfc3339("2026-10-02T00:00:00+08:00").ok(),
            finished_at: chrono::DateTime::parse_from_rfc3339("2026-10-02T00:00:10+08:00").ok(),
            early_tokens: 2000,
            model: Some("M/m1".to_string()),
            quality: Some("Llm".to_string()),
            ..Default::default()
        },
    ];
    let report = summarize(&records);
    assert_eq!(report.total_compactions, 2);
    assert_eq!(report.per_day.get("2026-10-01"), Some(&1));
    assert_eq!(report.duration_ms.max, Some(20_000));
    assert_eq!(report.by_model.len(), 2);
    assert_eq!(
        report.by_model.get("M/m1").map(|stats| stats.count),
        Some(1)
    );
    assert_eq!(report.quality.get("unknown"), Some(&1));
    assert_eq!(report.chunk_shape.compactions_with_map, 1);
    assert_eq!(report.degraded_chunks_total, 1);
}
