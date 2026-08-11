use super::coordinator::{ActivityError, ActivityTerminal, StartActivity};
use super::model::{ActivityDetail, ActivityKind, ActivitySource, RunPhaseKind};
use super::ActivityCoordinator;
use crate::domain::agent_run::{RunStatus, RuntimeLifecycleEvent};
use sdk::{ActivityAudienceView, ActivityId, RunId};

impl ActivityCoordinator {
    /// 消费同一原子发布批次中的 Run 领域事件，更新根 Activity 与 Run phase Activity。
    pub(crate) fn observe_run_events(
        &self,
        events: &[RuntimeLifecycleEvent],
    ) -> Result<(), ActivityError> {
        self.transaction(|| {
            for event in events {
                self.observe_run_event(event)?;
            }
            Ok(())
        })
    }

    fn observe_run_event(&self, event: &RuntimeLifecycleEvent) -> Result<(), ActivityError> {
        validate_event_run(self.run_id(), event)?;
        match event {
            RuntimeLifecycleEvent::Started { .. } => self.ensure_run_root(),
            RuntimeLifecycleEvent::Transitioned { from, to, .. } => {
                self.observe_run_transition(*from, *to)
            }
            RuntimeLifecycleEvent::DrainingInput { .. } => {
                self.start_phase(RunPhaseKind::DrainingInput)
            }
            _ => Ok(()),
        }
    }

    fn observe_run_transition(&self, from: RunStatus, to: RunStatus) -> Result<(), ActivityError> {
        self.ensure_run_root()?;
        if from_phase(from).is_some() {
            self.finish_live_run_phase(ActivityTerminal::Succeeded)?;
        }
        if let Some(terminal) = terminal_for_status(to) {
            self.close_run(terminal)?;
            return Ok(());
        }
        if let Some(phase) = to_phase(to) {
            self.start_phase(phase)?;
        }
        Ok(())
    }

    fn ensure_run_root(&self) -> Result<(), ActivityError> {
        if self.live_activity_id(&ActivitySource::Run, None).is_some()
            || self.has_activity_source(&ActivitySource::Run, None)
        {
            return Ok(());
        }
        self.start(StartActivity {
            run_step_id: None,
            parent_activity_id: None,
            source: ActivitySource::Run,
            kind: ActivityKind::Run,
            detail: ActivityDetail::Run,
            audience: ActivityAudienceView::User,
        })?;
        Ok(())
    }

    fn start_phase(&self, phase: RunPhaseKind) -> Result<(), ActivityError> {
        let phase_source_id = phase_source_id(self.run_id(), phase);
        let phase_source = ActivitySource::RunStep(phase_source_id.clone());
        if self
            .live_activity_id(&phase_source, Some(&phase_source_id))
            .is_some()
        {
            return Ok(());
        }
        if self.live_activity_id(&ActivitySource::Run, None).is_none() {
            self.ensure_run_root()?;
        }
        if self.live_run_phase_id().is_some() {
            self.finish_live_run_phase(ActivityTerminal::Succeeded)?;
        }
        let root_id = self
            .live_run_root_id()
            .ok_or_else(|| ActivityError::UnknownActivity(ActivityId::new("run-root")))?;
        self.start(StartActivity {
            run_step_id: Some(phase_source_id.clone()),
            parent_activity_id: Some(root_id),
            source: ActivitySource::RunStep(phase_source_id),
            kind: ActivityKind::RunPhase(phase),
            detail: ActivityDetail::Phase(phase),
            audience: ActivityAudienceView::User,
        })?;
        Ok(())
    }

    fn finish_live_run_phase(&self, terminal: ActivityTerminal) -> Result<(), ActivityError> {
        if let Some(activity_id) = self.live_run_phase_id() {
            self.finish(activity_id, terminal)?;
        }
        Ok(())
    }
}

