//! bailian_wire 契约测试：TypeSafe System One 协议形状与领域类型互转。
//!
//! fixture 采用 2026-10-26 对百炼 `decision-model-preview` 的实测响应原文，
//! 钉死 wire 形状（beta 阶段协议漂移必须被本文件捕获，设计 #2095）。

use super::*;
use crate::domain::published_language::{CalibrationLevel, NoulCriteria, ScoringQuestion};

fn sample_questions() -> Vec<ScoringQuestion> {
    vec![
        ScoringQuestion::choice(
            "哪条记忆与当前对话最相关？",
            vec![
                ("m1".to_string(), "关于预防感冒的卫生习惯".to_string()),
                ("m2".to_string(), "Rust 编程语言的内存安全特性".to_string()),
                ("m3".to_string(), "今天的股市行情".to_string()),
            ],
        )
        .expect("合法 choice 题"),
        ScoringQuestion::noul("这段文本是否为中文？", None).expect("合法 noul 题"),
        ScoringQuestion::score(
            "问题的严重程度如何？",
            vec![
                "轻微问题，不影响功能".to_string(),
                "部分功能受影响，但存在替代方案".to_string(),
                "核心功能不可用，没有替代方案".to_string(),
            ],
        )
        .expect("合法 score 题"),
    ]
}

fn sample_state() -> ScoringState {
    ScoringState::new("用户问：如何预防感冒？勤洗手是有效方法。").expect("非空 state")
}

#[test]
fn build_request_assignes_sequential_question_ids() {
    let request = build_request(
        "decision-model-preview",
        &sample_state(),
        &sample_questions(),
    );
    let keys: Vec<&String> = request.questions.keys().collect();
    assert_eq!(keys, ["q0", "q1", "q2"], "qid 按请求顺序生成");
    assert_eq!(request.model, "decision-model-preview");
    assert_eq!(request.state, "用户问：如何预防感冒？勤洗手是有效方法。");
}

#[test]
fn build_request_wire_shape_is_pinned() {
    let request = build_request(
        "decision-model-preview",
        &sample_state(),
        &sample_questions(),
    );
    let value = serde_json::to_value(&request).expect("请求应可序列化");
    let expected = serde_json::json!({
        "model": "decision-model-preview",
        "state": "用户问：如何预防感冒？勤洗手是有效方法。",
        "questions": {
            "q0": {
                "type": "choice",
                "instructions": "哪条记忆与当前对话最相关？",
                "criteria": {
                    "m1": "关于预防感冒的卫生习惯",
                    "m2": "Rust 编程语言的内存安全特性",
                    "m3": "今天的股市行情"
                }
            },
            "q1": {
                "type": "noul",
                "instructions": "这段文本是否为中文？"
            },
            "q2": {
                "type": "score",
                "instructions": "问题的严重程度如何？",
                "criteria": [
                    "轻微问题，不影响功能",
                    "部分功能受影响，但存在替代方案",
                    "核心功能不可用，没有替代方案"
                ]
            }
        }
    });
    assert_eq!(value, expected, "请求 wire 形状必须钉死（beta 防漂移）");
}

#[test]
fn build_request_serializes_noul_criteria_when_present() {
    let questions = vec![ScoringQuestion::noul(
        "是否需要人工处理？",
        Some(
            NoulCriteria::new(
                "存在退款、投诉或客户明确要求人工介入",
                "问题清晰、风险较低，可由自动化流程处理",
            )
            .expect("合法 criteria"),
        ),
    )
    .expect("合法 noul 题")];
    let request = build_request("decision-model-preview", &sample_state(), &questions);
    let value = serde_json::to_value(&request).expect("序列化");
    assert_eq!(
        value["questions"]["q0"]["criteria"],
        serde_json::json!({
            "true": "存在退款、投诉或客户明确要求人工介入",
            "false": "问题清晰、风险较低，可由自动化流程处理"
        }),
        "noul criteria 应映射为 true/false 对象"
    );
}

/// 实测响应原文（choice，2026-10-26 token-plan 网关）。
const REAL_CHOICE_RESPONSE: &str = r#"{"model":"decision-model-preview","request_id":"c1bba89f-0177-4a9b-ad25-d1f41e8124b6","answers":{"rank":{"type":"choice","choice":"m1","confidence":1.0,"probabilities":{"m1":1.0,"m2":0.0,"m3":0.0}}},"usage":{"input_tokens":60},"latency_ms":572.1}"#;

