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
        decision: &context::CompactionDecisionData,
    ) -> sdk::RuntimeStatusView {
        let session_id = session_id.into();
        let mut state = self.inner.lock();
        let next_revision = state
            .status
            .as_ref()
            .filter(|status| status.session_id == session_id)
            .map_or(1, |status| status.revision.saturating_add(1));
        let source = match decision.reason {
            context::DecisionReason::ActualProviderUsage => {
                sdk::ContextDecisionSourceView::ActualProviderUsage
            }
            context::DecisionReason::HeuristicFallback => {
                sdk::ContextDecisionSourceView::HeuristicFallback
            }
            context::DecisionReason::MisconfiguredWindow => {
                sdk::ContextDecisionSourceView::MisconfiguredWindow
            }
            context::DecisionReason::Manual => sdk::ContextDecisionSourceView::Manual,
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
#[path = "registry_tests.rs"]
mod tests;
