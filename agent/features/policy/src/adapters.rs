use crate::{PolicyDecision, PolicyPort, PolicyRequest};

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllPolicy;

impl PolicyPort for AllowAllPolicy {
    fn evaluate(&self, _request: &PolicyRequest) -> PolicyDecision {
        PolicyDecision::Allow
    }
}
