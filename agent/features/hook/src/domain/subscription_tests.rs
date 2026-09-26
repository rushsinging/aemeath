//! `HookSubscription` 配置合法性校验测试（设计 §4）。
//!
//! 独立测试文件，遵循仓库 `*_tests.rs` 约定；不含运行时执行行为。

#![cfg(test)]

use super::*;
use crate::domain::invocation::HookPointData;

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
        if point == HookPointData::Stop {
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
        HookPointData::PreToolUse,
        HookPointData::UserPromptSubmit,
        HookPointData::PreCompact,
        HookPointData::PermissionRequest,
        HookPointData::Elicitation,
        HookPointData::UserPromptExpansion,
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
        let sub = HookSubscription::new(HookPointData::Stop, "cmd").with_failure_policy(policy);
        assert!(
            matches!(
                sub.validate(),
                Err(SubscriptionError::FailurePolicyOnStop {
                    point: HookPointData::Stop
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
        HookPointData::PostToolUse,
        HookPointData::SessionStart,
        HookPointData::Notification,
        HookPointData::StopFailure,
        HookPointData::PermissionDenied,
        HookPointData::TeammateIdle,
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

/// 返回全部 26 个 HookPointData（用于参数化校验）。
fn all_points() -> Vec<HookPointData> {
    vec![
        HookPointData::PreToolUse,
        HookPointData::UserPromptSubmit,
        HookPointData::PreCompact,
        HookPointData::PermissionRequest,
        HookPointData::Elicitation,
        HookPointData::UserPromptExpansion,
        HookPointData::Stop,
        HookPointData::PostToolUse,
        HookPointData::PostToolUseFailure,
        HookPointData::PostCompact,
        HookPointData::PostToolBatch,
        HookPointData::ElicitationResult,
        HookPointData::SessionStart,
        HookPointData::SessionEnd,
        HookPointData::SubRunStart,
        HookPointData::SubRunStop,
        HookPointData::TaskCreated,
        HookPointData::TaskCompleted,
        HookPointData::Notification,
        HookPointData::InstructionsLoaded,
        HookPointData::StopFailure,
        HookPointData::PermissionDenied,
        HookPointData::ConfigChange,
        HookPointData::CwdChanged,
        HookPointData::FileChanged,
        HookPointData::TeammateIdle,
    ]
}