fn validate_event_run(run_id: &RunId, event: &RuntimeLifecycleEvent) -> Result<(), ActivityError> {
    let event_run_id = match event {
        RuntimeLifecycleEvent::Transitioned { run_id, .. }
        | RuntimeLifecycleEvent::Started { run_id, .. }
        | RuntimeLifecycleEvent::StepStarted { run_id, .. }
        | RuntimeLifecycleEvent::StepCompleted { run_id, .. }
        | RuntimeLifecycleEvent::StepCancellationRequested { run_id, .. }
        | RuntimeLifecycleEvent::StepFinalizationStarted { run_id, .. }
        | RuntimeLifecycleEvent::StepCancelled { run_id, .. }
        | RuntimeLifecycleEvent::DrainingInput { run_id, .. }
        | RuntimeLifecycleEvent::TerminationRequested { run_id, .. }
        | RuntimeLifecycleEvent::Terminated { run_id, .. }
        | RuntimeLifecycleEvent::AwaitingUser { run_id, .. }
        | RuntimeLifecycleEvent::Resumed { run_id, .. }
        | RuntimeLifecycleEvent::StuckDetected { run_id, .. }
        | RuntimeLifecycleEvent::Completed { run_id, .. }
        | RuntimeLifecycleEvent::Failed { run_id, .. } => run_id,
    };
    if event_run_id != run_id {
        return Err(ActivityError::RunMismatch);
    }
    Ok(())
}

fn from_phase(status: RunStatus) -> Option<RunPhaseKind> {
    to_phase(status)
}

fn to_phase(status: RunStatus) -> Option<RunPhaseKind> {
    match status {
        RunStatus::DrainingInput => Some(RunPhaseKind::DrainingInput),
        RunStatus::PreparingContext => Some(RunPhaseKind::PreparingContext),
        RunStatus::ApplyingResponse => Some(RunPhaseKind::ApplyingResponse),
        RunStatus::AwaitingToolApproval => Some(RunPhaseKind::AwaitingToolApproval),
        RunStatus::ExecutingTools => Some(RunPhaseKind::ExecutingTools),
        RunStatus::FinalizingStep => Some(RunPhaseKind::FinalizingStep),
        RunStatus::CancellingStep => Some(RunPhaseKind::CancellingStep),
        RunStatus::Terminating => Some(RunPhaseKind::Terminating),
        RunStatus::Created
        | RunStatus::InvokingModel
        | RunStatus::AwaitingUser
        | RunStatus::Compacting
        | RunStatus::Completed
        | RunStatus::Failed
        | RunStatus::Terminated => None,
    }
}

fn terminal_for_status(status: RunStatus) -> Option<ActivityTerminal> {
    match status {
        RunStatus::Completed => Some(ActivityTerminal::Succeeded),
        RunStatus::Failed => Some(ActivityTerminal::Failed),
        RunStatus::Terminated => Some(ActivityTerminal::Terminated),
        RunStatus::Created
        | RunStatus::DrainingInput
        | RunStatus::PreparingContext
        | RunStatus::InvokingModel
        | RunStatus::ApplyingResponse
        | RunStatus::AwaitingToolApproval
        | RunStatus::ExecutingTools
        | RunStatus::AwaitingUser
        | RunStatus::Compacting
        | RunStatus::CancellingStep
        | RunStatus::FinalizingStep
        | RunStatus::Terminating => None,
    }
}

fn phase_source_id(run_id: &RunId, phase: RunPhaseKind) -> sdk::RunStepId {
    sdk::RunStepId::new(format!("{}:{phase:?}", run_id.as_str()))
}

#[cfg(test)]
pub(super) fn terminal_events_for_test(
    run_id: RunId,
    status: RunStatus,
) -> Vec<RuntimeLifecycleEvent> {
    vec![
        RuntimeLifecycleEvent::Started {
            run_id: run_id.clone(),
            parent_run_id: None,
        },
        RuntimeLifecycleEvent::Transitioned {
            run_id,
            parent_run_id: None,
            from: RunStatus::DrainingInput,
            to: status,
            reason: crate::domain::agent_run::RunTransitionReason::Failed,
            timing: crate::domain::agent_run::RunTimingSnapshot::default(),
        },
    ]
}
