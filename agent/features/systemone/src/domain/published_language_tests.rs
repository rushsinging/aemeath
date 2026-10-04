//! 三题型与校准级别的构造校验行为测试。

use super::*;

#[test]
fn scoring_state_with_prose_constructs() {
    let state = ScoringState::new("用户在排查日志路由问题。").expect("prose 应构造成功");
    assert_eq!(state.as_str(), "用户在排查日志路由问题。");
}

#[test]
fn scoring_state_with_blank_text_rejected() {
    assert!(ScoringState::new("   ").is_none());
    assert!(ScoringState::new("").is_none());
}

#[test]
fn noul_question_without_criteria_constructs() {
    let question =
        ScoringQuestion::noul("判断以下任务是否已经完成。", None).expect("noul 应构造成功");
    assert!(matches!(
        question,
        ScoringQuestion::Noul { criteria: None, .. }
    ));
}

#[test]
fn noul_question_with_criteria_constructs() {
    let criteria = NoulCriteria::new("任务目标已达成且验证通过。", "任务目标未达成。")
        .expect("完整句子应构造成功");
    let question =
        ScoringQuestion::noul("判断任务是否完成。", Some(criteria)).expect("noul 应构造成功");
    match question {
        ScoringQuestion::Noul {
            criteria: Some(criteria),
            ..
        } => {
            assert_eq!(criteria.when_true(), "任务目标已达成且验证通过。");
            assert_eq!(criteria.when_false(), "任务目标未达成。");
        }
        other => panic!("期望 Noul 带 criteria，实际 {other:?}"),
    }
}

#[test]
fn noul_question_with_blank_instructions_rejected() {
    assert!(ScoringQuestion::noul("  ", None).is_none());
}

#[test]
fn noul_criteria_with_blank_sentence_rejected() {
    assert!(NoulCriteria::new("", "任务未完成。").is_none());
    assert!(NoulCriteria::new("任务已完成。", "  ").is_none());
}

#[test]
fn choice_question_with_two_criteria_constructs() {
    let question = ScoringQuestion::choice(
        "哪个候选与问题最相关？",
        vec![
            ("a".to_owned(), "候选甲的完整描述句。".to_owned()),
            ("b".to_owned(), "候选乙的完整描述句。".to_owned()),
        ],
    )
    .expect("两个候选应构造成功");
    assert!(matches!(question, ScoringQuestion::Choice { .. }));
}

#[test]
fn choice_question_with_255_criteria_constructs() {
    let criteria: Vec<(String, String)> = (0..ScoringQuestion::CHOICE_CRITERIA_MAX)
        .map(|index| (index.to_string(), format!("候选 {index} 的完整描述句。")))
        .collect();
    assert!(ScoringQuestion::choice("哪个最相关？", criteria).is_ok());
}

#[test]
fn choice_question_with_one_criterion_rejected() {
    let rejected = ScoringQuestion::choice(
        "哪个最相关？",
        vec![("a".to_owned(), "唯一候选。".to_owned())],
    );
    assert_eq!(rejected, Err(QuestionRejected::TooFewCriteria));
}

#[test]
fn choice_question_with_256_criteria_rejected() {
    let criteria: Vec<(String, String)> = (0..=ScoringQuestion::CHOICE_CRITERIA_MAX)
        .map(|index| (index.to_string(), format!("候选 {index} 的完整描述句。")))
        .collect();
    assert_eq!(
        ScoringQuestion::choice("哪个最相关？", criteria),
        Err(QuestionRejected::TooManyCriteria)
    );
}

#[test]
fn choice_question_with_blank_instructions_rejected() {
    let rejected = ScoringQuestion::choice(
        " ",
        vec![
            ("a".to_owned(), "候选甲。".to_owned()),
            ("b".to_owned(), "候选乙。".to_owned()),
        ],
    );
    assert_eq!(rejected, Err(QuestionRejected::BlankInstructions));
}

#[test]
fn choice_question_with_duplicate_keys_rejected() {
    let rejected = ScoringQuestion::choice(
        "哪个最相关？",
        vec![
            ("a".to_owned(), "候选甲。".to_owned()),
            ("a".to_owned(), "候选甲重复 key。".to_owned()),
        ],
    );
    assert_eq!(rejected, Err(QuestionRejected::DuplicateCriterionKey));
}

