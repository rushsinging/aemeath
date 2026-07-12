use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct ActiveRun {
    pub run_id: sdk::RunId,
    pub cancel: CancellationToken,
    pub cancelling: bool,
    pub terminal: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ActiveRunRegistry {
    active: std::sync::Mutex<Option<ActiveRun>>,
}

impl crate::business::agent_run::ActiveRunPort for ActiveRunRegistry {
    fn activate(&self, run_id: sdk::RunId, cancel: CancellationToken) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *guard = Some(ActiveRun {
            run_id,
            cancel,
            cancelling: false,
            terminal: false,
        });
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.as_mut() else {
            return false;
        };
        if &active.run_id != run_id || active.cancelling || active.terminal {
            return false;
        }
        active.terminal = true;
        true
    }

    fn clear(&self, run_id: &sdk::RunId) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if guard
            .as_ref()
            .is_some_and(|active| &active.run_id == run_id)
        {
            *guard = None;
        }
    }
}

impl ActiveRunRegistry {
    pub fn cancel(&self, run_id: &sdk::RunId) -> sdk::CancelRunOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.as_mut() else {
            return sdk::CancelRunOutcome::NotFound;
        };
        if &active.run_id != run_id {
            return sdk::CancelRunOutcome::NotFound;
        }
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
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|active| active.run_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::agent_run::ActiveRunPort;

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
        assert_eq!(registry.active_id(), Some(run_id.clone()));
        registry.clear(&run_id);
        assert_eq!(registry.active_id(), None);
    }
}
