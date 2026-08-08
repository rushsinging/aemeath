#[derive(Clone, Default)]
pub(crate) struct PublishedStateRegistry {
    inner: std::sync::Arc<parking_lot::Mutex<PublishedStateRegistryState>>,
}

#[derive(Default)]
struct PublishedStateRegistryState {
    status: Option<sdk::RuntimeStatusView>,
}

impl PublishedStateRegistry {
    pub(crate) fn update_context_budget(
        &self,
        session_id: impl Into<String>,
        decision: &context::domain::CompactionDecision,
    ) -> sdk::RuntimeStatusView {
        let session_id = session_id.into();
        let mut state = self.inner.lock();
        let next_revision = state
            .status
            .as_ref()
            .filter(|status| status.session_id == session_id)
            .map_or(1, |status| status.revision.saturating_add(1));
        let source = match decision.reason {
            context::domain::DecisionReason::ActualProviderUsage => {
                sdk::ContextDecisionSourceView::ActualProviderUsage
            }
            context::domain::DecisionReason::HeuristicFallback => {
                sdk::ContextDecisionSourceView::HeuristicFallback
            }
            context::domain::DecisionReason::Manual => sdk::ContextDecisionSourceView::Manual,
        };
        let status = sdk::RuntimeStatusView {
            session_id,
            revision: next_revision,
            heartbeat_sequence: 0,
            context_budget: sdk::ContextBudgetView {
                context_size: decision.context_size as u64,
                effective_window: decision.effective_window as u64,
                decision_token_count: decision.decision_token_count as u64,
                threshold: decision.threshold as u64,
                usage_permille: decision
                    .decision_token_count
                    .saturating_mul(1_000)
                    .checked_div(decision.effective_window.max(1))
                    .unwrap_or(0)
                    .min(1_000) as u32,
                compaction_needed: decision.needed,
                source,
            },
        };
        state.status = Some(status.clone());
        status
    }

    pub(crate) fn heartbeat(&self) -> Option<sdk::RuntimeStatusView> {
        let mut state = self.inner.lock();
        let status = state.status.as_mut()?;
        status.heartbeat_sequence = status.heartbeat_sequence.saturating_add(1);
        Some(status.clone())
    }

    #[cfg(test)]
    pub(crate) fn reset_session(&self, session_id: impl Into<String>) {
        let session_id = session_id.into();
        let mut state = self.inner.lock();
        state.status = state.status.take().map(|mut status| {
            status.session_id = session_id;
            status.revision = 0;
            status.heartbeat_sequence = 0;
            status
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision() -> context::domain::CompactionDecision {
        context::domain::CompactionDecision {
            needed: true,
            urgency: context::domain::Urgency::Should,
            decision_token_count: 145_000,
            threshold: 144_000,
            context_size: 200_000,
            effective_window: 180_000,
            reason: context::domain::DecisionReason::ActualProviderUsage,
        }
    }

    #[test]
    fn heartbeat_keeps_business_revision() {
        let registry = PublishedStateRegistry::default();
        let status = registry.update_context_budget("session-1", &decision());
        let heartbeat = registry.heartbeat().expect("status");
        assert_eq!(status.revision, heartbeat.revision);
        assert_eq!(heartbeat.heartbeat_sequence, 1);
    }

    #[test]
    fn session_reset_starts_a_new_revision_epoch() {
        let registry = PublishedStateRegistry::default();
        registry.update_context_budget("session-1", &decision());
        registry.reset_session("session-2");
        let status = registry.update_context_budget("session-2", &decision());
        assert_eq!(status.session_id, "session-2");
        assert_eq!(status.revision, 1);
        assert_eq!(status.heartbeat_sequence, 0);
    }
}