/// 实测响应原文（noul，2026-10-26 token-plan 网关；qid 改写为 q0 模拟单题请求）。
const REAL_NOUL_RESPONSE: &str = r#"{"model":"decision-model-preview","request_id":"ab2602c0-d325-435c-bd67-4f72469ec539","answers":{"q0":{"type":"noul","noul":1.0}},"usage":{"input_tokens":23},"latency_ms":39.1}"#;

/// 文档示例响应原文（score；qid 改写为 q0 模拟单题请求）。
const DOC_SCORE_RESPONSE: &str = r#"{"model":"decision-model-preview","request_id":"req-1","answers":{"q0":{"type":"score","score":2.25,"confidence":0.91,"legend":{"0":"轻微问题，不影响功能","1":"部分功能受影响，但存在替代方案","2":"核心功能不可用，没有替代方案"},"probabilities":{"0":0.03,"1":0.12,"2":0.85}}},"usage":{"input_tokens":125},"latency_ms":52.9}"#;

#[test]
fn parse_real_choice_response_restores_request_order_probabilities() {
    let response: BailianSystemOneResponse =
        serde_json::from_str(REAL_CHOICE_RESPONSE).expect("实测响应必须可解析");
    let questions = vec![sample_questions()[0].clone()];
    // 响应 qid 是 rank（历史请求）；改写为 q0 模拟本客户端请求。
    let mut response = response;
    let answer = response.answers.remove("rank").expect("rank 答案应存在");
    response.answers.insert("q0".to_string(), answer);

    let answers = parse_answers(&response, &questions).expect("解析应成功");
    assert_eq!(answers.len(), 1);
    let ScoringAnswer::Choice {
        choice,
        probabilities,
        confidence,
        calibration,
    } = &answers[0]
    else {
        panic!("应为 Choice 答案");
    };
    assert_eq!(choice, "m1");
    assert_eq!(confidence, &1.0);
    assert_eq!(calibration, &CalibrationLevel::Raw);
    assert_eq!(
        probabilities,
        &vec![
            ("m1".to_string(), 1.0),
            ("m2".to_string(), 0.0),
            ("m3".to_string(), 0.0)
        ],
        "probabilities 按请求 criteria 顺序还原，不依赖响应 map 迭代序"
    );
}

#[test]
fn parse_real_noul_response_maps_p_true() {
    let response: BailianSystemOneResponse =
        serde_json::from_str(REAL_NOUL_RESPONSE).expect("实测响应必须可解析");
    let questions = vec![sample_questions()[1].clone()];
    let answers = parse_answers(&response, &questions).expect("解析应成功");
    let ScoringAnswer::Noul {
        p_true,
        calibration,
    } = &answers[0]
    else {
        panic!("应为 Noul 答案");
    };
    assert_eq!(p_true, &1.0);
    assert_eq!(calibration, &CalibrationLevel::Raw);
}

#[test]
fn parse_doc_score_response_restores_index_ordered_probabilities() {
    let response: BailianSystemOneResponse =
        serde_json::from_str(DOC_SCORE_RESPONSE).expect("文档响应必须可解析");
    let questions = vec![sample_questions()[2].clone()];
    let answers = parse_answers(&response, &questions).expect("解析应成功");
    let ScoringAnswer::Score {
        score,
        probabilities,
        confidence,
        ..
    } = &answers[0]
    else {
        panic!("应为 Score 答案");
    };
    assert_eq!(score, &2.25);
    assert_eq!(confidence, &0.91);
    assert_eq!(
        probabilities,
        &vec![0.03, 0.12, 0.85],
        "按档位 index 升序还原"
    );
}

#[test]
fn parse_answers_rejects_missing_question_id() {
    let response: BailianSystemOneResponse = serde_json::from_value(serde_json::json!({
        "model": "decision-model-preview",
        "request_id": "r",
        "answers": {},
        "usage": {"input_tokens": 1},
        "latency_ms": 1.0
    }))
    .expect("结构合法");
    let questions = vec![sample_questions()[0].clone()];
    let error = parse_answers(&response, &questions).expect_err("缺 qid 必须报错");
    assert_eq!(
        error,
        BailianWireError::MissingAnswer {
            question_id: "q0".into()
        }
    );
}

