use crate::{Policy, PolicyDecisionData, PolicyModeData, PolicyModeReader, PolicyRequestData};
use tools::AuthorizationContext;

pub struct ConfiguredPolicy<S> {
    source: S,
}

impl<S> ConfiguredPolicy<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: PolicyModeReader> Policy for ConfiguredPolicy<S> {
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        evaluate(self.source.current_mode(), request)
    }

    fn current_mode(&self) -> PolicyModeData {
        self.source.current_mode()
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct StandardPolicy;

impl Policy for StandardPolicy {
    fn evaluate(&self, request: &PolicyRequestData) -> PolicyDecisionData {
        evaluate(PolicyModeData::Standard, request)
    }

    fn current_mode(&self) -> PolicyModeData {
        PolicyModeData::Standard
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

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
