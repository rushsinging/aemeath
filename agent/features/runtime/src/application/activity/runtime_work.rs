use super::{
    ActivityCoordinator, ActivityDetail, ActivityError, ActivityKind, ActivitySource,
    StartActivity, UpdateActivity,
};
use sdk::{ActivityAudienceView, ActivityId, RunStepId};

impl ActivityCoordinator {
    pub(crate) fn start_hook_dispatch(
        &self,
        run_step_id: RunStepId,
        parent_activity_id: ActivityId,
        point: sdk::HookPointView,
        script: String,
        attempt: u8,
    ) -> Result<ActivityId, ActivityError> {
        self.start(StartActivity {
            run_step_id: Some(run_step_id),
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::HookDispatch(ActivityId::new_v7()),
            kind: ActivityKind::HookDispatch,
            detail: ActivityDetail::Hook {
                point,
                script,
                attempt,
            },
            audience: ActivityAudienceView::Operational,
        })
    }

    pub(crate) fn update_hook_dispatch(
        &self,
        activity_id: ActivityId,
        point: sdk::HookPointView,
        script: String,
        attempt: u8,
    ) -> Result<(), ActivityError> {
        self.update(UpdateActivity {
            activity_id,
            detail: Some(ActivityDetail::Hook {
                point,
                script,
                attempt,
            }),
        })
    }

    pub(crate) fn start_compaction(
        &self,
        run_step_id: RunStepId,
        parent_activity_id: ActivityId,
        stage: sdk::CompactStageView,
    ) -> Result<ActivityId, ActivityError> {
        self.start(StartActivity {
            run_step_id: Some(run_step_id.clone()),
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::Compaction(ActivityId::new_v7()),
            kind: ActivityKind::Compaction,
            detail: ActivityDetail::Compact {
                stage,
                work: sdk::CompactWorkView::Indeterminate,
            },
            audience: ActivityAudienceView::User,
        })
    }

    pub(crate) fn update_compaction(
        &self,
        activity_id: ActivityId,
        stage: sdk::CompactStageView,
        work: sdk::CompactWorkView,
    ) -> Result<(), ActivityError> {
        self.update(UpdateActivity {
            activity_id,
            detail: Some(ActivityDetail::Compact { stage, work }),
        })
    }

    pub(crate) fn start_manual_compaction(
        &self,
        stage: sdk::CompactStageView,
    ) -> Result<ActivityId, ActivityError> {
        self.transaction(|| {
            self.ensure_run_observation_started()?;
            let parent_activity_id = self
                .live_run_root_id()
                .ok_or_else(|| ActivityError::UnknownActivity(ActivityId::new("run-activity")))?;
            self.start(StartActivity {
                run_step_id: None,
                parent_activity_id: Some(parent_activity_id),
                source: ActivitySource::Compaction(ActivityId::new_v7()),
                kind: ActivityKind::Compaction,
                detail: ActivityDetail::Compact {
                    stage,
                    work: sdk::CompactWorkView::Indeterminate,
                },
                audience: ActivityAudienceView::User,
            })
        })
    }

    /// 手动反思 activity（Manual Reflection Run 无 RunStep；归属 Run 根下）。
    /// `trigger` 语义上恒为 `Manual`，参数化只为与 `start_reflection` 对齐。
    pub(crate) fn start_manual_reflection(
        &self,
        trigger: sdk::ReflectionTriggerView,
    ) -> Result<ActivityId, ActivityError> {
        self.transaction(|| {
            self.ensure_run_observation_started()?;
            let parent_activity_id = self
                .live_run_root_id()
                .ok_or_else(|| ActivityError::UnknownActivity(ActivityId::new("run-activity")))?;
            self.start(StartActivity {
                run_step_id: None,
                parent_activity_id: Some(parent_activity_id),
                source: ActivitySource::Reflection(ActivityId::new_v7()),
                kind: ActivityKind::Reflection,
                detail: ActivityDetail::Reflection { trigger },
                audience: ActivityAudienceView::User,
            })
        })
    }

    /// Interval / PreCompact 反思 activity（归属当前对话 Run 给定父节点下）。
    pub(crate) fn start_reflection(
        &self,
        parent_activity_id: ActivityId,
        trigger: sdk::ReflectionTriggerView,
    ) -> Result<ActivityId, ActivityError> {
        self.start(StartActivity {
            run_step_id: None,
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::Reflection(ActivityId::new_v7()),
            kind: ActivityKind::Reflection,
            detail: ActivityDetail::Reflection { trigger },
            audience: ActivityAudienceView::User,
        })
    }

    pub(crate) fn start_interaction(
        &self,
        run_step_id: RunStepId,
        parent_activity_id: ActivityId,
        request_id: sdk::InteractionRequestId,
        kind: sdk::InteractionKindView,
    ) -> Result<ActivityId, ActivityError> {
        let activity_id = self.start(StartActivity {
            run_step_id: Some(run_step_id),
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::Interaction(request_id),
            kind: ActivityKind::Interaction,
            detail: ActivityDetail::Interaction { kind },
            audience: ActivityAudienceView::User,
        })?;
        self.wait(UpdateActivity {
            activity_id: activity_id.clone(),
            detail: None,
        })?;
        Ok(activity_id)
    }

    pub(crate) fn finish_interaction_by_source(
        &self,
        request_id: &sdk::InteractionRequestId,
        terminal: super::ActivityTerminal,
    ) -> Result<(), ActivityError> {
        let source = ActivitySource::Interaction(request_id.clone());
        let Some(activity_id) = self.live_activity_id_for_source(&source) else {
            return Ok(());
        };
        match self.finish(activity_id, terminal) {
            Err(ActivityError::UnknownActivity(_)) => Ok(()),
            result => result,
        }
    }
}