#[test]
fn choice_question_with_blank_key_rejected() {
    let rejected = ScoringQuestion::choice(
        "哪个最相关？",
        vec![
            (" ".to_owned(), "候选甲。".to_owned()),
            ("b".to_owned(), "候选乙。".to_owned()),
        ],
    );
    assert_eq!(rejected, Err(QuestionRejected::BlankCriterionKey));
}

#[test]
fn choice_question_with_blank_description_rejected() {
    let rejected = ScoringQuestion::choice(
        "哪个最相关？",
        vec![
            ("a".to_owned(), "  ".to_owned()),
            ("b".to_owned(), "候选乙。".to_owned()),
        ],
    );
    assert_eq!(rejected, Err(QuestionRejected::BlankCriterionDescription));
}

#[test]
fn score_question_with_two_levels_constructs() {
    let question = ScoringQuestion::score(
        "评估该操作的风险等级。",
        vec!["低风险".to_owned(), "高风险".to_owned()],
    )
    .expect("两级应构造成功");
    assert!(matches!(question, ScoringQuestion::Score { .. }));
}

#[test]
fn score_question_with_one_level_rejected() {
    assert_eq!(
        ScoringQuestion::score("评估风险。", vec!["低风险".to_owned()]),
        Err(QuestionRejected::TooFewCriteria)
    );
}

#[test]
fn score_question_with_blank_level_rejected() {
    assert_eq!(
        ScoringQuestion::score("评估风险。", vec!["低风险".to_owned(), "".to_owned()]),
        Err(QuestionRejected::BlankLevel)
    );
}

#[test]
fn noul_answer_with_valid_probability_constructs() {
    let answer = ScoringAnswer::noul(0.93, CalibrationLevel::Raw).expect("合法概率应构造成功");
    assert!(matches!(answer, ScoringAnswer::Noul { p_true, .. } if p_true == 0.93));
}

#[test]
fn noul_answer_with_out_of_range_probability_rejected() {
    assert_eq!(
        ScoringAnswer::noul(1.01, CalibrationLevel::Raw),
        Err(AnswerRejected::ProbabilityOutOfRange)
    );
    assert_eq!(
        ScoringAnswer::noul(-0.01, CalibrationLevel::Raw),
        Err(AnswerRejected::ProbabilityOutOfRange)
    );
    assert_eq!(
        ScoringAnswer::noul(f64::NAN, CalibrationLevel::Raw),
        Err(AnswerRejected::ProbabilityOutOfRange)
    );
}

#[test]
fn choice_answer_with_consistent_key_constructs() {
    let answer = ScoringAnswer::choice(
        "a",
        vec![("a".to_owned(), 0.9), ("b".to_owned(), 0.1)],
        0.9,
        CalibrationLevel::Raw,
    )
    .expect("choice 在概率表中应构造成功");
    match answer {
        ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence,
            ..
        } => {
            assert_eq!(choice, "a");
            assert_eq!(probabilities.len(), 2);
            assert_eq!(confidence, 0.9);
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[test]
fn choice_answer_with_unknown_key_rejected() {
    assert_eq!(
        ScoringAnswer::choice(
            "c",
            vec![("a".to_owned(), 0.9), ("b".to_owned(), 0.1)],
            0.9,
            CalibrationLevel::Raw
        ),
        Err(AnswerRejected::ChoiceNotInProbabilities)
    );
}

#[test]
fn choice_answer_with_empty_probabilities_rejected() {
    assert_eq!(
        ScoringAnswer::choice("a", vec![], 0.0, CalibrationLevel::Raw),
        Err(AnswerRejected::EmptyProbabilities)
    );
}

#[test]
fn choice_answer_with_out_of_range_probability_rejected() {
    assert_eq!(
        ScoringAnswer::choice("a", vec![("a".to_owned(), 1.5)], 0.9, CalibrationLevel::Raw),
        Err(AnswerRejected::ProbabilityOutOfRange)
    );
}

#[test]
fn score_answer_constructs_and_validates() {
    let answer = ScoringAnswer::score(0.5, vec![0.2, 0.5, 0.3], 0.5, CalibrationLevel::Raw)
        .expect("合法响应应构造成功");
    assert!(matches!(answer, ScoringAnswer::Score { .. }));
    assert_eq!(
        ScoringAnswer::score(0.5, vec![], 0.0, CalibrationLevel::Raw),
        Err(AnswerRejected::EmptyProbabilities)
    );
    assert_eq!(
        ScoringAnswer::score(0.5, vec![0.2, f64::NAN], 0.5, CalibrationLevel::Raw),
        Err(AnswerRejected::ProbabilityOutOfRange)
    );
}
