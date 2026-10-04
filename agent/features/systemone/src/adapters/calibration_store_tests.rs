//! CalibrationStore 行为测试：observe 落盘字段完整性 + artifact 加载与降级。

use super::*;
use crate::domain::ScoringQuestion;
use crate::ports::CalibrationPort;

fn sample_observation() -> CalibrationObservation {
    CalibrationObservation {
        question: ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造"),
        probabilities: vec![0.93, 0.07],
        label: "true".to_owned(),
        engine_revision: "kev-0.8b@2026-09-30".to_owned(),
        timestamp: "2026-10-04T00:00:00Z".to_owned(),
    }
}

#[tokio::test]
async fn observe_appends_jsonl_record_with_full_fields() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = CalibrationStore::new(temp.path().to_path_buf());

    let path = temp.path().join("observations.jsonl");
    store.observe(sample_observation()).await;
    store.observe(sample_observation()).await;

    let source = std::fs::read_to_string(&path).expect("observations.jsonl 应存在");
    let lines: Vec<&str> = source.lines().collect();
    assert_eq!(lines.len(), 2, "两条观测应各占一行");

    let record: serde_json::Value = serde_json::from_str(lines[0]).expect("行应为合法 JSON");
    assert_eq!(record["label"], "true");
    assert_eq!(record["engine_revision"], "kev-0.8b@2026-09-30");
    assert_eq!(record["timestamp"], "2026-10-04T00:00:00Z");
    assert_eq!(record["probabilities"], serde_json::json!([0.93, 0.07]));
    assert!(
        record["question"]["Noul"].is_object(),
        "question 应携带题型结构：{record}"
    );
}

#[tokio::test]
async fn observe_creates_missing_directory() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let nested = temp.path().join("nested").join("scoring");
    let store = CalibrationStore::new(nested.clone());

    store.observe(sample_observation()).await;

    assert!(nested.join("observations.jsonl").exists());
}

#[test]
fn current_returns_raw_when_artifact_absent() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = CalibrationStore::new(temp.path().to_path_buf());
    assert_eq!(store.current(), CalibrationLevel::Raw);
    assert!(store.artifact().is_none());
}

#[test]
fn current_returns_temperature_when_artifact_present() {
    let temp = tempfile::TempDir::new().expect("临时目录");
    std::fs::write(
        temp.path().join("calibration.json"),
        r#"{"temperature": 1.35, "fitted_at": "2026-10-03T00:00:00Z", "sample_count": 120}"#,
    )
    .expect("写入 artifact");

    let store = CalibrationStore::new(temp.path().to_path_buf());

    assert_eq!(store.current(), CalibrationLevel::Temperature);
    let artifact = store.artifact().expect("artifact 应加载");
    assert_eq!(artifact.temperature(), 1.35);
}

#[test]
fn invalid_temperature_artifact_falls_back_to_raw() {
    for source in [
        r#"{"temperature": 0.0}"#,
        r#"{"temperature": -1.2}"#,
        r#"{"temperature": "hot"}"#,
        r#"not json at all"#,
        r#"{"other": 1}"#,
    ] {
        let temp = tempfile::TempDir::new().expect("临时目录");
        std::fs::write(temp.path().join("calibration.json"), source).expect("写入 artifact");
        let store = CalibrationStore::new(temp.path().to_path_buf());
        assert_eq!(
            store.current(),
            CalibrationLevel::Raw,
            "非法 artifact 应回退 Raw：{source}"
        );
        assert!(store.artifact().is_none());
    }
}
