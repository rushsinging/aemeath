//! Jev 线格式 L3 契约测试：请求序列化与响应解析和协议形态逐字段对齐。
//!
//! 协议形态实证来源：`eval/system-one/harness/run_eval.py`（六引擎实测线格式）。

use super::*;
use crate::domain::{NoulCriteria, ScoringQuestion, ScoringState};

fn sample_state() -> ScoringState {
    ScoringState::new("用户正在调试日志路由。").expect("state 构造")
}

#[test]
fn noul_request_serializes_wire_format_without_criteria() {
    let questions = vec![(
        "q0".to_owned(),
        ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造"),
    )];
    let request = WireRequest {
        state: &sample_state(),
        model: "kev-latest",
        questions: &questions,
    };
    let body = serde_json::to_string(&request).expect("序列化");
    let document: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(document["state"], "用户正在调试日志路由。");
    assert_eq!(document["model"], "kev-latest");
    assert_eq!(document["questions"]["q0"]["type"], "noul");
    assert_eq!(
        document["questions"]["q0"]["instructions"],
        "任务是否已完成？"
    );
    assert!(document["questions"]["q0"].get("criteria").is_none());
}

#[test]
fn noul_request_with_criteria_serializes_true_false_sentences() {
    let criteria =
        NoulCriteria::new("任务目标已达成。", "任务目标未达成。").expect("criteria 构造");
    let questions = vec![(
        "q0".to_owned(),
        ScoringQuestion::noul("任务是否已完成？", Some(criteria)).expect("question 构造"),
    )];
    let request = WireRequest {
        state: &sample_state(),
        model: "kev-latest",
        questions: &questions,
    };
    let body = serde_json::to_string(&request).expect("序列化");
    let document: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(
        document["questions"]["q0"]["criteria"]["true"],
        "任务目标已达成。"
    );
    assert_eq!(
        document["questions"]["q0"]["criteria"]["false"],
        "任务目标未达成。"
    );
}

#[test]
fn choice_request_preserves_criteria_insertion_order() {
    let questions = vec![(
        "q0".to_owned(),
        ScoringQuestion::choice(
            "哪个候选最相关？",
            vec![
                ("zeta".to_owned(), "候选泽塔。".to_owned()),
                ("alpha".to_owned(), "候选阿尔法。".to_owned()),
                ("mid".to_owned(), "候选中间。".to_owned()),
            ],
        )
        .expect("question 构造"),
    )];
    let request = WireRequest {
        state: &sample_state(),
        model: "kev-latest",
        questions: &questions,
    };
    let body = serde_json::to_string(&request).expect("序列化");
    let criteria_position = body.find("\"criteria\"").expect("含 criteria");
    let zeta = body.find("\"zeta\"").expect("含 zeta");
    let alpha = body.find("\"alpha\"").expect("含 alpha");
    let mid = body.find("\"mid\"").expect("含 mid");
    assert!(
        criteria_position < zeta && zeta < alpha && alpha < mid,
        "criteria 必须按插入序序列化：{body}"
    );
}

#[test]
fn score_request_serializes_levels_as_ordered_array() {
    let questions = vec![(
        "q0".to_owned(),
        ScoringQuestion::score(
            "评估风险等级。",
            vec!["低".to_owned(), "中".to_owned(), "高".to_owned()],
        )
        .expect("question 构造"),
    )];
    let request = WireRequest {
        state: &sample_state(),
        model: "kev-latest",
        questions: &questions,
    };
    let body = serde_json::to_string(&request).expect("序列化");
    let document: serde_json::Value = serde_json::from_str(&body).expect("合法 JSON");
    assert_eq!(
        document["questions"]["q0"]["criteria"],
        serde_json::json!(["低", "中", "高"])
    );
}

#[test]
fn parse_noul_answer_from_wire_response() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"noul": 0.9325}}}"#;
    let answers = parse_answers(body, &requests).expect("解析应成功");
    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.9325).abs() < 1e-9)
    );
}

