use super::{
    ActivityCoordinator, ActivityDetail, ActivityError, ActivityKind, ActivitySource,
    StartActivity, UpdateActivity,
};
use crate::application::tool::agent::ToolCall;
use sdk::{ActivityAudienceView, ActivityId, ModelInvocationId, RunStepId};

impl ActivityCoordinator {
    pub(crate) fn start_model_invocation(
        &self,
        run_step_id: RunStepId,
        parent_activity_id: ActivityId,
        invocation_id: ModelInvocationId,
        model: String,
        attempt: u32,
    ) -> Result<ActivityId, ActivityError> {
        self.start(StartActivity {
            run_step_id: Some(run_step_id),
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::ModelInvocation(invocation_id),
            kind: ActivityKind::ModelInvocation,
            detail: ActivityDetail::Model {
                model,
                attempt,
                stream: sdk::ModelStreamStateView::Invoking,
            },
            audience: ActivityAudienceView::User,
        })
    }

    pub(crate) fn update_model_invocation(
        &self,
        activity_id: ActivityId,
        model: String,
        attempt: u32,
        stream: sdk::ModelStreamStateView,
    ) -> Result<(), ActivityError> {
        self.update(UpdateActivity {
            activity_id,
            detail: Some(ActivityDetail::Model {
                model,
                attempt,
                stream,
            }),
        })
    }

    pub(crate) fn start_tool_call(
        &self,
        run_step_id: RunStepId,
        parent_activity_id: ActivityId,
        call: &ToolCall,
        parallel_count: u16,
    ) -> Result<ActivityId, ActivityError> {
        self.start(StartActivity {
            run_step_id: Some(run_step_id),
            parent_activity_id: Some(parent_activity_id),
            source: ActivitySource::ToolCall(call.id.clone()),
            kind: ActivityKind::ToolCall,
            detail: ActivityDetail::Tool {
                name: call.name.clone(),
                summary: None,
                parallel_count,
            },
            audience: ActivityAudienceView::User,
        })
    }

    pub(crate) fn finish_tool_call_by_source(
        &self,
        call_id: &sdk::ToolCallId,
        terminal: super::ActivityTerminal,
    ) -> Result<(), ActivityError> {
        let Some(activity_id) =
            self.live_activity_id_for_source(&ActivitySource::ToolCall(call_id.clone()))
        else {
            return Ok(());
        };
        self.finish(activity_id, terminal)
    }
}
