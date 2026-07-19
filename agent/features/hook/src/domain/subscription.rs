//! Hook 用户配置订阅（设计 §4）。
//!
//! 承载配置数据 + 配置合法性校验（`HookSubscription::validate`）。
//! 匹配 / 排序 / 聚合 / 重试语义由 dispatcher adapter（#924）实现，
//! 本模块不含任何运行时执行行为。

use std::time::Duration;

use crate::domain::invocation::HookPoint;

// ─── HookSubscription ─────────────────────────────────────────

/// Hook 用户配置订阅。
///
/// 配置按 `order` + 声明顺序稳定执行；只有 metadata 允许时才能配置
/// `failure_policy=Block`（校验在 Config 阶段，由 #925/#926 承接）。
#[derive(Debug, Clone)]
pub struct HookSubscription {
    /// 触发点。
    pub point: HookPoint,
    /// 匹配器。
    pub matcher: HookMatcher,
    /// 执行命令。
    pub command: HookCommand,
    /// 单次执行超时。
    pub timeout: Duration,
    /// 失败策略（普通 Hook 可配置；Stop 固定 Block）。
    pub failure_policy: Option<HookFailurePolicy>,
    /// 排序键（小者先执行；相同则按声明顺序）。
    pub order: i32,
    /// 是否启用。
    pub enabled: bool,
}

impl HookSubscription {
    /// 创建一个默认订阅（`All` matcher、order 0、无 failure_policy、60s 超时、enabled）。
    pub fn new(point: HookPoint, command: impl Into<String>) -> Self {
        Self {
            point,
            matcher: HookMatcher::All,
            command: HookCommand::new(command),
            timeout: Duration::from_secs(60),
            failure_policy: None,
            order: 0,
            enabled: true,
        }
    }

    /// 设置匹配器。
    pub fn with_matcher(mut self, matcher: HookMatcher) -> Self {
        self.matcher = matcher;
        self
    }

    /// 设置排序键。
    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// 设置失败策略。
    pub fn with_failure_policy(mut self, policy: HookFailurePolicy) -> Self {
        self.failure_policy = Some(policy);
        self
    }

    /// 校验本订阅的配置合法性（设计 §4）。
    ///
    /// 规则：
    /// - Stop point **禁止**任何 failure_policy（其语义固定为 Block，不可由用户覆盖）；
    /// - `failure_policy=Block` 仅允许出现在 `failure_policy_configurable=true` 的 point
    ///   （即前置闸门 PreToolUse / UserPromptSubmit / PreCompact / PermissionRequest /
    ///   Elicitation / UserPromptExpansion）；
    /// - `failure_policy=Continue` 可出现在任何 point（显式声明默认行为，合法）。
    ///
    /// 非法组合在 Config 校验阶段拒绝；Dispatcher 在构造时也会严格拒绝
    /// （`try_new` 返回全部错误而非静默过滤），保证运行时不变量。
    pub fn validate(&self) -> Result<(), SubscriptionError> {
        let Some(policy) = self.failure_policy else {
            return Ok(());
        };
        // Stop 固定 Block，禁止用户配置任何 failure_policy。
        if self.point == HookPoint::Stop {
            return Err(SubscriptionError::FailurePolicyOnStop { point: self.point });
        }
        // Block 策略仅允许出现在 failure_policy_configurable=true 的 point。
        if policy == HookFailurePolicy::Block && !self.point.metadata().failure_policy_configurable
        {
            return Err(SubscriptionError::BlockPolicyOnNonConfigurablePoint { point: self.point });
        }
        Ok(())
    }
}

/// 订阅配置校验错误。
///
/// 对应设计 §4「非法组合在 Config 校验阶段拒绝，而非运行时静默忽略」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionError {
    /// Stop point 配置了 failure_policy（其语义固定为 Block，用户不可覆盖）。
    FailurePolicyOnStop {
        /// 违规的 point。
        point: HookPoint,
    },
    /// 在不支持配置 Block 策略的 point（非前置闸门）上声明了 `failure_policy=Block`。
    BlockPolicyOnNonConfigurablePoint {
        /// 违规的 point。
        point: HookPoint,
    },
}

// ─── HookMatcher ──────────────────────────────────────────────

/// Hook 匹配器。
///
/// 匹配语义（invocation → 是否命中）由 dispatcher 依据本类型实现；
/// 本类型仅承载配置数据。空 / `All` 匹配所有；`ToolName` 精确匹配
/// PreToolUse / PostToolUse / PermissionRequest 等带工具名的 point。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookMatcher {
    /// 匹配全部（默认）。
    All,
    /// 工具名精确匹配。
    ToolName(String),
}

// ─── HookCommand ──────────────────────────────────────────────

/// Hook 命令（shell 字符串）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommand {
    /// shell 命令字符串。
    pub command: String,
}

impl HookCommand {
    /// 创建命令。
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }
}

// ─── HookFailurePolicy ────────────────────────────────────────

/// Hook 失败策略（设计 §4）。
///
/// 普通 Hook 未配置时默认 Continue；Stop 固定 Block（不可改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookFailurePolicy {
    /// 执行失败重试耗尽后继续。
    Continue,
    /// 执行失败重试耗尽后阻断。
    Block,
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
