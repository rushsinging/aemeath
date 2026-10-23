//! 证据聚合契约测试（#2048 commit 1）。

use crate::domain::evidence::{
    aggregate_evidence, validate_evidence_bundle, EvidenceBundle, SessionEventRecord, SignalKind,
};

fn tool_event(tool: &str, error: Option<&str>, result_chars: usize) -> SessionEventRecord {
    SessionEventRecord {
        session_id: "s1".into(),
        run_id: "r1".into(),
        tool_name: Some(tool.into()),
        is_error: error.is_some(),
        error_text: error.map(str::to_string),
        result_chars,
        call_fingerprint: None,
    }
}

/// 同 (tool, 签名) 失败 ≥2 聚合为 repeated_failure；数字归一化后同签名合并。
#[test]
fn repeated_failure_aggregates_by_signature() {
    let events = vec![
        tool_event("Bash", Some("exit code 1: cargo build failed"), 100),
        tool_event("Bash", Some("exit code 2: cargo build failed"), 100),
        tool_event("Bash", Some("other error"), 100),
    ];
    let bundle = aggregate_evidence(&events);
    let repeated: Vec<_> = bundle
        .signals
        .iter()
        .filter(|signal| signal.kind == SignalKind::RepeatedFailure)
        .collect();
    assert_eq!(repeated.len(), 1, "exit 1/2 数字归一后合并");
    assert!(repeated[0].summary.contains("×2"));
    assert!(repeated[0].signal_id.starts_with("repeated_failure:Bash:"));
}

/// 单次失败不产生 repeated_failure（NEVER 把孤立错误当模式）。
#[test]
fn single_failure_is_not_repeated() {
    let events = vec![tool_event("Edit", Some("old_string not found"), 50)];
    let bundle = aggregate_evidence(&events);
    assert!(!bundle
        .signals
        .iter()
        .any(|signal| signal.kind == SignalKind::RepeatedFailure));
}

/// 同指纹调用 ≥2 → retry_pattern。
#[test]
fn retry_pattern_detected_by_fingerprint() {
    let mut first = tool_event("Edit", Some("old_string not found"), 50);
    first.call_fingerprint = Some("edit:file.rs:anchor".into());
    let mut second = tool_event("Edit", None, 50);
    second.call_fingerprint = Some("edit:file.rs:anchor".into());
    let bundle = aggregate_evidence(&[first, second]);
    assert!(bundle
        .signals
        .iter()
        .any(|signal| signal.kind == SignalKind::RetryPattern && signal.summary.contains("×2")));
}

/// 超长结果按工具聚合计数。
#[test]
fn oversized_results_counted_by_tool() {
    let events = vec![
        tool_event("Bash", None, 4000),
        tool_event("Bash", None, 5000),
        tool_event("Grep", None, 100),
    ];
    let bundle = aggregate_evidence(&events);
    let oversized: Vec<_> = bundle
        .signals
        .iter()
        .filter(|signal| signal.kind == SignalKind::OversizedResult)
        .collect();
    assert_eq!(oversized.len(), 1);
    assert!(oversized[0].summary.contains("Bash"));
    assert!(oversized[0].summary.contains("×2"));
}

/// run 级事件聚合为终态分布；空事件流产空块。
#[test]
fn run_outcomes_aggregate_and_empty_stream_yields_empty() {
    let run_event = |outcome: &str| SessionEventRecord {
        session_id: "s1".into(),
        run_id: format!("r-{outcome}"),
        tool_name: None,
        is_error: false,
        error_text: Some(outcome.into()),
        result_chars: 0,
        call_fingerprint: None,
    };
    let bundle = aggregate_evidence(&[
        run_event("completed"),
        run_event("completed"),
        run_event("cancelled"),
    ]);
    let outcome_signal = bundle
        .signals
        .iter()
        .find(|signal| signal.kind == SignalKind::RunOutcome)
        .expect("终态分布必须存在");
    assert!(outcome_signal.summary.contains("completed×2"));
    assert!(outcome_signal.summary.contains("cancelled×1"));

    let empty = aggregate_evidence(&[]);
    assert_eq!(empty, EvidenceBundle::default());
}

/// sessions_covered 统计去重 session；信号数上限校验。
#[test]
fn sessions_covered_and_signal_cap() {
    let mut events = Vec::new();
    for index in 0..3 {
        events.push(SessionEventRecord {
            session_id: format!("s{index}"),
            run_id: "r".into(),
            tool_name: Some("Bash".into()),
            is_error: true,
            error_text: Some("exit code 1: boom".to_string()),
            result_chars: 10,
            call_fingerprint: None,
        });
    }
    let bundle = aggregate_evidence(&events);
    assert_eq!(bundle.sessions_covered, 3);
    assert!(validate_evidence_bundle(&bundle).is_ok());

    let oversized_bundle = EvidenceBundle {
        signals: (0..65)
            .map(|index| crate::domain::evidence::EvidenceSignal {
                kind: SignalKind::RunOutcome,
                signal_id: format!("sig-{index}"),
                summary: "x".into(),
                excerpt: None,
            })
            .collect(),
        sessions_covered: 0,
    };
    assert!(validate_evidence_bundle(&oversized_bundle).is_err());
}

/// 优先级排序：repeated_failure 在 retry/oversized/outcome 之前。
#[test]
fn signals_ordered_by_priority() {
    let mut retry = tool_event("Edit", Some("old_string not found"), 50);
    retry.call_fingerprint = Some("f".into());
    let mut retry2 = tool_event("Edit", None, 50);
    retry2.call_fingerprint = Some("f".into());
    let events = vec![
        tool_event("Bash", Some("exit code 1: fail"), 10),
        tool_event("Bash", Some("exit code 2: fail"), 10),
        retry,
        retry2,
        tool_event("Grep", None, 4000),
    ];
    let bundle = aggregate_evidence(&events);
    let kinds: Vec<SignalKind> = bundle.signals.iter().map(|s| s.kind).collect();
    let repeated_pos = kinds
        .iter()
        .position(|k| *k == SignalKind::RepeatedFailure)
        .unwrap();
    let retry_pos = kinds
        .iter()
        .position(|k| *k == SignalKind::RetryPattern)
        .unwrap();
    let oversized_pos = kinds
        .iter()
        .position(|k| *k == SignalKind::OversizedResult)
        .unwrap();
    assert!(repeated_pos < retry_pos);
    assert!(retry_pos < oversized_pos);
}
