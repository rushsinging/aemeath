//! kev 答案映射：概率分布 + KevQuestionPlan → ScoringAnswer（kev `to_answers` 口径）。

use crate::adapters::kev_answer_mapping::{
    build_kev_answer, kev_choice_confidence, kev_score_confidence, AnswerBuildFailure,
};
use crate::adapters::kev_question_plan::{plan_kev_question, KevQuestionPlan};
use crate::domain::{CalibrationLevel, NoulCriteria, ScoringAnswer, ScoringQuestion};

fn choice_plan() -> KevQuestionPlan {
    plan_kev_question(
        &ScoringQuestion::choice(
            "Which memory is most relevant?",
            vec![
                ("0".to_owned(), "First memory.".to_owned()),
                ("1".to_owned(), "Second memory.".to_owned()),
                ("2".to_owned(), "Third memory.".to_owned()),
            ],
        )
        .unwrap(),
    )
}

fn score_plan() -> KevQuestionPlan {
    plan_kev_question(
        &ScoringQuestion::score(
            "How risky is the operation?",
            vec![
                "Reversible.".to_owned(),
                "Destructive.".to_owned(),
                "Catastrophic.".to_owned(),
            ],
        )
        .unwrap(),
    )
}

fn noul_plan() -> KevQuestionPlan {
    let criteria =
        NoulCriteria::new("All requirements are met.", "A requirement is unmet.").unwrap();
    plan_kev_question(&ScoringQuestion::noul("Is the task complete?", Some(criteria)).unwrap())
}

#[test]
fn choice_answer_selects_argmax_key_and_keeps_criteria_order() {
    let plan = choice_plan();
    let answer = build_kev_answer(&plan, &[0.2, 0.7, 0.1]).expect("合法概率构建答案");
    match answer {
        ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence,
            calibration,
        } => {
            assert_eq!(choice, "1", "argmax key 即 choice");
            assert_eq!(
                probabilities,
                vec![
                    ("0".to_owned(), 0.2),
                    ("1".to_owned(), 0.7),
                    ("2".to_owned(), 0.1),
                ],
                "probabilities 按 criteria 提交顺序 zip"
            );
            assert!(
                (confidence - 0.55).abs() < 1e-9,
                "choice_confidence(0.7,K=3) = 0.55，实际 {confidence}"
            );
            assert_eq!(
                calibration,
                CalibrationLevel::Temperature,
                "指针头已含出厂温度缩放"
            );
        }
        other => panic!("答案类型不符：{other:?}"),
    }
}

#[test]
fn noul_answer_reads_second_probability_as_p_true() {
    let plan = noul_plan();
    let answer = build_kev_answer(&plan, &[0.25, 0.75]).expect("合法概率构建答案");
    match answer {
        ScoringAnswer::Noul {
            p_true,
            calibration,
        } => {
            assert!((p_true - 0.75).abs() < 1e-12, "kev Noul p = probs[1]");
            assert_eq!(calibration, CalibrationLevel::Temperature);
        }
        other => panic!("答案类型不符：{other:?}"),
    }
}

#[test]
fn score_answer_computes_weighted_expectation_and_kev_confidence() {
    let plan = score_plan();
    let answer = build_kev_answer(&plan, &[0.5, 0.3, 0.2]).expect("合法概率构建答案");
    match answer {
        ScoringAnswer::Score {
            score,
            probabilities,
            confidence,
            calibration,
        } => {
            assert!(
                (score - 0.7).abs() < 1e-9,
                "score = Σ i·p_i = 0.7，实际 {score}"
            );
            assert_eq!(probabilities, vec![0.5, 0.3, 0.2]);
            // L=3，D = Σ|i-1|/3 = 2/3，mode=0，E|level-mode| = 0.7 → conf = 1 - 0.7/(2/3) < 0 → 0
            assert!(
                (confidence - 0.0).abs() < 1e-9,
                "spread 过大时置信度压到 0，实际 {confidence}"
            );
            assert_eq!(calibration, CalibrationLevel::Temperature);
        }
        other => panic!("答案类型不符：{other:?}"),
    }
}

#[test]
fn choice_confidence_matches_kev_reference_formula() {
    assert!(
        (kev_choice_confidence(&[0.25, 0.25, 0.25, 0.25]) - 0.0).abs() < 1e-9,
        "均匀分布 → 0"
    );
    assert!(
        (kev_choice_confidence(&[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-9,
        "全质量 → 1"
    );
    assert!(
        (kev_choice_confidence(&[0.5, 0.5]) - 0.0).abs() < 1e-9,
        "两选均匀 → 0"
    );
    assert!(
        (kev_choice_confidence(&[1.0]) - 1.0).abs() < 1e-9,
        "单选项 → 1"
    );
}

#[test]
fn score_confidence_matches_kev_reference_formula() {
    assert!(
        (kev_score_confidence(&[0.25, 0.25, 0.25, 0.25]) - 0.0).abs() < 1e-9,
        "均匀分布 → 0"
    );
    assert!(
        (kev_score_confidence(&[1.0, 0.0, 0.0]) - 1.0).abs() < 1e-9,
        "全质量 → 1"
    );
    // mode=0，D = 2/3，E|level-mode| = 0.1 → conf = 1 - 0.1/(2/3) = 0.85
    assert!(
        (kev_score_confidence(&[0.9, 0.1, 0.0]) - 0.85).abs() < 1e-9,
        "集中在单档 → 0.85"
    );
}

#[test]
fn probability_count_mismatch_reports_failure() {
    let plan = choice_plan();
    let error = build_kev_answer(&plan, &[0.5, 0.5]).expect_err("概率数量与选项数不符必须失败");
    match &error {
        AnswerBuildFailure::OptionCountMismatch { expected, found } => {
            assert_eq!(*expected, 3);
            assert_eq!(*found, 2);
        }
        other => panic!("错误类型不符：{other:?}"),
    }
    assert!(
        error.to_string().contains("不符"),
        "错误消息为中文：{}",
        error
    );
}
