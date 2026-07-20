use tokio_util::sync::CancellationToken;

use crate::application::interaction::InteractionBridge;
use crate::domain::agent_run::RunControl;

#[derive(Debug)]
pub(crate) struct ActiveRun {
    pub parent_run_id: Option<sdk::RunId>,
    pub cancel: CancellationToken,
    pub control: Option<RunControl>,
    pub control_delivered: bool,
    pub legacy_cancelling: bool,
    pub legacy_delivered: bool,
    pub terminal: bool,
}

pub(crate) struct ActiveRunRegistry {
    active: std::sync::Mutex<std::collections::HashMap<sdk::RunId, ActiveRun>>,
    interaction: std::sync::Arc<InteractionBridge>,
}

impl Default for ActiveRunRegistry {
    fn default() -> Self {
        Self::new(std::sync::Arc::new(InteractionBridge::new()))
    }
}

impl crate::domain::agent_run::ActiveRunPort for ActiveRunRegistry {
    fn activate(
        &self,
        run_id: sdk::RunId,
        parent_run_id: Option<sdk::RunId>,
        cancel: CancellationToken,
    ) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let inherited_control = parent_run_id
            .as_ref()
            .and_then(|parent| guard.get(parent))
            .and_then(|parent| match parent.control.clone() {
                Some(control @ RunControl::Terminate { .. }) => Some(control),
                Some(RunControl::CancelStep) | None => None,
            });
        let inherited_termination = inherited_control.is_some();
        if inherited_termination {
            cancel.cancel();
        }
        guard.insert(
            run_id.clone(),
            ActiveRun {
                parent_run_id,
                cancel,
                control: inherited_control,
                control_delivered: false,
                legacy_cancelling: false,
                legacy_delivered: false,
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
        if active.control.is_some() || active.legacy_cancelling || active.terminal {
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
        if active.terminal || active.control.is_some() {
            return false;
        }
        if active.legacy_cancelling {
            return true;
        }
        active.legacy_cancelling = true;
        true
    }

    fn take_control(&self, run_id: &sdk::RunId) -> Option<RunControl> {
        ActiveRunRegistry::take_control(self, run_id)
    }

    fn take_legacy_cancellation(&self, run_id: &sdk::RunId) -> bool {
        ActiveRunRegistry::take_legacy_cancellation(self, run_id)
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
    pub fn new(interaction: std::sync::Arc<InteractionBridge>) -> Self {
        Self {
            active: std::sync::Mutex::new(std::collections::HashMap::new()),
            interaction,
        }
    }

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
        if active.control.is_some() || active.legacy_cancelling {
            return sdk::CancelRunOutcome::AlreadyCancelling;
        }
        active.legacy_cancelling = true;
        active.cancel.cancel();
        sdk::CancelRunOutcome::Accepted
    }

    pub fn cancel_step(&self, run_id: &sdk::RunId) -> sdk::CancelRunStepOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return sdk::CancelRunStepOutcome::NotFound;
        };
        if active.terminal {
            return sdk::CancelRunStepOutcome::RunTerminal;
        }
        if active.legacy_cancelling {
            return sdk::CancelRunStepOutcome::AlreadyCancelling;
        }
        match active.control {
            Some(RunControl::Terminate { .. }) => {
                return sdk::CancelRunStepOutcome::RunTerminating;
            }
            Some(RunControl::CancelStep) => {
                return sdk::CancelRunStepOutcome::AlreadyCancelling;
            }
            None => {}
        }
        active.control = Some(RunControl::CancelStep);
        active.control_delivered = false;
        active.cancel.cancel();
        drop(guard);
        self.interaction
            .drain_run(run_id, sdk::InteractionCancelReason::RunCancelled);
        sdk::CancelRunStepOutcome::Accepted
    }

    pub fn terminate(
        &self,
        run_id: &sdk::RunId,
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    ) -> sdk::TerminateRunOutcome {
        self.terminate_tree(run_id, reason, deadline)
    }

    fn terminate_tree(
        &self,
        run_id: &sdk::RunId,
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    ) -> sdk::TerminateRunOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get(run_id) else {
            return sdk::TerminateRunOutcome::NotFound;
        };
        if active.terminal {
            return sdk::TerminateRunOutcome::AlreadyTerminal;
        }
        if matches!(active.control, Some(RunControl::Terminate { .. })) {
            return sdk::TerminateRunOutcome::AlreadyTerminating;
        }

