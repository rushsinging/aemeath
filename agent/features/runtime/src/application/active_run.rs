use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct MainStepScope {
    id: sdk::RunStepId,
    cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRun {
    pub cancel: CancellationToken,
    pub cancelling: bool,
    pub terminal: bool,
    main_step: Option<MainStepScope>,
    control: Option<crate::domain::agent_run::RunControl>,
    control_delivered: bool,
}

#[derive(Debug, Default)]
pub struct ActiveRunRegistry {
    active: std::sync::Mutex<std::collections::HashMap<sdk::RunId, ActiveRun>>,
}

impl crate::domain::agent_run::ActiveRunPort for ActiveRunRegistry {
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
                main_step: None,
                control: None,
                control_delivered: false,
            },
        );
    }

    fn activate_main(&self, run_id: sdk::RunId, cancel: CancellationToken) {
        self.activate(run_id, cancel);
    }

    fn set_main_active_step(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(active) = guard.get_mut(run_id) {
            active.main_step = Some(MainStepScope {
                id: step_id,
                cancel,
            });
        }
    }

    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        ActiveRunRegistry::take_control(self, run_id)
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
    pub fn cancel_step(
        &self,
        run_id: &sdk::RunId,
        step_id: Option<&sdk::RunStepId>,
        deadline: sdk::ControlDeadline,
    ) -> sdk::CancelRunStepOutcome {
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
        if matches!(
            active.control,
            Some(crate::domain::agent_run::RunControl::Terminate { .. })
        ) {
            return sdk::CancelRunStepOutcome::RunTerminating;
        }
        if active.control.is_some() || active.cancelling {
            return sdk::CancelRunStepOutcome::AlreadyCancelling;
        }
        let Some(current_step) = active.main_step.as_ref() else {
            return sdk::CancelRunStepOutcome::NoActiveStep;
        };
        if step_id.is_some_and(|requested| requested != &current_step.id) {
            return sdk::CancelRunStepOutcome::NoActiveStep;
        }
        let step_id = current_step.id.clone();
        current_step.cancel.cancel();
        active.control =
            Some(crate::domain::agent_run::RunControl::CancelStep { step_id, deadline });
        active.control_delivered = false;
        sdk::CancelRunStepOutcome::Accepted
    }

    pub fn terminate(
        &self,
        run_id: &sdk::RunId,
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    ) -> sdk::TerminateRunOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(active) = guard.get_mut(run_id) else {
            return sdk::TerminateRunOutcome::NotFound;
        };
        if active.terminal {
            return sdk::TerminateRunOutcome::AlreadyTerminal;
        }
        if matches!(
            active.control,
            Some(crate::domain::agent_run::RunControl::Terminate { .. })
        ) {
            return sdk::TerminateRunOutcome::AlreadyTerminating;
        }
        active.cancel.cancel();
        if let Some(step) = &active.main_step {
            step.cancel.cancel();
        }
        active.control = Some(crate::domain::agent_run::RunControl::Terminate { reason, deadline });
        active.control_delivered = false;
        sdk::TerminateRunOutcome::Accepted
    }

    pub fn take_control(
        &self,
        run_id: &sdk::RunId,
    ) -> Option<crate::domain::agent_run::RunControl> {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = guard.get_mut(run_id)?;
        if active.control_delivered {
            return None;
        }
        let control = active.control.clone()?;
        active.control_delivered = true;
        Some(control)
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
#[path = "active_run_tests.rs"]
mod tests;
