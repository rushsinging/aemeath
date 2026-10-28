use super::*;

#[test]
fn matrix_report_serializes_model_rows() {
    let report = MatrixReport {
        models: vec![ModelRow {
            model: "A/m1".to_string(),
            cases: 2,
            succeeded: 1,
            failed: 1,
            degraded: 0,
            duration_ms_p50: Some(100),
            duration_ms_p95: Some(100),
            duration_ms_max: Some(100),
            summary_chars_mean: Some(5000.0),
            must_keep_recall_mean: Some(0.4),
        }],
        note: "note".to_string(),
    };
    let json = serde_json::to_string(&report).expect("json");
    assert!(json.contains("must_keep_recall_mean"));
    assert!(json.contains("\"degraded\":0"));
}