        let mut descendants = vec![run_id.clone()];
        let mut cursor = 0;
        while cursor < descendants.len() {
            let parent = descendants[cursor].clone();
            for (candidate, entry) in guard.iter() {
                if entry.parent_run_id.as_ref() == Some(&parent) && !descendants.contains(candidate)
                {
                    descendants.push(candidate.clone());
                }
            }
            cursor += 1;
        }
        for descendant in &descendants {
            if let Some(entry) = guard.get_mut(descendant) {
                entry.control = Some(RunControl::Terminate { reason, deadline });
                entry.control_delivered = false;
                entry.cancel.cancel();
            }
        }
        drop(guard);
        for descendant in descendants {
            self.interaction
                .drain_run(&descendant, sdk::InteractionCancelReason::RunCancelled);
        }
        sdk::TerminateRunOutcome::Accepted
    }

    pub fn take_control(&self, run_id: &sdk::RunId) -> Option<RunControl> {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = guard.get_mut(run_id)?;
        let control = active.control.clone()?;
        if active.control_delivered {
            return None;
        }
        active.control_delivered = true;
        if matches!(control, RunControl::CancelStep) {
            active.cancel = CancellationToken::new();
        }
        Some(control)
    }

    pub fn take_legacy_cancellation(&self, run_id: &sdk::RunId) -> bool {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return false;
        };
        if !active.legacy_cancelling || active.legacy_delivered {
            return false;
        }
        active.legacy_delivered = true;
        true
    }

    #[cfg(test)]
    pub fn control_token(&self, run_id: &sdk::RunId) -> Option<CancellationToken> {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(run_id)
            .map(|active| active.cancel.clone())
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
    use crate::domain::agent_run::ActiveRunPort;

    #[tokio::test]
    async fn accepted_control_drains_pending_interaction_waiter() {
        let bridge = std::sync::Arc::new(InteractionBridge::new());
        let registry = ActiveRunRegistry::new(bridge.clone());
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), None, CancellationToken::new());
        let request = sdk::InteractionRequest {
            id: sdk::InteractionRequestId::new_v7(),
            run_id: run_id.clone(),
            body: sdk::InteractionRequestBody::UserQuestions(vec![sdk::UserQuestion {
                prompt: "继续？".to_string(),
                options: vec!["是".to_string()],
                allow_multi: false,
            }]),
        };
        let waiter = bridge.register(request.clone()).unwrap();

        assert_eq!(
            registry.cancel_step(&run_id),
            sdk::CancelRunStepOutcome::Accepted
        );
        assert_eq!(
            waiter.await.unwrap(),
            crate::application::interaction::InteractionCompletion::Cancelled(
                sdk::InteractionCancelReason::RunCancelled
            )
        );
        assert!(!bridge.contains(&request.id));
    }

    #[test]
    fn legacy_cancellation_claim_never_overwrites_pending_terminate() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), None, CancellationToken::new());
        let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
        assert_eq!(
            registry.terminate(
                &run_id,
                sdk::RunTerminationReason::SessionShutdown,
                deadline,
            ),
            sdk::TerminateRunOutcome::Accepted
        );

        assert!(!registry.claim_cancellation(&run_id));
        assert_eq!(
            registry.take_control(&run_id),
            Some(RunControl::Terminate {
                reason: sdk::RunTerminationReason::SessionShutdown,
                deadline,
            })
        );
    }

    #[test]
    fn child_activated_after_parent_termination_inherits_same_control() {
        let registry = ActiveRunRegistry::default();
        let parent = sdk::RunId::new_v7();
        let child = sdk::RunId::new_v7();
        registry.activate(parent.clone(), None, CancellationToken::new());
        let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
        assert_eq!(
            registry.terminate(
                &parent,
                sdk::RunTerminationReason::ParentStepCancelled,
                deadline,
            ),
            sdk::TerminateRunOutcome::Accepted
        );
        let child_token = CancellationToken::new();

        registry.activate(child.clone(), Some(parent), child_token.clone());

        assert!(child_token.is_cancelled());
        assert_eq!(
            registry.take_control(&child),
            Some(RunControl::Terminate {
                reason: sdk::RunTerminationReason::ParentStepCancelled,
                deadline,
            })
        );
    }

    #[test]
    fn target_controls_are_typed_idempotent_and_termination_preempts_step_cancel() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        let token = CancellationToken::new();
        registry.activate(run_id.clone(), None, token.clone());
        let deadline = sdk::ControlDeadline::from_unix_millis(1234);

        assert_eq!(
            registry.cancel_step(&run_id),
            sdk::CancelRunStepOutcome::Accepted
        );
        assert!(token.is_cancelled());
        assert_eq!(
            registry.cancel_step(&run_id),
            sdk::CancelRunStepOutcome::AlreadyCancelling
        );
        assert_eq!(registry.take_control(&run_id), Some(RunControl::CancelStep));
        assert_eq!(registry.take_control(&run_id), None);
        assert!(!registry.control_token(&run_id).unwrap().is_cancelled());

        assert_eq!(
            registry.terminate(&run_id, sdk::RunTerminationReason::UserExit, deadline,),
            sdk::TerminateRunOutcome::Accepted
        );
        assert_eq!(
            registry.take_control(&run_id),
            Some(RunControl::Terminate {
                reason: sdk::RunTerminationReason::UserExit,
                deadline,
            })
        );
        assert_eq!(
            registry.terminate(&run_id, sdk::RunTerminationReason::UserExit, deadline,),
            sdk::TerminateRunOutcome::AlreadyTerminating
        );
    }

    #[test]
    fn terminate_propagates_same_absolute_deadline_to_all_descendants() {
        let registry = ActiveRunRegistry::default();
        let parent = sdk::RunId::new_v7();
        let child = sdk::RunId::new_v7();
        let grandchild = sdk::RunId::new_v7();
        registry.activate(parent.clone(), None, CancellationToken::new());
        registry.activate(
            child.clone(),
            Some(parent.clone()),
            CancellationToken::new(),
        );
        registry.activate(
            grandchild.clone(),
            Some(child.clone()),
            CancellationToken::new(),
        );
        let deadline = sdk::ControlDeadline::from_unix_millis(42_000);

        assert_eq!(
            registry.terminate(
                &parent,
                sdk::RunTerminationReason::ParentStepCancelled,
                deadline,
            ),
            sdk::TerminateRunOutcome::Accepted
        );
        let expected = Some(RunControl::Terminate {
            reason: sdk::RunTerminationReason::ParentStepCancelled,
            deadline,
        });
        assert_eq!(registry.take_control(&parent), expected);
        assert_eq!(registry.take_control(&child), expected);
        assert_eq!(registry.take_control(&grandchild), expected);
    }

    #[test]
    fn registry_tracks_parent_and_multiple_sub_runs_independently() {
        let registry = ActiveRunRegistry::default();
        let parent = sdk::RunId::new_v7();
        let sub_a = sdk::RunId::new_v7();
        let sub_b = sdk::RunId::new_v7();
        let parent_token = CancellationToken::new();
        let sub_a_token = parent_token.child_token();
        let sub_b_token = parent_token.child_token();

        registry.activate(parent.clone(), None, parent_token.clone());
        registry.activate(sub_a.clone(), Some(parent.clone()), sub_a_token.clone());
        registry.activate(sub_b.clone(), Some(parent.clone()), sub_b_token.clone());

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
        registry.activate(run_id.clone(), None, token.clone());

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
        registry.activate(run_id.clone(), None, CancellationToken::new());

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
        registry.activate(run_id.clone(), None, CancellationToken::new());

        assert!(registry.claim_terminal(&run_id));
        assert!(!registry.claim_cancellation(&run_id));
    }

    #[test]
    fn cancellation_wins_over_terminal_claim() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), None, CancellationToken::new());

        assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::Accepted);
        assert!(!registry.claim_terminal(&run_id));
    }

    #[test]
    fn clear_only_removes_matching_run() {
        let registry = ActiveRunRegistry::default();
        let run_id = sdk::RunId::new_v7();
        let other = sdk::RunId::new_v7();
        registry.activate(run_id.clone(), None, CancellationToken::new());

        registry.clear(&other);
        assert_eq!(registry.active_ids(), vec![run_id.clone()]);
        registry.clear(&run_id);
        assert!(registry.active_ids().is_empty());
    }
}
