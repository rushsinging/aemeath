use share::config::domain::snapshot::ConfigSnapshot;
use share::config::hooks::{HookEvent, HooksConfig};

use crate::adapters::dispatcher::Dispatcher;
use crate::domain::{HookMatcherData, HookPointData, HookSubscription};

/// 将 Claude Code 兼容的扁平 HooksConfig 转为 Hook BC 所有的订阅语言。
///
/// 每个事件数组的声明顺序映射到该触发点内稳定的 `order`；空 matcher 归一为
/// `All`，非空 matcher 归一为 `ToolName`。兼容配置未携带 failure policy，故由
/// Hook BC 保持默认的 `None` / Continue 语义。
pub fn subscriptions_from_config(config: &HooksConfig) -> Vec<HookSubscription> {
    let mut events = config.events.iter().collect::<Vec<_>>();
    events.sort_by_key(|(event, _)| hook_point_from_event(**event) as u8);

    events
        .into_iter()
        .flat_map(|(event, entries)| {
            let point = hook_point_from_event(*event);
            entries.iter().enumerate().map(move |(order, entry)| {
                let matcher = if entry.matcher.is_empty() {
                    HookMatcherData::All
                } else {
                    HookMatcherData::ToolName(entry.matcher.clone())
                };
                let mut subscription = HookSubscription::new(point, entry.command.clone())
                    .with_matcher(matcher)
                    .with_order(order as i32);
                subscription.timeout = std::time::Duration::from_secs(entry.timeout);
                subscription
            })
        })
        .collect()
}

/// 构造 Hook BC 唯一的生产 Dispatcher。
pub fn wire_hook_dispatcher(
    config: &ConfigSnapshot,
) -> Result<std::sync::Arc<dyn crate::ports::HookDispatcher>, share::error::DomainError> {
    let subscriptions = subscriptions_from_config(config.hooks());
    let policy = config.hook_execution_policy();
    log::debug!(
        target: crate::LOG_TARGET,
        "hook dispatcher built: configured_events={} subscriptions={} max_attempts={} env_passthrough={:?}",
        config.hooks().events.len(),
        subscriptions.len(),
        policy.max_attempts(),
        config.hooks().env_passthrough,
    );
    Dispatcher::try_new(
        subscriptions,
        policy,
        config.hooks().env_passthrough.clone(),
    )
    .map(|dispatcher| {
        std::sync::Arc::new(dispatcher) as std::sync::Arc<dyn crate::ports::HookDispatcher>
    })
    .map_err(|errors| {
        share::error::DomainError::invalid(
            "hook",
            format!(
                "hook 订阅配置非法：{}",
                errors
                    .into_iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        )
    })
}

fn hook_point_from_event(event: HookEvent) -> HookPointData {
    match event {
        HookEvent::PreToolUse => HookPointData::PreToolUse,
        HookEvent::PostToolUse => HookPointData::PostToolUse,
        HookEvent::PostToolUseFailure => HookPointData::PostToolUseFailure,
        HookEvent::UserPromptSubmit => HookPointData::UserPromptSubmit,
        HookEvent::Stop => HookPointData::Stop,
        HookEvent::StopFailure => HookPointData::StopFailure,
        HookEvent::SessionStart => HookPointData::SessionStart,
        HookEvent::SessionEnd => HookPointData::SessionEnd,
        HookEvent::PreCompact => HookPointData::PreCompact,
        HookEvent::PostCompact => HookPointData::PostCompact,
        HookEvent::PostToolBatch => HookPointData::PostToolBatch,
        HookEvent::SubagentStart => HookPointData::SubRunStart,
        HookEvent::SubagentStop => HookPointData::SubRunStop,
        HookEvent::TaskCreated => HookPointData::TaskCreated,
        HookEvent::TaskCompleted => HookPointData::TaskCompleted,
        HookEvent::PermissionRequest => HookPointData::PermissionRequest,
        HookEvent::PermissionDenied => HookPointData::PermissionDenied,
        HookEvent::Notification => HookPointData::Notification,
        HookEvent::InstructionsLoaded => HookPointData::InstructionsLoaded,
        HookEvent::ConfigChange => HookPointData::ConfigChange,
        HookEvent::Elicitation => HookPointData::Elicitation,
        HookEvent::ElicitationResult => HookPointData::ElicitationResult,
        HookEvent::UserPromptExpansion => HookPointData::UserPromptExpansion,
        HookEvent::CwdChanged => HookPointData::CwdChanged,
        HookEvent::FileChanged => HookPointData::FileChanged,
        HookEvent::TeammateIdle => HookPointData::TeammateIdle,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
