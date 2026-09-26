//! Hook dispatch helper — execute once and report through Activity observation.

use crate::application::activity::{ActivityCoordinator, ActivityTerminal};
use crate::application::hook::outcome_mapper::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookDispatch,
};
use hook::{
    HookDispatchContextData, HookDispatcher, HookExecutionEventData, HookExecutionObserver,
    HookExecutionTerminalData, HookInvocationData, HookPointData,
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

impl HookExecutionObserver for HookActivityObserver {
    fn observe(&self, event: HookExecutionEventData) {
        match event {
            HookExecutionEventData::Started {
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
            HookExecutionEventData::AttemptChanged {
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
            HookExecutionEventData::Finished { terminal, .. } => {
                if let Some(activity_id) = self.live_activity_id.lock().take() {
                    let terminal = match terminal {
                        HookExecutionTerminalData::Succeeded => ActivityTerminal::Succeeded,
                        HookExecutionTerminalData::Failed => ActivityTerminal::Failed,
                        HookExecutionTerminalData::Cancelled => ActivityTerminal::Cancelled,
                    };
                    let _ = self.activities.finish(activity_id, terminal);
                }
            }
        }
    }
}

fn hook_point_view(point: HookPointData) -> sdk::HookPointView {
    crate::application::hook::stop_coordination::hook_point_view(point)
}

pub(crate) fn subscription_activity_observer(
    activities: &ActivityCoordinator,
    run_step_id: &sdk::RunStepId,
) -> Option<Arc<dyn HookExecutionObserver>> {
    activities
        .live_hook_parent_id()
        .ok()
        .map(|parent_activity_id| {
            Arc::new(HookActivityObserver {
                activities: Arc::new(activities.clone()),
                run_step_id: run_step_id.clone(),
                parent_activity_id,
                live_activity_id: Mutex::new(None),
            }) as Arc<dyn HookExecutionObserver>
        })
}

/// 执行一次 Hook dispatch。生命周期展示只通过 ActivityCoordinator 发布。
///
/// `session_id` 是当前 Main Session id，写入 dispatch context 后由 Hook adapter
/// 注入子进程环境（`AEMEATH_SESSION_ID`），供外部集成捕获会话。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_hook(
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    run_step_id: &sdk::RunStepId,
    invocation: HookInvocationData,
    workspace_root: &Path,
    session_id: &str,
    cancel: &CancellationToken,
) -> RuntimeHookDispatch {
    let subscription_execution_observer = subscription_activity_observer(activities, run_step_id);
    let mut context = HookDispatchContextData::new(workspace_root).with_session_id(session_id);
    if let Some(observer) = subscription_execution_observer {
        context = context.with_subscription_execution_observer(observer);
    }
    let outcome = hook_port.dispatch_at(invocation, context, cancel).await;
    map_hook_outcome(&outcome)
}

pub(crate) fn dispatch_is_blocking(dispatch: &RuntimeHookDispatch) -> bool {
    matches!(dispatch.directive, RuntimeHookDirective::Block { .. })
}
