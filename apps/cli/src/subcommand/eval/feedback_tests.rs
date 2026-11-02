use super::*;

fn user(text: &str) -> Value {
    serde_json::json!({ "role": "user", "content": [{ "type": "text", "text": text }] })
}

fn assistant_read(path: &str) -> Value {
    serde_json::json!({
        "role": "assistant",
        "content": [{ "type": "tool_use", "id": "c1", "name": "Read", "input": { "file_path": path } }]
    })
}

fn tool_result(is_error: bool) -> Value {
    serde_json::json!({
        "role": "user",
        "content": [{ "type": "tool_result", "tool_use_id": "c1", "text": "x", "is_error": is_error }]
    })
}

fn step(run: &str, step: &str, cause: &str, messages: usize, tokens: u64) -> SessionStepMeta {
    SessionStepMeta {
        run_id: run.to_string(),
        step_id: step.to_string(),
        finalize_cause: Some(cause.to_string()),
        message_count: messages,
        api_input_tokens: tokens,
    }
}

#[test]
fn jaccard_is_one_for_identical_and_zero_for_disjoint_or_empty() {
    assert!((jaccard("请实现登录功能", "请实现登录功能") - 1.0).abs() < f64::EPSILON);
    assert_eq!(jaccard("请实现登录功能", "另外看看部署"), 0.0);
    assert_eq!(jaccard("", "任意"), 0.0);
}

#[test]
fn user_text_skips_tool_result_only_messages() {
    assert_eq!(user_text(&user("你好")).as_deref(), Some("你好"));
    assert!(user_text(&tool_result(false)).is_none());
    assert!(user_text(&assistant_read("a.rs")).is_none());
}

#[test]
fn uncompressed_session_yields_no_feedback() {
    let session = SessionSnapshot {
        session_id: "s1".to_string(),
        messages: vec![user("hi")],
        ..Default::default()
    };
    assert!(evaluate(&session).is_none());
}

#[test]
fn requery_repeated_reads_errors_and_interruptions_are_counted() {
    let session = SessionSnapshot {
        session_id: "s1".to_string(),
        compact_start: Some(("run_b".to_string(), "stp_b".to_string())),
        compacted_message_count: 3,
        messages: vec![
            user("请实现登录功能并跑测试"),
            assistant_read("agent/features/auth.rs"),
            tool_result(true),
            // tail：重复请求 + 重复读取 + 一次成功工具结果 + 不满措辞
            user("请实现登录功能并跑测试"),
            assistant_read("agent/features/auth.rs"),
            tool_result(false),
            user("我不是说了要跑测试吗"),
        ],
        steps: vec![
            step("run_a", "stp_a", "Completed", 3, 100),
            step("run_b", "stp_b", "Cancelled", 4, 200),
        ],
        ..Default::default()
    };
    let feedback = evaluate(&session).expect("compressed session");
    assert_eq!(feedback.compacted_messages, 3);
    assert_eq!(feedback.requery_candidates, 2);
    assert_eq!(feedback.requery_hits, 1);
    assert_eq!(feedback.requery_rate, Some(0.5));
    assert_eq!(feedback.repeated_read_paths, 1);
    assert_eq!(feedback.tool_calls_early, 1);
    assert_eq!(feedback.tool_errors_early, 1);
    assert_eq!(feedback.tool_calls_tail, 1);
    assert_eq!(feedback.tool_errors_tail, 0);
    assert_eq!(feedback.interrupted_steps_tail, 1);
    assert_eq!(feedback.dissatisfaction_signals, 1);
    assert_eq!(feedback.api_input_tokens_early, 100);
    assert_eq!(feedback.api_input_tokens_tail, 200);
}

#[test]
fn summary_uses_weighted_error_rates_and_ignores_empty_denominators() {
    let items = vec![
        SessionFeedback {
            requery_rate: None,
            tool_calls_early: 2,
            tool_errors_early: 1,
            tool_calls_tail: 4,
            tool_errors_tail: 1,
            ..Default::default()
        },
        SessionFeedback {
            requery_rate: Some(0.5),
            ..Default::default()
        },
    ];
    let report = summarize(&items, 3);
    assert_eq!(report.sessions_considered, 3);
    assert_eq!(report.sessions_with_compaction, 2);
    assert_eq!(report.requery_rate_mean, Some(0.5));
    assert_eq!(report.error_rate_early, Some(0.5));
    assert_eq!(report.error_rate_tail, Some(0.25));
}
