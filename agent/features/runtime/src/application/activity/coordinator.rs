use super::model::{
    ActivityDetail, ActivityKind, ActivityObservation, ActivitySource, ActivityState,
    ActivityTiming,
};
use sdk::{ActivityAudienceView, ActivityId, ActivityTimingView, ActivityView, RunId, RunStepId};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

pub(crate) trait ActivityClock: Send + Sync {
    fn now_monotonic_ms(&self) -> u64;
    fn now_unix_ms(&self) -> u64;
}

pub(crate) trait ActivityIdSource: Send + Sync {
    fn next_activity_id(&self) -> ActivityId;
}

struct UuidV7ActivityIdSource;

impl ActivityIdSource for UuidV7ActivityIdSource {
    fn next_activity_id(&self) -> ActivityId {
        ActivityId::new_v7()
    }
}

struct SystemActivityClock;

impl ActivityClock for SystemActivityClock {
    fn now_monotonic_ms(&self) -> u64 {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    fn now_unix_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityTerminal {
    Succeeded,
    Failed,
    Cancelled,
    Terminated,
}

impl ActivityTerminal {
    fn state(self) -> ActivityState {
        match self {
            Self::Succeeded => ActivityState::Succeeded,
            Self::Failed => ActivityState::Failed,
            Self::Cancelled => ActivityState::Cancelled,
            Self::Terminated => ActivityState::Terminated,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StartActivity {
    pub(crate) run_step_id: Option<RunStepId>,
    pub(crate) parent_activity_id: Option<ActivityId>,
    pub(crate) source: ActivitySource,
    pub(crate) kind: ActivityKind,
    pub(crate) detail: ActivityDetail,
    pub(crate) audience: ActivityAudienceView,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdateActivity {
    pub(crate) activity_id: ActivityId,
    pub(crate) detail: Option<ActivityDetail>,
}

#[derive(Debug, Error)]
pub(crate) enum ActivityError {
    #[error("活动不存在，无法更新: {0}")]
    UnknownActivity(ActivityId),
    #[error("活动已处于终态，无法再次变更: {0}")]
    TerminalActivity(ActivityId),
    #[error("同一来源已有活动存在: {0}")]
    DuplicateLiveSource(String),
    #[error("活动父节点不存在: {0}")]
    UnknownParent(ActivityId),
    #[error("活动所属 Run 不匹配")]
    RunMismatch,
    #[error("ActivityCoordinator 尚未绑定到 RunLoop")]
    CoordinatorNotBound,
    #[error("活动来源与所属 RunStep 不匹配")]
    RunStepMismatch,
}

#[derive(Default)]
struct ActivityRegistry {
    activities: HashMap<ActivityId, ActivityObservation>,
}

#[allow(dead_code)]
pub(crate) struct ActivitySnapshot {
    pub(crate) run_id: RunId,
    pub(crate) revision: u64,
    pub(crate) activities: Vec<ActivityView>,
}

#[allow(dead_code)]
impl ActivitySnapshot {
    pub(crate) fn find(&self, activity_id: &ActivityId) -> Option<&ActivityView> {
        self.activities
            .iter()
            .find(|activity| &activity.id == activity_id)
    }
}

pub(crate) struct ActivityCoordinator {
    run_id: RunId,
    clock: Arc<dyn ActivityClock>,
    ids: Arc<dyn ActivityIdSource>,
    registry: parking_lot::Mutex<ActivityRegistry>,
    revision: parking_lot::Mutex<u64>,
}

impl ActivityCoordinator {
    pub(crate) fn new(
        run_id: RunId,
        clock: Arc<dyn ActivityClock>,
        ids: Arc<dyn ActivityIdSource>,
    ) -> Self {
        Self {
            run_id,
            clock,
            ids,
            registry: parking_lot::Mutex::new(ActivityRegistry::default()),
            revision: parking_lot::Mutex::new(0),
        }
    }

    pub(crate) fn production(run_id: RunId) -> Self {
        Self::new(
            run_id,
            Arc::new(SystemActivityClock),
            Arc::new(UuidV7ActivityIdSource),
        )
    }

    pub(crate) fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub(super) fn has_activity_source(
        &self,
        source: &ActivitySource,
        run_step_id: Option<&RunStepId>,
    ) -> bool {
        self.registry.lock().activities.values().any(|activity| {
            &activity.source == source && activity.run_step_id.as_ref() == run_step_id
        })
    }

    pub(super) fn live_activity_id(
        &self,
        source: &ActivitySource,
        run_step_id: Option<&RunStepId>,
    ) -> Option<ActivityId> {
        self.registry
            .lock()
            .activities
            .values()
            .find(|activity| {
                !activity.state.is_terminal()
                    && &activity.source == source
                    && activity.run_step_id.as_ref() == run_step_id
            })
            .map(|activity| activity.id.clone())
    }

    pub(super) fn live_activity_id_for_source(
        &self,
        source: &ActivitySource,
    ) -> Option<ActivityId> {
        let registry = self.registry.lock();
        registry
            .activities
            .values()
            .find(|activity| !activity.state.is_terminal() && &activity.source == source)
            .map(|activity| activity.id.clone())
    }

    pub(crate) fn ensure_run_observation_started(&self) -> Result<(), ActivityError> {
        if self.live_run_root_id().is_some() {
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

    pub(crate) fn live_run_root_id(&self) -> Option<ActivityId> {
        self.live_activity_id(&ActivitySource::Run, None)
    }

    pub(crate) fn live_run_phase_id(&self) -> Option<ActivityId> {
        self.registry
            .lock()
            .activities
            .values()
            .find(|activity| {
                !activity.state.is_terminal() && matches!(activity.kind, ActivityKind::RunPhase(_))
            })
            .map(|activity| activity.id.clone())
    }

    pub(crate) fn start(&self, command: StartActivity) -> Result<ActivityId, ActivityError> {
        validate_source_scope(&command)?;
        let now = self.clock.now_monotonic_ms();
        let mut registry = self.registry.lock();
        if let Some(parent) = &command.parent_activity_id {
            let parent_activity = registry
                .activities
                .get(parent)
                .ok_or_else(|| ActivityError::UnknownParent(parent.clone()))?;
            if parent_activity.state.is_terminal() {
                return Err(ActivityError::TerminalActivity(parent.clone()));
            }
        }
        let duplicate_source = registry.activities.values().any(|activity| {
            !activity.state.is_terminal()
                && activity.source == command.source
                && activity.run_step_id == command.run_step_id
        });
        if duplicate_source {
            return Err(ActivityError::DuplicateLiveSource(format!(
                "{:?}",
                command.source
            )));
        }
        let activity_id = self.ids.next_activity_id();
        let revision = self.next_revision();
        registry.activities.insert(
            activity_id.clone(),
            ActivityObservation {
                id: activity_id.clone(),
                run_id: self.run_id.clone(),
                run_step_id: command.run_step_id,
                parent_activity_id: command.parent_activity_id,
                source: command.source,
                kind: command.kind,
                state: ActivityState::Running,
                detail: command.detail,
                audience: command.audience,
                revision,
                timing: ActivityTiming {
                    started_at_unix_ms: Some(self.clock.now_unix_ms()),
                    ..ActivityTimingView::default().into()
                },
                started_at_monotonic_ms: now,
                last_transition_monotonic_ms: now,
                active_started_monotonic_ms: Some(now),
            },
        );
        Ok(activity_id)
    }

    pub(crate) fn update(&self, command: UpdateActivity) -> Result<(), ActivityError> {
        let activity_id = command.activity_id;
        let mut registry = self.registry.lock();
        let activity = registry
            .activities
            .get_mut(&activity_id)
            .ok_or_else(|| ActivityError::UnknownActivity(activity_id.clone()))?;
        if activity.state.is_terminal() {
            return Err(ActivityError::TerminalActivity(activity_id));
        }
        if let Some(detail) = command.detail {
            activity.detail = detail;
        }
        activity.revision = self.next_revision();
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn wait(&self, command: UpdateActivity) -> Result<(), ActivityError> {
        self.update_detail(&command)?;
        self.transition(command.activity_id, ActivityState::Waiting)
    }

    #[allow(dead_code)]
    pub(crate) fn resume(&self, command: UpdateActivity) -> Result<(), ActivityError> {
        self.update_detail(&command)?;
        self.transition(command.activity_id, ActivityState::Running)
    }

    pub(crate) fn finish(
        &self,
        activity_id: ActivityId,
        terminal: ActivityTerminal,
    ) -> Result<(), ActivityError> {
        let now = self.clock.now_monotonic_ms();
        let mut registry = self.registry.lock();
        let activity = registry
            .activities
            .get_mut(&activity_id)
            .ok_or_else(|| ActivityError::UnknownActivity(activity_id.clone()))?;
        if activity.state.is_terminal() {
            if activity.state == terminal.state() {
                return Ok(());
            }
            return Err(ActivityError::TerminalActivity(activity_id));
        }
        let active_elapsed_ms = current_active_elapsed(activity, now);
        activity.timing = ActivityTiming {
            total_elapsed_ms: now.saturating_sub(activity.started_at_monotonic_ms),
            active_elapsed_ms,
            state_elapsed_ms: now.saturating_sub(activity.last_transition_monotonic_ms),
            started_at_unix_ms: activity.timing.started_at_unix_ms,
            finished_at_unix_ms: Some(self.clock.now_unix_ms()),
        };
        activity.active_started_monotonic_ms = None;
        activity.last_transition_monotonic_ms = now;
        activity.state = terminal.state();
        activity.revision = self.next_revision();
        Ok(())
    }

    pub(crate) fn close_run(&self, terminal: ActivityTerminal) -> Result<(), ActivityError> {
        let activity_ids = self
            .registry
            .lock()
            .activities
            .values()
            .filter(|activity| !activity.state.is_terminal())
            .map(|activity| activity.id.clone())
            .collect::<Vec<_>>();
        for activity_id in activity_ids {
            self.finish(activity_id, terminal)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn snapshot(&self) -> ActivitySnapshot {
        let now = self.clock.now_monotonic_ms();
        let registry = self.registry.lock();
        let mut activities = registry
            .activities
            .values()
            .map(|activity| activity.to_sdk(now))
            .collect::<Vec<_>>();
        activities.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        ActivitySnapshot {
            run_id: self.run_id.clone(),
            revision: *self.revision.lock(),
            activities,
        }
    }

    fn update_detail(&self, command: &UpdateActivity) -> Result<(), ActivityError> {
        let Some(detail) = command.detail.clone() else {
            return Ok(());
        };
        self.update(UpdateActivity {
            activity_id: command.activity_id.clone(),
            detail: Some(detail),
        })
    }

    fn transition(
        &self,
        activity_id: ActivityId,
        state: ActivityState,
    ) -> Result<(), ActivityError> {
        let now = self.clock.now_monotonic_ms();
        let mut registry = self.registry.lock();
        let activity = registry
            .activities
            .get_mut(&activity_id)
            .ok_or_else(|| ActivityError::UnknownActivity(activity_id.clone()))?;
        if activity.state.is_terminal() {
            return Err(ActivityError::TerminalActivity(activity_id));
        }
        if activity.state == state {
            return Ok(());
        }
        if state == ActivityState::Running {
            activity.active_started_monotonic_ms = Some(now);
        } else if activity.state == ActivityState::Running {
            activity.timing.active_elapsed_ms += current_active_elapsed(activity, now)
                .saturating_sub(activity.timing.active_elapsed_ms);
            activity.active_started_monotonic_ms = None;
        }
        activity.state = state;
        activity.last_transition_monotonic_ms = now;
        activity.revision = self.next_revision();
        Ok(())
    }

    fn next_revision(&self) -> u64 {
        let mut revision = self.revision.lock();
        *revision += 1;
        *revision
    }
}

fn validate_source_scope(command: &StartActivity) -> Result<(), ActivityError> {
    if let ActivitySource::RunStep(source_step_id) = &command.source {
        if command.run_step_id.as_ref() != Some(source_step_id) {
            return Err(ActivityError::RunStepMismatch);
        }
    }
    Ok(())
}

fn current_active_elapsed(activity: &ActivityObservation, now: u64) -> u64 {
    activity.timing.active_elapsed_ms
        + activity
            .active_started_monotonic_ms
            .map(|started| now.saturating_sub(started))
            .unwrap_or_default()
}

impl From<ActivityTimingView> for super::model::ActivityTiming {
    fn from(timing: ActivityTimingView) -> Self {
        Self {
            total_elapsed_ms: timing.total_elapsed_ms,
            active_elapsed_ms: timing.active_elapsed_ms,
            state_elapsed_ms: timing.state_elapsed_ms,
            started_at_unix_ms: timing.started_at_unix_ms,
            finished_at_unix_ms: timing.finished_at_unix_ms,
        }
    }
}
