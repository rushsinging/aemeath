//! Hook dispatch helper — execute once and report through Activity observation.

use crate::application::activity::{ActivityCoordinator, ActivityTerminal};
use crate::application::hook::outcome_mapper::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookDispatch,
};
use hook::{
    HookDispatchContext, HookInvocation, HookPoint, HookPort, HookSubscriptionExecutionEvent,
    HookSubscriptionExecutionObserver, HookSubscriptionExecutionTerminal,
};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct HookActivityObserver {
    activities: Arc<ActivityCoordinator>,
    run_step_id: sdk::RunStepId,
    parent_activity_id: sdk::ActivityId,
    live_activity_id: Mutex<Option<sdk::ActivityId>>,
}

impl HookSubscriptionExecutionObserver for HookActivityObserver {
    fn observe(&self, event: HookSubscriptionExecutionEvent) {
        match event {
            HookSubscriptionExecutionEvent::Started {
                point,
                script,
                attempt,
            } => {
                let activity_id = self
                    .activities
                    .start_hook_dispatch(
                        self.run_step_id.clone(),
                        self.parent_activity_id.clone(),
                        hook_point_view(point),
                        script,
                        attempt,
                    )
                    .ok();
                *self.live_activity_id.lock() = activity_id;
            }
            HookSubscriptionExecutionEvent::AttemptChanged {
                point,
                script,
                attempt,
            } => {
                if let Some(activity_id) = self.live_activity_id.lock().clone() {
                    let _ = self.activities.update_hook_dispatch(
                        activity_id,
                        hook_point_view(point),
                        script,
                        attempt,
                    );
                }
            }
            HookSubscriptionExecutionEvent::Finished { terminal, .. } => {
                if let Some(activity_id) = self.live_activity_id.lock().take() {
                    let terminal = match terminal {
                        HookSubscriptionExecutionTerminal::Succeeded => ActivityTerminal::Succeeded,
                        HookSubscriptionExecutionTerminal::Failed => ActivityTerminal::Failed,
                        HookSubscriptionExecutionTerminal::Cancelled => ActivityTerminal::Cancelled,
                    };
                    let _ = self.activities.finish(activity_id, terminal);
                }
            }
        }
    }
}

fn hook_point_view(point: HookPoint) -> sdk::HookPointView {
    crate::application::hook::stop_coordination::hook_point_view(point)
}

pub(crate) fn subscription_activity_observer(
    activities: &ActivityCoordinator,
    run_step_id: &sdk::RunStepId,
) -> Option<Arc<dyn HookSubscriptionExecutionObserver>> {
    activities
        .live_hook_parent_id()
        .ok()
        .map(|parent_activity_id| {
            Arc::new(HookActivityObserver {
                activities: Arc::new(activities.clone()),
                run_step_id: run_step_id.clone(),
                parent_activity_id,
                live_activity_id: Mutex::new(None),
            }) as Arc<dyn HookSubscriptionExecutionObserver>
        })
}

/// 执行一次 Hook dispatch。生命周期展示只通过 ActivityCoordinator 发布。
pub(crate) async fn dispatch_hook(
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    run_step_id: &sdk::RunStepId,
    invocation: HookInvocation,
    workspace_root: &Path,
    cancel: &CancellationToken,
) -> RuntimeHookDispatch {
    let subscription_execution_observer = subscription_activity_observer(activities, run_step_id);
    let mut context = HookDispatchContext::new(workspace_root);
    if let Some(observer) = subscription_execution_observer {
        context = context.with_subscription_execution_observer(observer);
    }
    let outcome = hook_port.dispatch_at(invocation, context, cancel).await;
    map_hook_outcome(&outcome)
}

pub(crate) fn dispatch_is_blocking(dispatch: &RuntimeHookDispatch) -> bool {
    matches!(dispatch.directive, RuntimeHookDirective::Block { .. })
}
