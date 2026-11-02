//! CalibratedScoringAdapter 行为测试：温度重缩放 + argmax 克制 + 透传语义。

use super::*;
use crate::adapters::calibration_store::CalibrationStore;
use crate::domain::{CalibrationLevel, ScoringUnavailable, UnavailableKind};

/// 固定应答的评分桩。
struct StubScoringPort {
    outcome: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
}

#[async_trait]
impl ScoringPort for StubScoringPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        self.outcome.clone()
    }
}

fn store_with_temperature(temperature: f64) -> (tempfile::TempDir, CalibrationStore) {
    let temp = tempfile::TempDir::new().expect("临时目录");
    std::fs::write(
        temp.path().join("calibration.json"),
        format!(r#"{{"temperature": {temperature}}}"#),
    )
    .expect("写入 artifact");
    let store = CalibrationStore::new(temp.path().to_path_buf());
    (temp, store)
}

fn store_without_artifact() -> (tempfile::TempDir, CalibrationStore) {
    let temp = tempfile::TempDir::new().expect("临时目录");
    let store = CalibrationStore::new(temp.path().to_path_buf());
    (temp, store)
}

fn sample_state() -> ScoringState {
    ScoringState::new("任意评分上下文。").expect("state 构造")
}

fn sample_questions() -> Vec<ScoringQuestion> {
    vec![ScoringQuestion::noul("命题是否为真？", None).expect("question 构造")]
}

#[tokio::test]
async fn raw_level_passes_answers_through_unchanged() {
    let original = ScoringAnswer::noul(0.9, CalibrationLevel::Raw).expect("答案构造");
    let (_temp, store) = store_without_artifact();
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Ok(vec![original.clone()]),
        }),
        &store,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分应成功");

    assert_eq!(answers, vec![original], "Raw 级别必须原样透传");
}

#[tokio::test]
async fn temperature_softens_noul_p_true_and_marks_level() {
    let (_temp, store) = store_with_temperature(2.0);
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Ok(vec![
                ScoringAnswer::noul(0.9, CalibrationLevel::Raw).expect("答案构造")
            ]),
        }),
        &store,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分应成功");

    match &answers[0] {
        ScoringAnswer::Noul {
            p_true,
            calibration,
        } => {
            assert!(*p_true < 0.9, "T=2 软化应降低自信概率，实际 {p_true}");
            assert!(*p_true > 0.5, "软化不得越过 0.5 翻转判定，实际 {p_true}");
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("期望 Noul，实际 {other:?}"),
    }
}

#[tokio::test]
async fn temperature_rescale_keeps_choice_argmax_unchanged() {
    let (_temp, store) = store_with_temperature(3.0);
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Ok(vec![ScoringAnswer::choice(
                "b",
                vec![
                    ("a".to_owned(), 0.2),
                    ("b".to_owned(), 0.7),
                    ("c".to_owned(), 0.1),
                ],
                0.7,
                CalibrationLevel::Raw,
            )
            .expect("答案构造")]),
        }),
        &store,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分应成功");

    match &answers[0] {
        ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence,
            calibration,
        } => {
            assert_eq!(choice, "b", "校准 NEVER 改变胜负");
            let top = probabilities
                .iter()
                .max_by(|(_, left), (_, right)| left.total_cmp(right))
                .expect("非空");
            assert_eq!(top.0, "b", "argmax 必须不变");
            assert!(top.1 < 0.7, "T=3 软化应降低头部概率，实际 {}", top.1);
            assert_eq!(*confidence, top.1, "confidence 应取重缩放后最高概率");
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[tokio::test]
async fn temperature_rescale_recomputes_score_expectation() {
    let (_temp, store) = store_with_temperature(0.5);
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Ok(vec![ScoringAnswer::score(
                0.55,
                vec![0.2, 0.5, 0.3],
                0.5,
                CalibrationLevel::Raw,
            )
            .expect("答案构造")]),
        }),
        &store,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分应成功");

    match &answers[0] {
        ScoringAnswer::Score {
            score,
            probabilities,
            calibration,
            ..
        } => {
            let expected: f64 = probabilities
                .iter()
                .enumerate()
                .map(|(index, p)| p * index as f64)
                .sum();
            assert!(
                (*score - expected).abs() < 1e-9,
                "score 应为重缩放后分档期望值（Σ p_i·i）：{score} vs {expected}"
            );
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("期望 Score，实际 {other:?}"),
    }
}

#[tokio::test]
async fn inner_unavailable_propagates_without_rescale() {
    let (_temp, store) = store_with_temperature(2.0);
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Err(ScoringUnavailable::new(
                UnavailableKind::Connect,
                "服务未启动",
            )),
        }),
        &store,
    );

    let outcome = adapter.answer(&sample_state(), &sample_questions()).await;

    let error = outcome.expect_err("内层不可用必须透传");
    assert_eq!(error.kind(), UnavailableKind::Connect);
}

#[tokio::test]
async fn temperature_one_keeps_probabilities_but_marks_level() {
    let (_temp, store) = store_with_temperature(1.0);
    let adapter = CalibratedScoringAdapter::new(
        Arc::new(StubScoringPort {
            outcome: Ok(vec![
                ScoringAnswer::noul(0.8, CalibrationLevel::Raw).expect("答案构造")
            ]),
        }),
        &store,
    );

    let answers = adapter
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("评分应成功");

    match &answers[0] {
        ScoringAnswer::Noul {
            p_true,
            calibration,
        } => {
            assert!(
                (*p_true - 0.8).abs() < 1e-9,
                "T=1 应保持概率，实际 {p_true}"
            );
            assert_eq!(*calibration, CalibrationLevel::Temperature);
        }
        other => panic!("期望 Noul，实际 {other:?}"),
    }
}
