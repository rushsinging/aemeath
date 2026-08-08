use std::collections::HashMap;

use provider::ReasoningLevel;
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;

use super::{
    context_decision, ContextRequest, ContextRequestId, DecisionReason, Language, SessionId,
    SystemPromptSpec,
};

fn request(last_api_total_tokens: Option<u64>) -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("pending")],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 1_000,
        max_output_tokens: 100,
        last_api_total_tokens,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[test]
fn provider_total_is_used_without_projected_delta() {
    let decision = context_decision::calculate(
        &request(Some(700)),
        &vec![Message::user("x".repeat(4_000))].into(),
        &[],
    );

    assert_eq!(decision.decision_token_count, 700);
    assert!(!decision.needed);
    assert_eq!(decision.reason, DecisionReason::ActualProviderUsage);
}

#[test]
fn provider_total_above_threshold_triggers_compaction() {
    let decision = context_decision::calculate(&request(Some(900)), &Vec::new().into(), &[]);

    assert!(decision.needed);
    assert_eq!(decision.reason, DecisionReason::ActualProviderUsage);
}

#[test]
fn custom_output_limit_changes_actual_usage_threshold() {
    let mut smaller_output_budget = request(Some(730));
    smaller_output_budget.max_output_tokens = 10;
    let smaller_output_decision =
        context_decision::calculate(&smaller_output_budget, &Vec::new().into(), &[]);

    let mut larger_output_budget = request(Some(730));
    larger_output_budget.max_output_tokens = 200;
    let larger_output_decision =
        context_decision::calculate(&larger_output_budget, &Vec::new().into(), &[]);

    assert!(!smaller_output_decision.needed);
    assert!(larger_output_decision.needed);
    assert_eq!(smaller_output_decision.threshold, 776);
    assert_eq!(larger_output_decision.threshold, 624);
}

#[test]
fn missing_provider_total_falls_back_to_complete_candidate_estimate() {
    let decision = context_decision::calculate(
        &request(None),
        &vec![Message::user("x".repeat(4_000))].into(),
        &[],
    );

    assert!(decision.needed);
    assert!(decision.decision_token_count > decision.threshold);
    assert_eq!(decision.reason, DecisionReason::HeuristicFallback);
}

/// 复现 #1500：session `019fc5e4` 实测 API input=119,524（44% of 272,000）
/// 却触发 compact——heuristic 估算（CJK×2 + 1.33x margin + JSON 2B/token）
/// 高达 ~216K，超过 threshold 200,141。校准后同一内容的估算应显著下降
/// 且低于 threshold，不再误触发。
#[test]
fn heuristic_estimate_with_realistic_mix_no_longer_triggers_at_43_percent() {
    use super::{SystemBlock, Urgency};

    let context_size = 272_000usize;
    let max_output_tokens = 16_384usize;

    // 与实测比例相近的内容组成：中文为主的 system prompt + 中文/英文混合
    // messages + JSON tool schemas。规模经核算：旧估算（CJK×2 + 1.33x margin
    // + JSON 2B/token）≈ 242K > threshold 200,141 必触发，新估算 ≈ 104K 不触发。
    let system_prompt = "你是一个 AI 编程助手，负责阅读代码、定位问题并实施修复。\
                         所有操作必须遵守仓库规范与安全约束。"
        .repeat(500); // ~22K CJK 字符
    let chinese_message = "请查看 src/runtime.rs 并分析 compact 进度条卡在 50% 的根因，\
                           然后给出修复方案并实施。"
        .repeat(1400); // ~53K CJK 字符
    let english_message =
        "the quick brown fox jumps over the lazy dog and inspects the token budget logic. "
            .repeat(1200); // ~100K ASCII
    let tool_schema_json = r#"{"name":"Bash","description":"run a shell command","parameters":{"type":"object","properties":{"command":{"type":"string"}}}}"#;
    let tool_schemas: Vec<serde_json::Value> = (0..80)
        .map(|_| serde_json::from_str(tool_schema_json).unwrap())
        .collect();

    let candidate = ContextRequest {
        context_size,
        max_output_tokens,
        last_api_total_tokens: None,
        tool_schema_tokens: super::estimate_tool_schemas_tokens(&tool_schemas),
        ..request(None)
    };
    let messages = vec![
        Message::user(chinese_message),
        Message::user(english_message),
    ]
    .into();
    let system_blocks = vec![SystemBlock {
        kind: "guidance".to_string(),
        content: system_prompt,
        cacheable: true,
        cache_break: false,
    }];

    let decision = context_decision::calculate(&candidate, &messages, &system_blocks);

    // 校准后估算不再超过 threshold（43% 实际用量场景不应触发 compact）
    assert_eq!(decision.reason, DecisionReason::HeuristicFallback);
    assert!(
        !decision.needed,
        "heuristic 估算 {} 仍超过 threshold {}，43% 场景误触发复现",
        decision.decision_token_count, decision.threshold,
    );
    assert!(
        decision.decision_token_count < decision.threshold,
        "估算 {} 应低于 threshold {}",
        decision.decision_token_count,
        decision.threshold,
    );
    // urgency 不应达到 Should/Must（80%+ 才需要压缩）
    assert_ne!(decision.urgency, Urgency::Should);
    assert_ne!(decision.urgency, Urgency::Must);
}