#[test]
fn parse_choice_answer_with_explicit_confidence() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::choice(
            "哪个最相关？",
            vec![
                ("a".to_owned(), "候选甲。".to_owned()),
                ("b".to_owned(), "候选乙。".to_owned()),
            ],
        )
        .expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"choice": "a", "probabilities": {"a": 0.9, "b": 0.1}, "confidence": 0.88}}}"#;
    let answers = parse_answers(body, &requests).expect("解析应成功");
    match &answers[0] {
        ScoringAnswer::Choice {
            choice,
            probabilities,
            confidence,
            ..
        } => {
            assert_eq!(choice, "a");
            assert_eq!(probabilities.len(), 2);
            assert!(probabilities
                .iter()
                .any(|(key, p)| key == "a" && (*p - 0.9).abs() < 1e-9));
            assert!((*confidence - 0.88).abs() < 1e-9);
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[test]
fn parse_choice_answer_defaults_confidence_to_top_probability() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::choice(
            "哪个最相关？",
            vec![
                ("a".to_owned(), "候选甲。".to_owned()),
                ("b".to_owned(), "候选乙。".to_owned()),
            ],
        )
        .expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"choice": "b", "probabilities": {"a": 0.2, "b": 0.8}}}}"#;
    let answers = parse_answers(body, &requests).expect("解析应成功");
    match &answers[0] {
        ScoringAnswer::Choice {
            choice, confidence, ..
        } => {
            assert_eq!(choice, "b");
            assert!((*confidence - 0.8).abs() < 1e-9, "confidence 应取最高概率");
        }
        other => panic!("期望 Choice，实际 {other:?}"),
    }
}

#[test]
fn parse_score_answer_from_wire_response() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::score(
            "评估风险。",
            vec!["低".to_owned(), "中".to_owned(), "高".to_owned()],
        )
        .expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"score": 0.55, "probabilities": [0.2, 0.5, 0.3]}}}"#;
    let answers = parse_answers(body, &requests).expect("解析应成功");
    match &answers[0] {
        ScoringAnswer::Score {
            score,
            probabilities,
            confidence,
            ..
        } => {
            assert!((*score - 0.55).abs() < 1e-9);
            assert_eq!(probabilities, &vec![0.2, 0.5, 0.3]);
            assert!((*confidence - 0.5).abs() < 1e-9);
        }
        other => panic!("期望 Score，实际 {other:?}"),
    }
}

#[test]
fn parse_score_answer_rejects_probabilities_levels_count_mismatch() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::score(
            "评估风险。",
            vec!["低".to_owned(), "中".to_owned(), "高".to_owned()],
        )
        .expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"score": 0.5, "probabilities": [0.5, 0.5]}}}"#;
    let rejected = parse_answers(body, &requests);
    assert!(matches!(rejected, Err(WireRejected::InvalidAnswer(_))));
}

#[test]
fn parse_answers_preserves_request_order_regardless_of_response_order() {
    let requests = vec![
        (
            "q0".to_owned(),
            ScoringQuestion::noul("甲命题是否为真？", None).expect("question 构造"),
        ),
        (
            "q1".to_owned(),
            ScoringQuestion::noul("乙命题是否为真？", None).expect("question 构造"),
        ),
    ];
    let body = r#"{"answers": {"q1": {"noul": 0.2}, "q0": {"noul": 0.9}}}"#;
    let answers = parse_answers(body, &requests).expect("解析应成功");
    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.9).abs() < 1e-9)
    );
    assert!(
        matches!(&answers[1], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.2).abs() < 1e-9)
    );
}

#[test]
fn parse_answers_rejects_missing_question_id() {
    let requests = vec![
        (
            "q0".to_owned(),
            ScoringQuestion::noul("甲命题？", None).expect("question 构造"),
        ),
        (
            "q1".to_owned(),
            ScoringQuestion::noul("乙命题？", None).expect("question 构造"),
        ),
    ];
    let body = r#"{"answers": {"q0": {"noul": 0.9}}}"#;
    let rejected = parse_answers(body, &requests);
    assert_eq!(rejected, Err(WireRejected::MissingAnswer("q1".to_owned())));
}

#[test]
fn parse_answers_rejects_malformed_body() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::noul("甲命题？", None).expect("question 构造"),
    )];
    assert_eq!(
        parse_answers("not json", &requests),
        Err(WireRejected::MalformedBody)
    );
    assert_eq!(
        parse_answers(r#"{"unexpected": {}}"#, &requests),
        Err(WireRejected::MalformedBody)
    );
}

#[test]
fn parse_answers_rejects_out_of_range_probability() {
    let requests = vec![(
        "q0".to_owned(),
        ScoringQuestion::noul("甲命题？", None).expect("question 构造"),
    )];
    let body = r#"{"answers": {"q0": {"noul": 1.5}}}"#;
    assert!(matches!(
        parse_answers(body, &requests),
        Err(WireRejected::InvalidAnswer(_))
    ));
}
