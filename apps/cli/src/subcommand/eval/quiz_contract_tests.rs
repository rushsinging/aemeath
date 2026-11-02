//! L3 契约测试：评测产物 JSON schema 稳定性（键存在性，不锁取值）。
//!
//! 这些结构是评测链路的对外契约（`results/` 入库、下游报告消费），
//! 字段改名/删除会静默破坏历史基线对比，因此用必需键集合钉死。

use serde_json::Value;

fn keys(value: &Value) -> Vec<String> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

fn assert_has_keys(value: &Value, required: &[&str]) {
    let present = keys(value);
    for key in required {
        assert!(
            present.iter().any(|item| item == key),
            "缺少契约字段 {key}；实际字段: {present:?}"
        );
    }
}

#[test]
fn quiz_report_schema_is_stable() {
    let report = super::QuizReport {
        answerer: Some("a/m".to_string()),
        judge: Some("j/m".to_string()),
        cases_total: 1,
        evaluated: 1,
        parse_failed: 0,
        correct_rate_mean: Some(1.0),
        per_epoch: vec![super::EpochStat {
            epoch: "early".to_string(),
            questions: 1,
            correct: 1,
            correct_rate: Some(1.0),
        }],
        usage: vec![super::QuizUsage {
            stage: "answer".to_string(),
            input_tokens: Some(1),
            output_tokens: Some(1),
            reasoning_tokens: Some(0),
        }],
        cases: vec![super::CaseQuiz {
            case_id: "c1".to_string(),
            status: "evaluated".to_string(),
            questions_total: 1,
            answered: 1,
            correct: 1,
            correct_rate: Some(1.0),
            error: None,
            items: vec![super::QuizItem {
                question: "q".to_string(),
                gold: Some("g".to_string()),
                answer: Some("a".to_string()),
                correct: Some(true),
                evidence: Some("e".to_string()),
                epoch: Some("early".to_string()),
                kind: Some("decision".to_string()),
            }],
            per_epoch: Vec::new(),
        }],
    };
    let value = serde_json::to_value(&report).expect("quiz json");
    assert_has_keys(
        &value,
        &[
            "answerer",
            "judge",
            "cases_total",
            "evaluated",
            "parse_failed",
            "correct_rate_mean",
            "per_epoch",
            "usage",
            "cases",
        ],
    );
    assert_has_keys(
        &value["per_epoch"][0],
        &["epoch", "questions", "correct", "correct_rate"],
    );
    assert_has_keys(
        &value["usage"][0],
        &["stage", "input_tokens", "output_tokens", "reasoning_tokens"],
    );
    assert_has_keys(
        &value["cases"][0]["items"][0],
        &[
            "question", "gold", "answer", "correct", "evidence", "epoch", "kind",
        ],
    );
}

#[test]
fn question_set_round_trips_through_json() {
    let set = super::QuestionSet {
        case_id: "c1".to_string(),
        items: vec![super::QuizItem {
            question: "q".to_string(),
            gold: Some("g".to_string()),
            answer: None,
            correct: None,
            evidence: None,
            epoch: Some("early".to_string()),
            kind: Some("constraint".to_string()),
        }],
    };
    let text = serde_json::to_string(&set).expect("serialize");
    let decoded: Vec<super::QuestionSet> =
        serde_json::from_str(&format!("[{text}]")).expect("deserialize 题库");
    assert_eq!(decoded[0].case_id, "c1");
    assert_eq!(decoded[0].items.len(), 1);
    assert_eq!(decoded[0].items[0].kind.as_deref(), Some("constraint"));
}
