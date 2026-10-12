//! Daemon UDS 协议：length-prefixed JSON 帧与 answer DTO 行为测试。

use super::*;
use crate::constants::DAEMON_PROTOCOL_VERSION;
use crate::domain::published_language::{
    CalibrationLevel, NoulCriteria, ScoringAnswer, ScoringQuestion, ScoringState,
};

fn sample_state() -> ScoringState {
    ScoringState::new("用户在排查日志路由问题。").expect("state 非空")
}

fn sample_question() -> ScoringQuestion {
    ScoringQuestion::noul(
        "判断以下任务是否已经完成。",
        Some(
            NoulCriteria::new("任务目标已达成且验证通过。", "任务目标未达成或验证失败。")
                .expect("criteria"),
        ),
    )
    .expect("noul")
}

fn sample_answer() -> ScoringAnswer {
    ScoringAnswer::noul(0.73, CalibrationLevel::Temperature).expect("noul answer")
}

#[test]
fn encode_decode_frame_round_trips_json_body() {
    let body = br#"{"hello":"world"}"#;
    let frame = encode_frame(body);
    assert_eq!(&frame[..4], &(body.len() as u32).to_le_bytes());
    assert_eq!(&frame[4..], body);

    let (decoded, consumed) = try_decode_frame(&frame).expect("完整帧应可解码");
    assert_eq!(decoded, body);
    assert_eq!(consumed, frame.len());
}

#[test]
fn try_decode_frame_returns_incomplete_when_length_prefix_or_body_missing() {
    assert!(matches!(try_decode_frame(&[]), Err(FrameError::Incomplete)));
    assert!(matches!(
        try_decode_frame(&[1, 0, 0]),
        Err(FrameError::Incomplete)
    ));
    let mut truncated = encode_frame(br#"{"a":1}"#);
    truncated.pop();
    assert!(matches!(
        try_decode_frame(&truncated),
        Err(FrameError::Incomplete)
    ));
}

#[test]
fn answer_request_json_round_trip_preserves_state_and_questions() {
    let request = DaemonRequest::answer(
        Some("corr-1".to_string()),
        AnswerParams {
            scenario: Some("memory_rerank".to_string()),
            state: sample_state(),
            questions: vec![sample_question()],
        },
    );
    let frame = encode_json_frame(&request).expect("编码请求");
    let (decoded, consumed) = decode_answer_request(&frame).expect("解码请求");
    assert_eq!(consumed, frame.len());
    assert_eq!(decoded.protocol_version(), DAEMON_PROTOCOL_VERSION);
    match decoded {
        DaemonRequest::Answer { id, params, .. } => {
            assert_eq!(id.as_deref(), Some("corr-1"));
            assert_eq!(params.scenario.as_deref(), Some("memory_rerank"));
            assert_eq!(params.state, sample_state());
            assert_eq!(params.questions, vec![sample_question()]);
        }
    }
}

#[test]
fn decode_answer_request_rejects_protocol_version_mismatch() {
    let request = DaemonRequest::answer(
        None,
        AnswerParams {
            scenario: None,
            state: sample_state(),
            questions: vec![sample_question()],
        },
    );
    // 直接改 wire JSON 的 version，绕过构造器保护。
    let mut value = serde_json::to_value(&request).expect("serialize");
    value["protocol_version"] = serde_json::json!(99);
    let frame = encode_frame(serde_json::to_vec(&value).expect("body").as_slice());

    let err = decode_answer_request(&frame).expect_err("版本不匹配应失败");
    assert!(matches!(
        err,
        ProtocolError::VersionMismatch {
            found: 99,
            expected: DAEMON_PROTOCOL_VERSION
        }
    ));
}

#[test]
fn decode_answer_request_rejects_unknown_method() {
    let body = serde_json::json!({
        "protocol_version": DAEMON_PROTOCOL_VERSION,
        "method": "ping",
        "id": null,
        "params": {
            "state": "hello",
            "questions": []
        }
    });
    let frame = encode_frame(serde_json::to_vec(&body).expect("body").as_slice());
    let err = decode_answer_request(&frame).expect_err("未知 method 应失败");
    assert!(matches!(err, ProtocolError::UnsupportedMethod(_)));
}

#[test]
fn answer_response_ok_and_error_round_trip() {
    let ok = DaemonResponse::ok(
        Some("corr-1".to_string()),
        AnswerResult {
            answers: vec![sample_answer()],
            engine_revision: "kev-0.8b@test".to_string(),
            latency_ms: 42,
        },
    );
    let frame = encode_json_frame(&ok).expect("编码 ok");
    let (decoded, _) = decode_response(&frame).expect("解码 ok");
    match decoded {
        DaemonResponse::Ok { id, result, .. } => {
            assert_eq!(id.as_deref(), Some("corr-1"));
            assert_eq!(result.engine_revision, "kev-0.8b@test");
            assert_eq!(result.latency_ms, 42);
            assert_eq!(result.answers, vec![sample_answer()]);
        }
        DaemonResponse::Err { .. } => panic!("应为 Ok"),
    }

    let err = DaemonResponse::error(
        None,
        DaemonErrorBody {
            kind: DaemonErrorKind::Unavailable,
            message: "model missing".to_string(),
        },
    );
    let frame = encode_json_frame(&err).expect("编码 err");
    let (decoded, _) = decode_response(&frame).expect("解码 err");
    match decoded {
        DaemonResponse::Err { id, error, .. } => {
            assert!(id.is_none());
            assert_eq!(error.kind, DaemonErrorKind::Unavailable);
            assert_eq!(error.message, "model missing");
        }
        DaemonResponse::Ok { .. } => panic!("应为 Err"),
    }
}
