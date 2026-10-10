//! ScoringEvent 序列化与题型快照行为测试。

use super::*;
use crate::constants::EVENT_SCHEMA_VERSION;
use crate::domain::published_language::NoulCriteria;

fn sample_noul_question() -> ScoringQuestion {
    ScoringQuestion::noul(
        "判断以下任务是否已经完成。",
        Some(
            NoulCriteria::new("任务目标已达成且验证通过。", "任务目标未达成或验证失败。")
                .expect("完整句子应构造成功"),
        ),
    )
    .expect("noul 应构造成功")
}

fn sample_choice_question() -> ScoringQuestion {
    ScoringQuestion::choice(
        "哪个候选与问题最相关？",
        vec![
            (
                "log_router".to_string(),
                "log_router：负责按级别路由结构化日志到下游 sink。".to_string(),
            ),
            (
                "log_filter".to_string(),
                "log_filter：按正则表达式过滤日志条目后再进入队列。".to_string(),
            ),
        ],
    )
    .expect("choice 应构造成功")
}

fn sample_score_question() -> ScoringQuestion {
    ScoringQuestion::score(
        "该回复的帮助程度如何？",
        vec![
            "完全无帮助：未解决任何问题。".to_string(),
            "部分有帮助：推进了排查但未给出结论。".to_string(),
            "完全有帮助：直接给出可执行的修复步骤。".to_string(),
        ],
    )
    .expect("score 应构造成功")
}

fn sample_event() -> ScoringEvent {
    let noul_answer =
        ScoringAnswer::noul(0.73, CalibrationLevel::Temperature).expect("noul 答案应构造成功");
    let choice_answer = ScoringAnswer::choice(
        "log_router",
        vec![
            ("log_filter".to_string(), 0.4),
            ("log_router".to_string(), 0.6),
        ],
        0.61,
        CalibrationLevel::Raw,
    )
    .expect("choice 答案应构造成功");
    let score_answer = ScoringAnswer::score(2.0, vec![0.1, 0.2, 0.7], 0.72, CalibrationLevel::Head)
        .expect("score 答案应构造成功");

    ScoringEvent {
        schema_version: ScoringEvent::SCHEMA_VERSION,
        event_id: ScoringEvent::generate_event_id(),
        ts_unix_ms: 1_791_672_240_000,
        timestamp: "2026-10-10T22:44:00+08:00".to_string(),
        scenario: "memory_rerank".to_string(),
        engine_revision: "kev-r3".to_string(),
        prompt_sha256: "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b"
            .to_string(),
        question_count: 3,
        latency_ms: 42,
        outcome: "ok".to_string(),
        unavailable_kind: None,
        state_text: "用户在排查日志路由问题。".to_string(),
        questions: vec![
            ScoringQuestionSnapshot::from_question(&sample_noul_question()),
            ScoringQuestionSnapshot::from_question(&sample_choice_question()),
            ScoringQuestionSnapshot::from_question(&sample_score_question()),
        ],
        answers: vec![
            ScoringAnswerSnapshot::from_answer(&noul_answer),
            ScoringAnswerSnapshot::from_answer(&choice_answer),
            ScoringAnswerSnapshot::from_answer(&score_answer),
        ],
        ranking: Some(ScoringRankingSnapshot {
            before: vec!["log_filter".to_string(), "log_router".to_string()],
            after: vec!["log_router".to_string(), "log_filter".to_string()],
        }),
        // PR1 关联字段全缺省（序列化为 null，不 skip）。
        correlation_id: None,
        session_id: None,
        run_ordinal: None,
        step_ordinal: None,
        tool_call_id: None,
    }
}

#[test]
fn event_schema_version_constant_is_one() {
    assert_eq!(EVENT_SCHEMA_VERSION, 1);
    assert_eq!(ScoringEvent::SCHEMA_VERSION, 1);
}

#[test]
fn scoring_event_serde_round_trip_preserves_every_field() {
    let event = sample_event();
    let json = serde_json::to_value(&event).expect("事件应序列化成功");
    let restored: ScoringEvent = serde_json::from_value(json).expect("事件应反序列化成功");
    assert_eq!(restored, event);
}

#[test]
fn scoring_event_serializes_all_envelope_fields_as_null_when_absent() {
    let mut event = sample_event();
    event.unavailable_kind = None;
    event.ranking = None;
    let json = serde_json::to_value(&event).expect("事件应序列化成功");
    let object = json.as_object().expect("事件应序列化为对象");

    for field_name in [
        "unavailable_kind",
        "ranking",
        "correlation_id",
        "session_id",
        "run_ordinal",
        "step_ordinal",
        "tool_call_id",
    ] {
        let field = object
            .get(field_name)
            .unwrap_or_else(|| panic!("字段 {field_name} 应存在而非被 skip"));
        assert!(
            field.is_null(),
            "字段 {field_name} 缺省时应为 null，实际 {field}"
        );
    }

    let restored: ScoringEvent = serde_json::from_value(json).expect("事件应反序列化成功");
    assert_eq!(restored.correlation_id, None);
    assert_eq!(restored.ranking, None);
}

#[test]
fn scoring_event_with_unavailable_outcome_round_trips_kind() {
    let mut event = sample_event();
    event.outcome = "unavailable".to_string();
    event.unavailable_kind = Some("model_missing".to_string());
    event.ranking = None;
    let json = serde_json::to_value(&event).expect("事件应序列化成功");
    let restored: ScoringEvent = serde_json::from_value(json).expect("事件应反序列化成功");
    assert_eq!(restored.outcome, "unavailable");
    assert_eq!(restored.unavailable_kind.as_deref(), Some("model_missing"));
    assert_eq!(restored, event);
}

