use crate::{Policy, PolicyDecisionData, PolicyModeData, PolicyRequestData};
use share::tools_vocab::AuthorizationContext;

pub(crate) struct ConfiguredPolicy<ModeFn> {
    mode: ModeFn,
}

impl<ModeFn> ConfiguredPolicy<ModeFn> {
    pub(crate) fn new(mode: ModeFn) -> Self {
        Self { mode }
    }
}

impl<ModeFn> Policy for ConfiguredPolicy<ModeFn>
where
    ModeFn: Fn() -> PolicyModeData + Send + Sync,
{
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        evaluate((self.mode)(), request)
    }

    fn current_mode(&self) -> PolicyModeData {
        (self.mode)()
    }
}

// 标准模式已由 `configured(|| PolicyModeData::Standard)` 生产路径供给；
// 本结构仅测试直接引用，收窄到 test 目标（#1995）。
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StandardPolicy;

#[cfg(test)]
impl Policy for StandardPolicy {
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        evaluate(PolicyModeData::Standard, request)
    }

    fn current_mode(&self) -> PolicyModeData {
        PolicyModeData::Standard
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AllowAllPolicy;

impl Policy for AllowAllPolicy {
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        evaluate(PolicyModeData::AllowAll, request)
    }

    fn current_mode(&self) -> PolicyModeData {
        PolicyModeData::AllowAll
    }
}

fn evaluate(mode: PolicyModeData, request: &PolicyRequestData) -> PolicyDecisionData {
    log::debug!(
        target: crate::LOG_TARGET,
        "policy evaluate entry: mode={mode:?} capability_count={}",
        request.required_capabilities().bits().count_ones(),
    );
    let authorization = match mode {
        PolicyModeData::Standard => AuthorizationContext::STANDARD,
        PolicyModeData::AllowAll => AuthorizationContext::ALLOW_ALL,
    };
    let decision = PolicyDecisionData::Allow(authorization);
    log::debug!(
        target: crate::LOG_TARGET,
        "policy evaluate exit: mode={mode:?} decision={decision:?}",
    );
    decision
}

#[cfg(test)]
mod adapters_tests;

/// 生产策略工厂：mode 由闭包动态供给（config reload 后跟随变化）。
pub fn configured<ModeFn>(mode: ModeFn) -> std::sync::Arc<dyn crate::domain::Policy>
where
    ModeFn: Fn() -> crate::domain::PolicyModeData + Send + Sync + 'static,
{
    std::sync::Arc::new(ConfiguredPolicy::new(mode))
}

/// 全允许策略工厂（测试/显式放行装配）。
pub fn allow_all() -> std::sync::Arc<dyn crate::domain::Policy> {
    std::sync::Arc::new(AllowAllPolicy)
}
