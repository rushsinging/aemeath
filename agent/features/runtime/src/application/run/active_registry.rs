use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct MainStepScope {
    id: sdk::RunStepId,
    cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRun {
    pub cancel: CancellationToken,
    main_step: Option<MainStepScope>,
    control: Option<crate::domain::agent_run::RunControl>,
    control_delivered: bool,
}

#[derive(Debug, Default)]
pub struct ActiveRunRegistry {
    active: std::sync::Mutex<ActiveRunState>,
}

#[derive(Debug, Default)]
struct ActiveRunState {
    runs: std::collections::HashMap<sdk::RunId, ActiveRun>,
    current_main_run_id: Option<sdk::RunId>,
}

impl crate::domain::agent_run::ActiveRunPort for ActiveRunRegistry {
    fn activate(&self, run_id: sdk::RunId, cancel: CancellationToken) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        guard.runs.insert(
            run_id.clone(),
            ActiveRun {
                cancel,
                main_step: None,
                control: None,
                control_delivered: false,
            },
        );
    }

    fn activate_main(&self, run_id: sdk::RunId, cancel: CancellationToken) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous_main_run_id = guard.current_main_run_id.clone();
        guard.runs.insert(
            run_id.clone(),
            ActiveRun {
                cancel,
                main_step: None,
                control: None,
                control_delivered: false,
            },
        );
        guard.current_main_run_id = Some(run_id.clone());
        log::debug!(
            target: crate::LOG_TARGET,
            "active run main activated: run_id={} previous_main_run_id={:?} active_run_count={}",
            run_id,
            previous_main_run_id,
            guard.runs.len()
        );
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
        if let Some(active) = guard.runs.get_mut(run_id) {
            log::debug!(
                target: crate::LOG_TARGET,
                "active run main step set: run_id={} step_id={} root_cancelled={} step_cancelled={}",
                run_id,
                step_id,
                active.cancel.is_cancelled(),
                cancel.is_cancelled()
            );
            active.main_step = Some(MainStepScope {
                id: step_id,
                cancel,
            });
        } else {
            log::debug!(
                target: crate::LOG_TARGET,
                "active run main step ignored: run_id={} step_id={} reason=run_not_found current_main_run_id={:?} active_run_count={}",
                run_id,
                step_id,
                guard.current_main_run_id,
                guard.runs.len()
            );
        }
    }

    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        ActiveRunRegistry::take_control(self, run_id)
    }

    fn clear(&self, run_id: &sdk::RunId) {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let removed = guard.runs.remove(run_id).is_some();
        let cleared_current_main = guard.current_main_run_id.as_ref() == Some(run_id);
        if cleared_current_main {
            guard.current_main_run_id = None;
        }
        log::debug!(
            target: crate::LOG_TARGET,
            "active run cleared: run_id={} removed={} cleared_current_main={} remaining_run_count={}",
            run_id,
            removed,
            cleared_current_main,
            guard.runs.len()
        );
    }
}

impl ActiveRunRegistry {
    pub fn cancel_current_main(
        &self,
        deadline: sdk::ControlDeadline,
    ) -> sdk::CancelCurrentRunOutcome {
        let mut guard = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(run_id) = guard.current_main_run_id.clone() else {
            log::debug!(
                target: crate::LOG_TARGET,
                "cancel current main rejected: outcome=NoActiveRun active_run_count={} deadline_unix_ms={}",
                guard.runs.len(),
                deadline.unix_millis()
            );
            return sdk::CancelCurrentRunOutcome::NoActiveRun;
        };
        let Some(active) = guard.runs.get_mut(&run_id) else {
            guard.current_main_run_id = None;
            log::debug!(
                target: crate::LOG_TARGET,
                "cancel current main rejected: run_id={} outcome=NoActiveRun reason=registry_entry_missing deadline_unix_ms={}",
                run_id,
                deadline.unix_millis()
            );
            return sdk::CancelCurrentRunOutcome::NoActiveRun;
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "cancel current main evaluating: run_id={} step_id={:?} control={:?} root_cancelled={} step_cancelled={} deadline_unix_ms={}",
            run_id,
            active.main_step.as_ref().map(|step| &step.id),
            active.control,
            active.cancel.is_cancelled(),
            active.main_step.as_ref().is_some_and(|step| step.cancel.is_cancelled()),
            deadline.unix_millis()
        );
        let outcome = if matches!(
            active.control,
            Some(crate::domain::agent_run::RunControl::Terminate { .. })
        ) {
            sdk::CancelCurrentRunOutcome::RunTerminating
        } else if active.control.is_some() {
            sdk::CancelCurrentRunOutcome::AlreadyCancelling
        } else if let Some(current_step) = active.main_step.as_ref() {
            let step_id = current_step.id.clone();
            current_step.cancel.cancel();
            active.control =
                Some(crate::domain::agent_run::RunControl::CancelStep { step_id, deadline });
            active.control_delivered = false;
            sdk::CancelCurrentRunOutcome::Accepted
        } else {
            sdk::CancelCurrentRunOutcome::NoActiveStep
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "cancel current main completed: run_id={} step_id={:?} outcome={:?} root_cancelled={} step_cancelled={} control={:?}",
            run_id,
            active.main_step.as_ref().map(|step| &step.id),
            outcome,
            active.cancel.is_cancelled(),
            active.main_step.as_ref().is_some_and(|step| step.cancel.is_cancelled()),
            active.control
        );
        outcome
    }

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
        let Some(active) = guard.runs.get_mut(run_id) else {
            return sdk::CancelRunStepOutcome::NotFound;
        };
        if matches!(
            active.control,
            Some(crate::domain::agent_run::RunControl::Terminate { .. })
        ) {
            return sdk::CancelRunStepOutcome::RunTerminating;
        }
        if active.control.is_some() {
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
        let Some(active) = guard.runs.get_mut(run_id) else {
            return sdk::TerminateRunOutcome::NotFound;
        };
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
        let active = guard.runs.get_mut(run_id)?;
        if active.control_delivered {
            return None;
        }
        let control = active.control.clone()?;
        active.control_delivered = true;
        Some(control)
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
            .runs
            .keys()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
#[path = "active_registry_tests.rs"]
mod tests;
