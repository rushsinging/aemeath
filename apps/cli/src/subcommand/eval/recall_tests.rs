use super::*;

fn user(text: &str) -> serde_json::Value {
    serde_json::json!({ "role": "user", "content": [{ "type": "text", "text": text }] })
}

fn session(messages: Vec<serde_json::Value>, compacted: usize) -> SessionSnapshot {
    SessionSnapshot {
        session_id: "s1".to_string(),
        compact_start: Some(("run_b".to_string(), "stp_b".to_string())),
        compacted_message_count: compacted,
        messages,
        ..Default::default()
    }
}

#[test]
fn recall_counts_identifiers_kept_in_summary() {
    let session = session(
        vec![
            user("改动 agent/features/auth.rs 与 run_abc123，注意边界"),
            user("tail 消息"),
        ],
        1,
    );
    let recall = evaluate(&session, "摘要提到 agent/features/auth.rs", 200);
    assert_eq!(recall.must_keep_total, 2);
    assert_eq!(recall.recall, Some(0.5));
    assert_eq!(recall.missed, vec!["run_abc123"]);
    assert_eq!(recall.early_messages, 1);
    assert_eq!(recall.tail_messages, 1);
}

#[test]
fn compression_ratio_compares_summary_to_early_tokens() {
    let session = session(vec![user("中文内容很多很多很多很多很多很多很多很多")], 1);
    let recall = evaluate(&session, "短摘要", 200);
    let ratio = recall.compression_ratio.expect("ratio");
    assert!(ratio > 0.0 && ratio < 1.0, "ratio={ratio}");
}

#[test]
fn empty_early_history_yields_no_must_keep_and_zero_ratio_guard() {
    let session = session(vec![user("只在 tail 里")], 0);
    let recall = evaluate(&session, "摘要", 200);
    assert_eq!(recall.must_keep_total, 0);
    assert!(recall.recall.is_none());
    assert!(recall.compression_ratio.is_none());
}

#[test]
fn summary_aggregates_means_and_percentiles() {
    let items = vec![
        SessionRecall {
            recall: Some(1.0),
            compression_ratio: Some(0.2),
            early_tokens: 100,
            ..Default::default()
        },
        SessionRecall {
            recall: Some(0.5),
            compression_ratio: Some(0.4),
            early_tokens: 300,
            ..Default::default()
        },
        SessionRecall {
            recall: None,
            compression_ratio: None,
            ..Default::default()
        },
    ];
    let report = summarize(&items, 5);
    assert_eq!(report.sessions_considered, 5);
    assert_eq!(report.sessions_with_compaction, 3);
    assert_eq!(report.must_keep_recall_mean, Some(0.75));
    assert!((report.compression_ratio_mean.expect("ratio mean") - 0.3).abs() < 1e-9);
    assert!(report.must_keep_recall_p50.is_some());
    assert_eq!(report.early_tokens_p50, Some(100));
}
