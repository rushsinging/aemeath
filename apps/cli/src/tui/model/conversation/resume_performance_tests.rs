//! Resume 场景性能回归测试。
//!
//! 复现 issue #1467：长会话 resume 时逐条 apply 约 1.4 万个 tool call，
//! 每个触发 3~5 次对 timeline（≈30000 items）的 O(n) 全表扫描。
//! 基线（修复前）：release 下 > 60s；目标（修复后）：< 3s。

use std::time::{Duration, Instant};

use super::intent::ConversationIntent;
use super::model::ConversationModel;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiResumedSessionStep,
    TuiResumedStepFinalizeCause,
};

/// 构造与真实长会话等价的 resume workload：
/// `step_count` 个 step，每 step 一个含 `tools_per_step` 个 ToolUse 的
/// assistant 消息 + 一个含同数量 ToolResult 的 user 消息 + Completed 终态。
/// timeline items ≈ tools_per_step * 2 * step_count + step_count。
fn build_resume_workload(step_count: usize, tools_per_step: usize) -> Vec<TuiResumedSessionStep> {
    (0..step_count)
        .map(|step| TuiResumedSessionStep {
            run_id: format!("run-{step}"),
            step_id: format!("step-{step}"),
            messages: {
                let assistant = TuiChatMessage {
                    role: "assistant".to_string(),
                    content: (0..tools_per_step)
                        .map(|t| TuiContentBlock::ToolUse {
                            id: format!("tool-{step}-{t}"),
                            name: "Bash".to_string(),
                            input: serde_json::json!({ "command": "true" }),
                        })
                        .collect(),
                    source: TuiMessageSource::User,
                    stop_hook: None,
                    skill_request: None,
                    input_id: None,
                };
                let result = TuiChatMessage {
                    role: "user".to_string(),
                    content: (0..tools_per_step)
                        .map(|t| TuiContentBlock::ToolResult {
                            tool_use_id: format!("tool-{step}-{t}"),
                            content: serde_json::json!({ "stdout": "ok" }),
                            is_error: false,
                            text: Some("ok".to_string()),
                        })
                        .collect(),
                    source: TuiMessageSource::User,
                    stop_hook: None,
                    skill_request: None,
                    input_id: None,
                };
                vec![assistant, result]
            },
            finalize_cause: Some(TuiResumedStepFinalizeCause::Completed),
            duration_ms: Some(1000),
        })
        .collect()
}

#[test]
#[ignore = "性能回归；手动运行：cargo test -p cli --release resume_performance -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
fn resume_performance_large_session() {
    // 2000 steps × 7 tools → 14000 calls + 14000 results + 2000 System ≈ 30000 items，
    // 等价于 issue #1467 的 29462 items 现场。
    const STEPS: usize = 2000;
    const TOOLS_PER_STEP: usize = 7;
    let steps = build_resume_workload(STEPS, TOOLS_PER_STEP);
    let mut model = ConversationModel::default();

    let started = Instant::now();
    let changes = model.apply(ConversationIntent::ResumeConversation(
        super::intent::ResumeConversation { steps },
    ));
    let elapsed = started.elapsed();

    println!(
        "resume_performance: steps={STEPS} tools={TOOLS_PER_STEP} timeline_items={} changes={} elapsed={:.2}s",
        model.timeline.items().len(),
        changes.len(),
        elapsed.as_secs_f64()
    );
    assert_eq!(
        model.timeline.items().len(),
        STEPS * TOOLS_PER_STEP * 2 + STEPS,
        "timeline 规模必须与 workload 一致"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "resume 耗时 {elapsed:?} 超过 3s 目标（修复前基线 > 60s）"
    );
}

#[test]
fn resume_workload_timeline_stays_bounded_in_debug() {
    // debug 模式不卡阈值，只验证规模与顺序正确性（顺序断言见 Task 2 一致性测试）。
    let steps = build_resume_workload(10, 3);
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::ResumeConversation(
        super::intent::ResumeConversation { steps },
    ));
    assert_eq!(model.timeline.items().len(), 10 * 3 * 2 + 10);
}
