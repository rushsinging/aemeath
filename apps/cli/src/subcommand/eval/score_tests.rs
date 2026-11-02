use super::*;

fn case(id: &str, must_keep: &[&str], estimated_tokens: u64) -> CaseInput {
    CaseInput {
        case_id: id.to_string(),
        message_count: 100,
        estimated_tokens,
        must_keep: must_keep.iter().map(|item| item.to_string()).collect(),
        structure_health: None,
    }
}

#[test]
fn empty_gold_is_not_zero_score() {
    let (recall, missed) = must_keep_recall(&[], "anything");
    assert!(recall.is_none());
    assert!(missed.is_empty());
}

#[test]
fn recall_reports_missing_items() {
    let gold = vec!["mem_abc123".to_string(), "a.rs".to_string()];
    let (recall, missed) = must_keep_recall(&gold, "这里只有 mem_abc123");
    assert_eq!(recall, Some(0.5));
    assert_eq!(missed, vec!["a.rs"]);
}

#[test]
fn compression_ratio_of_zero_input_is_undefined() {
    assert!(compression_ratio(0, 100).is_none());
    assert_eq!(compression_ratio(1000, 0), Some(0.0));
}

#[test]
fn missing_summary_is_skipped_not_scored() {
    let result = evaluate_case(&case("c1", &["mem_abc123"], 1000), None);
    assert!(result.recall.is_none());
    assert_eq!(result.skipped_reason.as_deref(), Some("no_summary"));
}

#[test]
fn scored_case_keeps_recall_and_ratio() {
    let result = evaluate_case(&case("c1", &["mem_abc123"], 1000), Some("mem_abc123"));
    assert_eq!(result.recall, Some(1.0));
    assert!(result.compression_ratio.is_some());
    assert_eq!(result.summary_chars, Some(10));
}

#[test]
fn skipped_cases_do_not_pollute_denominator() {
    let mut health_case = case("c1", &["mem_abc123"], 1000);
    health_case.structure_health = Some(StructureInput {
        quality: Some("Llm".to_string()),
        degraded_chunks: 0,
        repair_attempts: 1,
    });
    let results = vec![
        evaluate_case(&health_case, Some("mem_abc123")),
        evaluate_case(&case("c2", &["mem_xyz"], 1000), None),
    ];
    let report = summarize(&results);
    assert_eq!(report.total, 2);
    assert_eq!(report.evaluated, 1);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.must_keep_recall_mean, Some(1.0));
    assert_eq!(report.repair_attempts_total, 1);
    assert_eq!(report.quality.get("Llm"), Some(&1));
}

#[test]
fn degraded_chunks_mark_case_as_degraded() {
    let mut input = case("c1", &["mem_abc123"], 1000);
    input.structure_health = Some(StructureInput {
        quality: Some("PartialMapFallback".to_string()),
        degraded_chunks: 2,
        repair_attempts: 0,
    });
    let report = summarize(&[evaluate_case(&input, Some("mem_abc123"))]);
    assert_eq!(report.degraded_cases, 1);
}
