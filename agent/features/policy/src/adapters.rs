use crate::{PolicyDecision, PolicyMode, PolicyPort, PolicyRequest};

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

impl PolicyPort for AllowAllPolicy {
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        let mode = PolicyMode::AllowAll;
        // Entry log — records only mode + capability count, never the tool
        // name or workspace path (privacy contract per logging spec).
        log::debug!(
            target: crate::LOG_TARGET,
            "policy evaluate entry: mode={mode:?} capability_count={}",
            request.required_capabilities().bits().count_ones(),
        );
        let decision = PolicyDecision::Allow;
        log::debug!(
            target: crate::LOG_TARGET,
            "policy evaluate exit: mode={mode:?} decision={decision:?}",
        );
        decision
    }
}

#[cfg(test)]
mod adapters_tests;
