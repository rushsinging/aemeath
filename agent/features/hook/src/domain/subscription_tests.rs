//! `HookSubscription` 配置合法性校验测试（设计 §4）。
//!
//! 独立测试文件，遵循仓库 `*_tests.rs` 约定；不含运行时执行行为。

#![cfg(test)]

use super::*;
use crate::domain::invocation::HookPoint;

// ════════════════════════════════════════════════════════════
// 配置合法性校验（设计 §4）
// ════════════════════════════════════════════════════════════

#[test]
fn validate_accepts_no_failure_policy_on_any_point() {
    for point in all_points() {
        let sub = HookSubscription::new(point, "cmd");
        assert!(
            sub.validate().is_ok(),
            "{point:?}: 无 failure_policy 应始终合法"
        );
    }
}

#[test]
fn validate_accepts_continue_policy_on_non_stop_points() {
    for point in all_points() {
        // Stop 固定 Block 语义，禁止任何 failure_policy（由下一用例覆盖）。
        if point == HookPoint::Stop {
            continue;
        }
        let sub =
            HookSubscription::new(point, "cmd").with_failure_policy(HookFailurePolicy::Continue);
        assert!(
            sub.validate().is_ok(),
            "{point:?}: failure_policy=Continue 应合法（显式声明默认行为）"
        );
    }
}

#[test]
fn validate_accepts_block_policy_on_configurable_points() {
    for point in [
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PreCompact,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
    ] {
        let sub = HookSubscription::new(point, "cmd").with_failure_policy(HookFailurePolicy::Block);
        assert!(
            sub.validate().is_ok(),
            "{point:?}: failure_policy_configurable=true 应允许 Block"
        );
    }
}

#[test]
fn validate_rejects_failure_policy_on_stop() {
    for policy in [HookFailurePolicy::Continue, HookFailurePolicy::Block] {
        let sub = HookSubscription::new(HookPoint::Stop, "cmd").with_failure_policy(policy);
        assert!(
            matches!(
                sub.validate(),
                Err(SubscriptionError::FailurePolicyOnStop {
                    point: HookPoint::Stop
                })
            ),
            "Stop 固定 Block 语义，禁止任何 failure_policy（测试 {policy:?}）"
        );
    }
}

#[test]
fn validate_rejects_block_policy_on_non_configurable_points() {
    // Stop 由上一用例覆盖（FailurePolicyOnStop）；这里覆盖非前置闸门。
    for point in [
        HookPoint::PostToolUse,
        HookPoint::SessionStart,
        HookPoint::Notification,
        HookPoint::StopFailure,
        HookPoint::PermissionDenied,
        HookPoint::TeammateIdle,
    ] {
        let sub = HookSubscription::new(point, "cmd").with_failure_policy(HookFailurePolicy::Block);
        assert!(
            matches!(
                sub.validate(),
                Err(SubscriptionError::BlockPolicyOnNonConfigurablePoint { .. })
            ),
            "{point:?}: failure_policy_configurable=false 不应允许 Block"
        );
    }
}

/// 返回全部 26 个 HookPoint（用于参数化校验）。
fn all_points() -> Vec<HookPoint> {
    vec![
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PreCompact,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
        HookPoint::Stop,
        HookPoint::PostToolUse,
        HookPoint::PostToolUseFailure,
        HookPoint::PostCompact,
        HookPoint::PostToolBatch,
        HookPoint::ElicitationResult,
        HookPoint::SessionStart,
        HookPoint::SessionEnd,
        HookPoint::SubRunStart,
        HookPoint::SubRunStop,
        HookPoint::TaskCreated,
        HookPoint::TaskCompleted,
        HookPoint::Notification,
        HookPoint::InstructionsLoaded,
        HookPoint::StopFailure,
        HookPoint::PermissionDenied,
        HookPoint::ConfigChange,
        HookPoint::CwdChanged,
        HookPoint::FileChanged,
        HookPoint::TeammateIdle,
    ]
}
