use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use hook::api::{HookData, StopHookData};
use share::config::hooks::HookEvent;
use tools::api::ToolContext;

pub(crate) async fn run_post_tool_batch<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    ctx: &ToolContext,
    turn_count: usize,
) where
    S: ChatEventSink,
{
    hook_runner.set_project_dir(ctx.workspace_read().current_root().display().to_string());
    let post_batch_results = hook_ui
        .run_json(
            hook_runner,
            HookEvent::PostToolBatch,
            None,
            HookData::Stop(StopHookData { turns: turn_count }),
        )
        .await;
    for (_entry, _result, json_output) in &post_batch_results {
        if let Some(json) = json_output {
            if let Some(ref ctx) = json.additional_context {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(ctx.clone()))
                    .await;
            }
            if let Some(ref msg) = json.system_message {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(msg.clone()))
                    .await;
            }
        }
    }
}