#[test]
fn parse_answers_rejects_type_mismatch() {
    let response: BailianSystemOneResponse = serde_json::from_value(serde_json::json!({
        "model": "decision-model-preview",
        "request_id": "r",
        "answers": {
            "q0": {"type": "noul", "noul": 0.5}
        },
        "usage": {"input_tokens": 1},
        "latency_ms": 1.0
    }))
    .expect("结构合法");
    let questions = vec![sample_questions()[0].clone()];
    let error = parse_answers(&response, &questions).expect_err("题型不匹配必须报错");
    assert_eq!(
        error,
        BailianWireError::TypeMismatch {
            question_id: "q0".into()
        }
    );
}

#[test]
fn parse_choice_fills_missing_candidates_with_zero_probability() {
    // 响应 probabilities 缺 m2（协议允许仅返回部分候选时防御性补 0）。
    let response: BailianSystemOneResponse = serde_json::from_value(serde_json::json!({
        "model": "decision-model-preview",
        "request_id": "r",
        "answers": {
            "q0": {"type": "choice", "choice": "m1", "confidence": 0.9,
                   "probabilities": {"m1": 0.9, "m3": 0.1}}
        },
        "usage": {"input_tokens": 1},
        "latency_ms": 1.0
    }))
    .expect("结构合法");
    let questions = vec![sample_questions()[0].clone()];
    let answers = parse_answers(&response, &questions).expect("补 0 后应可构造");
    let ScoringAnswer::Choice { probabilities, .. } = &answers[0] else {
        panic!("应为 Choice 答案");
    };
    assert_eq!(
        probabilities,
        &vec![
            ("m1".to_string(), 0.9),
            ("m2".to_string(), 0.0),
            ("m3".to_string(), 0.1)
        ],
        "缺失候选按请求序补 0"
    );
}

#[test]
fn parse_score_fills_missing_levels_with_zero_probability() {
    let response: BailianSystemOneResponse = serde_json::from_value(serde_json::json!({
        "model": "decision-model-preview",
        "request_id": "r",
        "answers": {
            "q0": {"type": "score", "score": 1.4, "confidence": 0.8,
                   "legend": {"0": "a", "1": "b", "2": "c"},
                   "probabilities": {"0": 0.2, "2": 0.8}}
        },
        "usage": {"input_tokens": 1},
        "latency_ms": 1.0
    }))
    .expect("结构合法");
    let questions = vec![sample_questions()[2].clone()];
    let answers = parse_answers(&response, &questions).expect("补 0 后应可构造");
    let ScoringAnswer::Score { probabilities, .. } = &answers[0] else {
        panic!("应为 Score 答案");
    };
    assert_eq!(probabilities, &vec![0.2, 0.0, 0.8], "缺失档位补 0");
}

#[test]
fn roundtrip_all_three_question_kinds_preserves_semantics() {
    let questions = sample_questions();
    let response = BailianSystemOneResponse {
        model: "decision-model-preview".into(),
        request_id: "roundtrip".into(),
        answers: std::collections::BTreeMap::from([
            (
                "q0".into(),
                BailianAnswerWire::Choice {
                    choice: "m2".into(),
                    confidence: 0.55,
                    probabilities: std::collections::BTreeMap::from([
                        ("m1".into(), 0.2),
                        ("m2".into(), 0.55),
                        ("m3".into(), 0.25),
                    ]),
                },
            ),
            ("q1".into(), BailianAnswerWire::Noul { noul: 0.25 }),
            (
                "q2".into(),
                BailianAnswerWire::Score {
                    score: 1.0,
                    confidence: 0.7,
                    legend: std::collections::BTreeMap::new(),
                    probabilities: std::collections::BTreeMap::from([
                        ("0".into(), 0.0),
                        ("1".into(), 1.0),
                        ("2".into(), 0.0),
                    ]),
                },
            ),
        ]),
        usage: BailianUsageWire { input_tokens: 42 },
        latency_ms: 12.5,
    };
    let answers = parse_answers(&response, &questions).expect("往返应成功");
    assert_eq!(answers.len(), 3);
    assert!(matches!(&answers[0], ScoringAnswer::Choice { choice, .. } if choice == "m2"));
    assert!(
        matches!(&answers[1], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.25).abs() < f64::EPSILON)
    );
    assert!(
        matches!(&answers[2], ScoringAnswer::Score { score, .. } if (*score - 1.0).abs() < f64::EPSILON)
    );
}
