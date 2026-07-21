use std::collections::HashMap;

use share::config::hooks::{HookEvent, HooksConfig};

use crate::adapters::dispatcher::Dispatcher;
use crate::domain::{HookMatcher, HookPoint, HookSubscription, SubscriptionError};

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
                    HookMatcher::All
                } else {
                    HookMatcher::ToolName(entry.matcher.clone())
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
pub fn build_dispatcher(
    config: &HooksConfig,
    env: HashMap<String, String>,
) -> Result<Dispatcher, Vec<SubscriptionError>> {
    let subscriptions = subscriptions_from_config(config);
    log::debug!(
        target: crate::LOG_TARGET,
        "hook dispatcher built: configured_events={} subscriptions={}",
        config.events.len(),
        subscriptions.len(),
    );
    Dispatcher::try_new(subscriptions, env)
}

fn hook_point_from_event(event: HookEvent) -> HookPoint {
    match event {
        HookEvent::PreToolUse => HookPoint::PreToolUse,
        HookEvent::PostToolUse => HookPoint::PostToolUse,
        HookEvent::PostToolUseFailure => HookPoint::PostToolUseFailure,
        HookEvent::UserPromptSubmit => HookPoint::UserPromptSubmit,
        HookEvent::Stop => HookPoint::Stop,
        HookEvent::StopFailure => HookPoint::StopFailure,
        HookEvent::SessionStart => HookPoint::SessionStart,
        HookEvent::SessionEnd => HookPoint::SessionEnd,
        HookEvent::PreCompact => HookPoint::PreCompact,
        HookEvent::PostCompact => HookPoint::PostCompact,
        HookEvent::PostToolBatch => HookPoint::PostToolBatch,
        HookEvent::SubagentStart => HookPoint::SubRunStart,
        HookEvent::SubagentStop => HookPoint::SubRunStop,
        HookEvent::TaskCreated => HookPoint::TaskCreated,
        HookEvent::TaskCompleted => HookPoint::TaskCompleted,
        HookEvent::PermissionRequest => HookPoint::PermissionRequest,
        HookEvent::PermissionDenied => HookPoint::PermissionDenied,
        HookEvent::Notification => HookPoint::Notification,
        HookEvent::InstructionsLoaded => HookPoint::InstructionsLoaded,
        HookEvent::ConfigChange => HookPoint::ConfigChange,
        HookEvent::Elicitation => HookPoint::Elicitation,
        HookEvent::ElicitationResult => HookPoint::ElicitationResult,
        HookEvent::UserPromptExpansion => HookPoint::UserPromptExpansion,
        HookEvent::CwdChanged => HookPoint::CwdChanged,
        HookEvent::FileChanged => HookPoint::FileChanged,
        HookEvent::TeammateIdle => HookPoint::TeammateIdle,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