#[test]
fn noul_question_snapshot_preserves_instructions_and_criteria_text() {
    let snapshot = ScoringQuestionSnapshot::from_question(&sample_noul_question());
    assert_eq!(snapshot.kind, "noul");
    assert_eq!(snapshot.instructions, "判断以下任务是否已经完成。");
    assert_eq!(snapshot.criteria.len(), 2);
    assert_eq!(snapshot.criteria[0].key, None);
    assert_eq!(snapshot.criteria[0].text, "任务目标已达成且验证通过。");
    assert_eq!(snapshot.criteria[1].key, None);
    assert_eq!(snapshot.criteria[1].text, "任务目标未达成或验证失败。");
}

#[test]
fn noul_question_snapshot_without_criteria_has_empty_criteria() {
    let question = ScoringQuestion::noul("判断任务是否完成。", None).expect("noul 应构造成功");
    let snapshot = ScoringQuestionSnapshot::from_question(&question);
    assert_eq!(snapshot.kind, "noul");
    assert!(snapshot.criteria.is_empty());
}

#[test]
fn choice_question_snapshot_preserves_every_option_in_full() {
    let snapshot = ScoringQuestionSnapshot::from_question(&sample_choice_question());
    assert_eq!(snapshot.kind, "choice");
    assert_eq!(snapshot.criteria.len(), 2);
    assert_eq!(snapshot.criteria[0].key.as_deref(), Some("log_router"));
    assert_eq!(
        snapshot.criteria[0].text,
        "log_router：负责按级别路由结构化日志到下游 sink。"
    );
    assert_eq!(snapshot.criteria[1].key.as_deref(), Some("log_filter"));
    assert_eq!(
        snapshot.criteria[1].text,
        "log_filter：按正则表达式过滤日志条目后再进入队列。"
    );
}

#[test]
fn score_question_snapshot_preserves_level_text_in_order() {
    let snapshot = ScoringQuestionSnapshot::from_question(&sample_score_question());
    assert_eq!(snapshot.kind, "score");
    let level_texts: Vec<&str> = snapshot
        .criteria
        .iter()
        .map(|criterion| criterion.text.as_str())
        .collect();
    assert_eq!(
        level_texts,
        vec![
            "完全无帮助：未解决任何问题。",
            "部分有帮助：推进了排查但未给出结论。",
            "完全有帮助：直接给出可执行的修复步骤。",
        ]
    );
    assert!(snapshot
        .criteria
        .iter()
        .all(|criterion| criterion.key.is_none()));
}

#[test]
fn answer_snapshots_map_probabilities_and_calibration_per_kind() {
    let noul_answer =
        ScoringAnswer::noul(0.73, CalibrationLevel::Temperature).expect("noul 答案应构造成功");
    let noul_snapshot = ScoringAnswerSnapshot::from_answer(&noul_answer);
    assert_eq!(noul_snapshot.probabilities, vec![0.73]);
    assert_eq!(
        noul_snapshot.calibration,
        Some(CalibrationLevel::Temperature)
    );

    let choice_answer = ScoringAnswer::choice(
        "log_router",
        vec![
            ("log_filter".to_string(), 0.4),
            ("log_router".to_string(), 0.6),
        ],
        0.61,
        CalibrationLevel::Raw,
    )
    .expect("choice 答案应构造成功");
    let choice_snapshot = ScoringAnswerSnapshot::from_answer(&choice_answer);
    assert_eq!(choice_snapshot.probabilities, vec![0.4, 0.6]);
    assert_eq!(choice_snapshot.calibration, Some(CalibrationLevel::Raw));

    let score_answer = ScoringAnswer::score(2.0, vec![0.1, 0.2, 0.7], 0.72, CalibrationLevel::Head)
        .expect("score 答案应构造成功");
    let score_snapshot = ScoringAnswerSnapshot::from_answer(&score_answer);
    assert_eq!(score_snapshot.probabilities, vec![0.1, 0.2, 0.7]);
    assert_eq!(score_snapshot.calibration, Some(CalibrationLevel::Head));
}

#[test]
fn ranking_snapshot_keeps_before_and_after_key_order() {
    let ranking = ScoringRankingSnapshot {
        before: vec!["beta".to_string(), "alpha".to_string()],
        after: vec!["alpha".to_string(), "beta".to_string()],
    };
    let json = serde_json::to_value(&ranking).expect("排序快照应序列化成功");
    assert_eq!(json["before"], serde_json::json!(["beta", "alpha"]));
    assert_eq!(json["after"], serde_json::json!(["alpha", "beta"]));
}

#[test]
fn event_ids_are_unique_and_non_empty() {
    let first_id = ScoringEvent::generate_event_id();
    let second_id = ScoringEvent::generate_event_id();
    assert!(!first_id.is_empty());
    assert_ne!(first_id, second_id);
}

#[test]
fn call_context_slot_snapshot_reflects_set_and_clear() {
    let slot = ScoringCallContextSlot::new();
    let source = slot.snapshot_source();
    assert_eq!(source(), ScoringCallContext::default());

    slot.set(ScoringCallContext {
        correlation_id: Some("c1".to_string()),
        session_id: Some("s1".to_string()),
        run_ordinal: Some(1),
        step_ordinal: Some(2),
        tool_call_id: Some("t1".to_string()),
    });
    let snapshot = source();
    assert_eq!(snapshot.correlation_id.as_deref(), Some("c1"));
    assert_eq!(snapshot.session_id.as_deref(), Some("s1"));
    assert_eq!(snapshot.run_ordinal, Some(1));
    assert_eq!(snapshot.step_ordinal, Some(2));
    assert_eq!(snapshot.tool_call_id.as_deref(), Some("t1"));

    slot.clear();
    assert_eq!(source(), ScoringCallContext::default());
}
