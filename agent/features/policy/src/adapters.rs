use crate::{PolicyDecision, PolicyMode, PolicyModeSource, PolicyPort, PolicyRequest};
use tools::AuthorizationContext;

pub struct ConfiguredPolicy<S> {
    source: S,
}

impl<S> ConfiguredPolicy<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: PolicyModeSource> PolicyPort for ConfiguredPolicy<S> {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        evaluate(self.source.current_mode(), request)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StandardPolicy;

impl PolicyPort for StandardPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        evaluate(PolicyMode::Standard, request)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

impl PolicyPort for AllowAllPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        evaluate(PolicyMode::AllowAll, request)
    }
}

fn evaluate(mode: PolicyMode, request: &PolicyRequest) -> PolicyDecision {
    log::debug!(
        target: crate::LOG_TARGET,
        "policy evaluate entry: mode={mode:?} capability_count={}",
        request.required_capabilities().bits().count_ones(),
    );
    let authorization = match mode {
        PolicyMode::Standard => AuthorizationContext::STANDARD,
        PolicyMode::AllowAll => AuthorizationContext::ALLOW_ALL,
    };
    let decision = PolicyDecision::Allow(authorization);
    log::debug!(
        target: crate::LOG_TARGET,
        "policy evaluate exit: mode={mode:?} decision={decision:?}",
    );
    decision
}

#[cfg(test)]
mod adapters_tests;
