use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct ActiveRun {
    pub cancel: CancellationToken,
    pub cancelling: bool,
    pub terminal: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ActiveRunRegistry {
    active: std::sync::Mutex<std::collections::HashMap<sdk::RunId, ActiveRun>>,
}

impl crate::business::agent_run::ActiveRunPort for ActiveRunRegistry {
    fn activate(&self, run_id: sdk::RunId, cancel: CancellationToken) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        guard.insert(
            run_id.clone(),
            ActiveRun {
                cancel,
                cancelling: false,
                terminal: false,
            },
        );
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return false;
        };
        if active.cancelling || active.terminal {
            return false;
        }
        active.terminal = true;
        true
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return false;
        };
        if active.terminal {
            return false;
        }
        active.cancelling = true;
        true
    }

    fn clear(&self, run_id: &sdk::RunId) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        guard.remove(run_id);
    }
}

impl ActiveRunRegistry {
    pub fn cancel(&self, run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return sdk::CancelRunOutcome::NotFound;
        };
        if active.terminal {
            return sdk::CancelRunOutcome::AlreadyTerminal;
        }
        if active.cancelling {
            return sdk::CancelRunOutcome::AlreadyCancelling;
        }
        active.cancelling = true;
        active.cancel.cancel();
        sdk::CancelRunOutcome::Accepted
    }

    #[cfg(test)]
    pub fn active_id(&self) -> Option<sdk::RunId> {
        let ids = self.active_ids();
        (ids.len() == 1).then(|| ids[0].clone())
    }

    #[cfg(test)]
    pub fn active_ids(&self) -> Vec<sdk::RunId> {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::agent_run::ActiveRunPort;

    #[test]
    fn registry_tracks_parent_and_multiple_sub_runs_independently() {
        let registry = ActiveRunRegistry::default();
        let parent = sdk::RunId::new_v7();
        let sub_a = sdk::RunId::new_v7();
        let sub_b = sdk::RunId::new_v7();
        let parent_token = CancellationToken::new();
        let sub_a_token = parent_token.child_token();
        let sub_b_token = parent_token.child_token();

        registry.activate(parent.clone(), parent_token.clone());
        registry.activate(sub_a.clone(), sub_a_token.clone());
        registry.activate(sub_b.clone(), sub_b_token.clone());

        assert_eq!(registry.active_ids().len(), 3);
        assert_eq!(registry.cancel(&sub_a), sdk::CancelRunOutcome::Accepted);
        assert!(sub_a_token.is_cancelled());
        assert!(!parent_token.is_cancelled());
        assert!(!sub_b_token.is_cancelled());

        registry.clear(&sub_a);
        assert_eq!(registry.active_ids().len(), 2);
        assert_eq!(registry.cancel(&parent), sdk::CancelRunOutcome::Accepted);
        assert!(parent_token.is_cancelled());
        assert!(sub_b_token.is_cancelled());
    }

    #[test]
    fn cancel_is_synchronous_and_id_scoped() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        let other = sdk::RunId::new_v7();
        let token = CancellationToken::new();
        registry.activate(run_id.clone(), token.clone());

        assert_eq!(registry.cancel(&other), sdk::CancelRunOutcome::NotFound);
        assert!(!token.is_cancelled());
        assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::Accepted);
        assert!(
            token.is_cancelled(),
            "token must be cancelled before return"
        );
        assert_eq!(
            registry.cancel(&run_id),
            sdk::CancelRunOutcome::AlreadyCancelling
        );
    }

    #[test]
    fn terminal_claim_is_visible_to_late_cancel_until_clear() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), CancellationToken::new());

        assert!(registry.claim_terminal(&run_id));
        assert_eq!(
            registry.cancel(&run_id),
            sdk::CancelRunOutcome::AlreadyTerminal
        );
        registry.clear(&run_id);
        assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::NotFound);
    }

    #[test]
    fn terminal_claim_blocks_late_cancellation_claim() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), CancellationToken::new());

        assert!(registry.claim_terminal(&run_id));
        assert!(!registry.claim_cancellation(&run_id));
    }

    #[test]
    fn cancellation_wins_over_terminal_claim() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), CancellationToken::new());

        assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::Accepted);
        assert!(!registry.claim_terminal(&run_id));
    }

    #[test]
    fn clear_only_removes_matching_run() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        let other = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), CancellationToken::new());

        registry.clear(&other);
        assert_eq!(registry.active_ids(), vec![run_id.clone()]);
        registry.clear(&run_id);
        assert!(registry.active_ids().is_empty());
    }
}
